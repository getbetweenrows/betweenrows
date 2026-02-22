use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Json,
    },
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait,
};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::admin::discovery_job::{DiscoveryEvent, DiscoveryJob, DiscoveryRequest};
use crate::admin::{AdminState, ApiErr};
use crate::admin::dto::*;
use crate::admin::jwt::AdminClaims;
use crate::discovery;
use crate::engine::DataSourceConfig;
use crate::entity::{data_source, discovered_column, discovered_schema, discovered_table};

// ---------- POST /datasources/{id}/discover — submit a job ----------

pub async fn submit_discovery(
    _claims: AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(request): Json<DiscoveryRequest>,
) -> Result<(StatusCode, Json<SubmitDiscoveryResponse>), ApiErr> {
    // Verify datasource exists
    let ds = data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let action = match &request {
        DiscoveryRequest::DiscoverSchemas => "discover_schemas",
        DiscoveryRequest::DiscoverTables { .. } => "discover_tables",
        DiscoveryRequest::DiscoverColumns { .. } => "discover_columns",
        DiscoveryRequest::SaveCatalog { .. } => "save_catalog",
        DiscoveryRequest::SyncCatalog => "sync_catalog",
    };

    let job = DiscoveryJob::new(id, action.to_string());
    let job_id = job.id.clone();
    let cancel = job.cancel.clone();
    let tx = job.tx.clone();

    {
        let mut store = state.job_store.lock().await;
        store.try_register(job).map_err(|existing_id| {
            ApiErr::new(
                StatusCode::CONFLICT,
                format!("Discovery already in progress (job_id: {existing_id})"),
            )
        })?;
    }

    // Spawn the runner
    let state_clone = state.clone();
    let ds_name = ds.name.clone();
    let job_id_clone = job_id.clone();
    let request_clone = request.clone();

    tokio::spawn(async move {
        run_discovery_job(
            &state_clone,
            id,
            ds_name,
            request_clone,
            job_id_clone.clone(),
            tx,
            cancel,
        )
        .await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(SubmitDiscoveryResponse { job_id }),
    ))
}

// ---------- Core runner — HTTP-independent ----------

async fn run_discovery_job(
    state: &AdminState,
    datasource_id: Uuid,
    ds_name: String,
    request: DiscoveryRequest,
    job_id: String,
    tx: tokio::sync::broadcast::Sender<DiscoveryEvent>,
    cancel: tokio_util::sync::CancellationToken,
) {
    let send = |event: DiscoveryEvent| {
        let _ = tx.send(event);
    };

    let result = run_inner(state, datasource_id, &ds_name, request, &tx, &cancel).await;

    match result {
        Ok(data) => {
            send(DiscoveryEvent::Result { data: data.clone() });
            send(DiscoveryEvent::Done);
            let mut store = state.job_store.lock().await;
            store.complete(&job_id, data);
        }
        Err(e) if cancel.is_cancelled() => {
            send(DiscoveryEvent::Cancelled);
            let mut store = state.job_store.lock().await;
            store.fail(&job_id, "cancelled".to_string());
        }
        Err(e) => {
            let msg = e.to_string();
            send(DiscoveryEvent::Error { message: msg.clone() });
            let mut store = state.job_store.lock().await;
            store.fail(&job_id, msg);
        }
    }
}

async fn run_inner(
    state: &AdminState,
    datasource_id: Uuid,
    ds_name: &str,
    request: DiscoveryRequest,
    tx: &tokio::sync::broadcast::Sender<DiscoveryEvent>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let send = |event: DiscoveryEvent| {
        let _ = tx.send(event);
    };

    match request {
        DiscoveryRequest::DiscoverSchemas => {
            send(progress("connecting", "Connecting to upstream database…"));

            let ds = data_source::Entity::find_by_id(datasource_id)
                .one(&state.db)
                .await?
                .ok_or("Data source not found")?;
            let cfg = DataSourceConfig::from_model(&ds, &state.master_key)?;
            let provider = discovery::create_provider(&ds.ds_type, cfg)?;

            send(progress("querying", "Querying schemas…"));

            let discovered = provider.discover_schemas(cancel).await?;

            // Load existing selected schemas for cross-reference
            let existing: Vec<discovered_schema::Model> = discovered_schema::Entity::find()
                .filter(discovered_schema::Column::DataSourceId.eq(datasource_id))
                .filter(discovered_schema::Column::IsSelected.eq(true))
                .all(&state.db)
                .await?;

            let selected_names: std::collections::HashSet<String> =
                existing.into_iter().map(|s| s.schema_name).collect();

            let response: Vec<DiscoveredSchemaResponse> = discovered
                .into_iter()
                .map(|s| DiscoveredSchemaResponse {
                    is_already_selected: selected_names.contains(&s.schema_name),
                    schema_name: s.schema_name,
                })
                .collect();

            Ok(serde_json::to_value(response)?)
        }

        DiscoveryRequest::DiscoverTables { schemas } => {
            send(progress("connecting", "Connecting to upstream database…"));

            let ds = data_source::Entity::find_by_id(datasource_id)
                .one(&state.db)
                .await?
                .ok_or("Data source not found")?;
            let cfg = DataSourceConfig::from_model(&ds, &state.master_key)?;
            let provider = discovery::create_provider(&ds.ds_type, cfg)?;

            send(progress("querying", "Querying tables…"));

            let discovered = provider.discover_tables(&schemas, cancel).await?;

            // Load existing selected tables for cross-reference
            let schemas_with_tables: Vec<(discovered_schema::Model, Vec<discovered_table::Model>)> =
                discovered_schema::Entity::find()
                    .filter(discovered_schema::Column::DataSourceId.eq(datasource_id))
                    .find_with_related(discovered_table::Entity)
                    .all(&state.db)
                    .await?;

            let selected_tables: std::collections::HashSet<(String, String)> = schemas_with_tables
                .into_iter()
                .flat_map(|(schema, tables)| {
                    tables
                        .into_iter()
                        .filter(|t| t.is_selected)
                        .map(move |t| (schema.schema_name.clone(), t.table_name))
                })
                .collect();

            let response: Vec<DiscoveredTableResponse> = discovered
                .into_iter()
                .map(|t| DiscoveredTableResponse {
                    is_already_selected: selected_tables
                        .contains(&(t.schema_name.clone(), t.table_name.clone())),
                    schema_name: t.schema_name,
                    table_name: t.table_name,
                    table_type: t.table_type,
                })
                .collect();

            Ok(serde_json::to_value(response)?)
        }

        DiscoveryRequest::DiscoverColumns { tables } => {
            send(progress("connecting", "Connecting to upstream database…"));

            let ds = data_source::Entity::find_by_id(datasource_id)
                .one(&state.db)
                .await?
                .ok_or("Data source not found")?;
            let cfg = DataSourceConfig::from_model(&ds, &state.master_key)?;
            let provider = discovery::create_provider(&ds.ds_type, cfg)?;

            send(progress("querying", "Querying columns…"));

            let pairs: Vec<(String, String)> = tables
                .into_iter()
                .map(|t| (t.schema, t.table))
                .collect();

            let discovered = provider.discover_columns(&pairs, cancel).await?;

            let response: Vec<DiscoveredColumnResponse> = discovered
                .into_iter()
                .map(|c| DiscoveredColumnResponse {
                    schema_name: c.schema_name,
                    table_name: c.table_name,
                    column_name: c.column_name,
                    ordinal_position: c.ordinal_position,
                    data_type: c.data_type,
                    is_nullable: c.is_nullable,
                    column_default: c.column_default,
                    arrow_type: c.arrow_type,
                })
                .collect();

            Ok(serde_json::to_value(response)?)
        }

        DiscoveryRequest::SaveCatalog { schemas } => {
            send(progress("saving", "Saving catalog selections…"));

            // Discover columns for selected tables
            let ds = data_source::Entity::find_by_id(datasource_id)
                .one(&state.db)
                .await?
                .ok_or("Data source not found")?;
            let cfg = DataSourceConfig::from_model(&ds, &state.master_key)?;

            let txn = state.db.begin().await?;
            let now = Utc::now().naive_utc();

            // Collect selected tables for column discovery
            let mut tables_needing_columns: Vec<(String, String, Uuid)> = Vec::new();

            for schema_sel in &schemas {
                // Upsert schema (use deterministic UUID v5)
                let schema_uuid = catalog_schema_uuid(datasource_id, &schema_sel.schema_name);

                let existing_schema = discovered_schema::Entity::find_by_id(schema_uuid)
                    .one(&txn)
                    .await?;

                if let Some(existing) = existing_schema {
                    let mut active: discovered_schema::ActiveModel = existing.into();
                    active.is_selected = Set(schema_sel.is_selected);
                    active.discovered_at = Set(now);
                    active.update(&txn).await?;
                } else {
                    discovered_schema::ActiveModel {
                        id: Set(schema_uuid),
                        data_source_id: Set(datasource_id),
                        schema_name: Set(schema_sel.schema_name.clone()),
                        is_selected: Set(schema_sel.is_selected),
                        discovered_at: Set(now),
                    }
                    .insert(&txn)
                    .await?;
                }

                for table_sel in &schema_sel.tables {
                    let table_uuid = catalog_table_uuid(schema_uuid, &table_sel.table_name);

                    let existing_table = discovered_table::Entity::find_by_id(table_uuid)
                        .one(&txn)
                        .await?;

                    if let Some(existing) = existing_table {
                        let mut active: discovered_table::ActiveModel = existing.into();
                        active.is_selected = Set(table_sel.is_selected);
                        active.table_type = Set(table_sel.table_type.clone());
                        active.discovered_at = Set(now);
                        active.update(&txn).await?;
                    } else {
                        discovered_table::ActiveModel {
                            id: Set(table_uuid),
                            discovered_schema_id: Set(schema_uuid),
                            table_name: Set(table_sel.table_name.clone()),
                            table_type: Set(table_sel.table_type.clone()),
                            is_selected: Set(table_sel.is_selected),
                            discovered_at: Set(now),
                        }
                        .insert(&txn)
                        .await?;
                    }

                    if table_sel.is_selected && schema_sel.is_selected {
                        tables_needing_columns.push((
                            schema_sel.schema_name.clone(),
                            table_sel.table_name.clone(),
                            table_uuid,
                        ));
                    }
                }

                // Delete tables not in selection
                let selected_table_names: Vec<String> = schema_sel
                    .tables
                    .iter()
                    .map(|t| t.table_name.clone())
                    .collect();

                let all_tables: Vec<discovered_table::Model> = discovered_table::Entity::find()
                    .filter(discovered_table::Column::DiscoveredSchemaId.eq(schema_uuid))
                    .all(&txn)
                    .await?;

                for table in all_tables {
                    if !selected_table_names.contains(&table.table_name) {
                        discovered_table::Entity::delete_by_id(table.id)
                            .exec(&txn)
                            .await?;
                    }
                }
            }

            // Delete schemas not in selection
            let selected_schema_names: Vec<String> =
                schemas.iter().map(|s| s.schema_name.clone()).collect();

            let all_schemas: Vec<discovered_schema::Model> = discovered_schema::Entity::find()
                .filter(discovered_schema::Column::DataSourceId.eq(datasource_id))
                .all(&txn)
                .await?;

            for schema in all_schemas {
                if !selected_schema_names.contains(&schema.schema_name) {
                    discovered_schema::Entity::delete_by_id(schema.id)
                        .exec(&txn)
                        .await?;
                }
            }

            txn.commit().await?;

            // Run column discovery for selected tables
            if !tables_needing_columns.is_empty() {
                send(progress("connecting", "Connecting to upstream database for column discovery…"));

                let provider = discovery::create_provider(&ds.ds_type, cfg)?;

                let pairs: Vec<(String, String)> = tables_needing_columns
                    .iter()
                    .map(|(s, t, _)| (s.clone(), t.clone()))
                    .collect();

                send(progress("querying", &format!("Discovering columns for {} tables…", pairs.len())));

                match provider.discover_columns(&pairs, cancel).await {
                    Ok(columns) => {
                        let now2 = Utc::now().naive_utc();
                        let table_id_map: std::collections::HashMap<(String, String), Uuid> =
                            tables_needing_columns
                                .iter()
                                .map(|(s, t, id)| ((s.clone(), t.clone()), *id))
                                .collect();

                        for col in columns {
                            let key = (col.schema_name.clone(), col.table_name.clone());
                            if let Some(&table_id) = table_id_map.get(&key) {
                                let col_uuid = catalog_column_uuid(table_id, &col.column_name);

                                let existing = discovered_column::Entity::find_by_id(col_uuid)
                                    .one(&state.db)
                                    .await?;

                                if let Some(existing) = existing {
                                    let mut active: discovered_column::ActiveModel = existing.into();
                                    active.ordinal_position = Set(col.ordinal_position);
                                    active.data_type = Set(col.data_type);
                                    active.is_nullable = Set(col.is_nullable);
                                    active.column_default = Set(col.column_default);
                                    active.arrow_type = Set(col.arrow_type);
                                    active.discovered_at = Set(now2);
                                    active.update(&state.db).await?;
                                } else {
                                    discovered_column::ActiveModel {
                                        id: Set(col_uuid),
                                        discovered_table_id: Set(table_id),
                                        column_name: Set(col.column_name),
                                        ordinal_position: Set(col.ordinal_position),
                                        data_type: Set(col.data_type),
                                        is_nullable: Set(col.is_nullable),
                                        column_default: Set(col.column_default),
                                        arrow_type: Set(col.arrow_type),
                                        discovered_at: Set(now2),
                                    }
                                    .insert(&state.db)
                                    .await?;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Column discovery failed: {e}");
                    }
                }
            }

            // Invalidate engine cache
            state.engine_cache.invalidate(ds_name).await;

            Ok(serde_json::json!({ "ok": true }))
        }

        DiscoveryRequest::SyncCatalog => {
            send(progress("connecting", "Connecting to upstream database…"));

            let ds = data_source::Entity::find_by_id(datasource_id)
                .one(&state.db)
                .await?
                .ok_or("Data source not found")?;
            let cfg = DataSourceConfig::from_model(&ds, &state.master_key)?;
            let provider = discovery::create_provider(&ds.ds_type, cfg)?;

            // Load selected schemas
            let existing_schemas: Vec<discovered_schema::Model> = discovered_schema::Entity::find()
                .filter(discovered_schema::Column::DataSourceId.eq(datasource_id))
                .filter(discovered_schema::Column::IsSelected.eq(true))
                .all(&state.db)
                .await?;

            let selected_schema_names: Vec<String> =
                existing_schemas.iter().map(|s| s.schema_name.clone()).collect();

            send(progress("querying", "Querying live schemas…"));
            let live_schemas = provider.discover_schemas(cancel).await?;
            let live_schema_names: std::collections::HashSet<String> =
                live_schemas.into_iter().map(|s| s.schema_name).collect();

            send(progress("querying", "Querying live tables…"));
            let live_tables = provider.discover_tables(&selected_schema_names, cancel).await?;
            let live_table_set: std::collections::HashSet<(String, String)> = live_tables
                .iter()
                .map(|t| (t.schema_name.clone(), t.table_name.clone()))
                .collect();

            // Load existing selected tables
            let existing_tables_by_schema: Vec<(discovered_schema::Model, Vec<discovered_table::Model>)> =
                discovered_schema::Entity::find()
                    .filter(discovered_schema::Column::DataSourceId.eq(datasource_id))
                    .filter(discovered_schema::Column::IsSelected.eq(true))
                    .find_with_related(discovered_table::Entity)
                    .all(&state.db)
                    .await?;

            let mut added_schemas = Vec::new();
            let mut removed_schemas = Vec::new();
            let mut added_tables = Vec::new();
            let mut removed_tables = Vec::new();
            let mut changed_columns = Vec::new();
            let mut selected_table_pairs: Vec<(String, String)> = Vec::new();

            for schema_name in &selected_schema_names {
                if !live_schema_names.contains(schema_name) {
                    removed_schemas.push(schema_name.clone());
                }
            }

            for (schema, tables) in &existing_tables_by_schema {
                for table in tables {
                    if table.is_selected {
                        let key = (schema.schema_name.clone(), table.table_name.clone());
                        if !live_table_set.contains(&key) {
                            removed_tables.push(format!("{}.{}", schema.schema_name, table.table_name));
                        } else {
                            selected_table_pairs.push(key);
                        }
                    }
                }
            }

            // Detect new tables
            for live_table in &live_tables {
                let key = (live_table.schema_name.clone(), live_table.table_name.clone());
                if !selected_table_pairs.contains(&key) {
                    added_tables.push(format!("{}.{}", live_table.schema_name, live_table.table_name));
                }
            }

            // Detect new schemas
            for schema_name in &live_schema_names {
                if !selected_schema_names.contains(schema_name) {
                    added_schemas.push(schema_name.clone());
                }
            }
            added_schemas.dedup();

            // Discover columns for selected tables
            if !selected_table_pairs.is_empty() {
                send(progress("querying", &format!("Querying columns for {} tables…", selected_table_pairs.len())));

                let live_columns = provider.discover_columns(&selected_table_pairs, cancel).await?;
                let now = Utc::now().naive_utc();

                for (schema, tables) in &existing_tables_by_schema {
                    for table in tables {
                        if !table.is_selected {
                            continue;
                        }

                        let existing_cols: Vec<discovered_column::Model> = discovered_column::Entity::find()
                            .filter(discovered_column::Column::DiscoveredTableId.eq(table.id))
                            .all(&state.db)
                            .await?;

                        let existing_col_map: std::collections::HashMap<String, &discovered_column::Model> =
                            existing_cols.iter().map(|c| (c.column_name.clone(), c)).collect();

                        let live_cols_for_table: Vec<_> = live_columns
                            .iter()
                            .filter(|c| {
                                c.schema_name == schema.schema_name && c.table_name == table.table_name
                            })
                            .collect();

                        for live_col in &live_cols_for_table {
                            if let Some(existing) = existing_col_map.get(&live_col.column_name) {
                                if existing.data_type != live_col.data_type
                                    || existing.is_nullable != live_col.is_nullable
                                    || existing.arrow_type != live_col.arrow_type
                                {
                                    changed_columns.push(format!(
                                        "{}.{}.{}",
                                        schema.schema_name, table.table_name, live_col.column_name
                                    ));

                                    let mut active: discovered_column::ActiveModel =
                                        (*existing).clone().into();
                                    active.data_type = Set(live_col.data_type.clone());
                                    active.is_nullable = Set(live_col.is_nullable);
                                    active.arrow_type = Set(live_col.arrow_type.clone());
                                    active.discovered_at = Set(now);
                                    active.update(&state.db).await?;
                                }
                            } else {
                                let col_uuid = catalog_column_uuid(table.id, &live_col.column_name);
                                let new_col = discovered_column::ActiveModel {
                                    id: Set(col_uuid),
                                    discovered_table_id: Set(table.id),
                                    column_name: Set(live_col.column_name.clone()),
                                    ordinal_position: Set(live_col.ordinal_position),
                                    data_type: Set(live_col.data_type.clone()),
                                    is_nullable: Set(live_col.is_nullable),
                                    column_default: Set(live_col.column_default.clone()),
                                    arrow_type: Set(live_col.arrow_type.clone()),
                                    discovered_at: Set(now),
                                };
                                new_col.insert(&state.db).await?;
                                changed_columns.push(format!(
                                    "{}.{}.{} (new)",
                                    schema.schema_name, table.table_name, live_col.column_name
                                ));
                            }
                        }
                    }
                }
            }

            // Update last_sync_at and last_sync_result on the datasource
            let has_breaking = !removed_schemas.is_empty() || !removed_tables.is_empty();
            let drift_report = serde_json::json!({
                "added_schemas": added_schemas,
                "removed_schemas": removed_schemas,
                "added_tables": added_tables,
                "removed_tables": removed_tables,
                "changed_columns": changed_columns,
                "has_breaking_changes": has_breaking,
            });

            let mut ds_active: data_source::ActiveModel = ds.into();
            ds_active.last_sync_at = Set(Some(Utc::now().naive_utc()));
            ds_active.last_sync_result = Set(Some(drift_report.to_string()));
            ds_active.update(&state.db).await?;

            state.engine_cache.invalidate(ds_name).await;

            Ok(drift_report)
        }
    }
}

fn progress(phase: &str, detail: &str) -> DiscoveryEvent {
    DiscoveryEvent::Progress {
        phase: phase.to_string(),
        detail: detail.to_string(),
    }
}

// ---------- UUID v5 helpers for deterministic catalog IDs ----------

/// Namespace UUID for catalog fingerprints (UUID v5).
const CATALOG_NS: Uuid = Uuid::from_bytes([
    0x8a, 0x1b, 0x9c, 0x4e, 0x3d, 0x7f, 0x5a, 0x21,
    0xb6, 0x0e, 0xf4, 0x12, 0x7c, 0x8d, 0x9e, 0x03,
]);

fn catalog_schema_uuid(datasource_id: Uuid, schema_name: &str) -> Uuid {
    let key = format!("{datasource_id}:{schema_name}");
    Uuid::new_v5(&CATALOG_NS, key.as_bytes())
}

fn catalog_table_uuid(schema_id: Uuid, table_name: &str) -> Uuid {
    let key = format!("{schema_id}:{table_name}");
    Uuid::new_v5(&CATALOG_NS, key.as_bytes())
}

fn catalog_column_uuid(table_id: Uuid, column_name: &str) -> Uuid {
    let key = format!("{table_id}:{column_name}");
    Uuid::new_v5(&CATALOG_NS, key.as_bytes())
}

// ---------- GET /datasources/{id}/discover/{job_id}/events — SSE stream ----------

pub async fn discovery_events(
    _claims: AdminClaims,
    State(state): State<AdminState>,
    Path((ds_id, job_id)): Path<(Uuid, String)>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    // Verify datasource exists
    data_source::Entity::find_by_id(ds_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let rx = {
        let store = state.job_store.lock().await;
        match store.subscribe(&job_id) {
            Some(rx) => rx,
            None => {
                return Err(ApiErr::not_found("Job not found"));
            }
        }
    };

    let stream = BroadcastStream::new(rx)
        .filter_map(|result| {
            match result {
                Ok(event) => {
                    let sse_event = event.to_sse_event().ok()?;
                    Some(Ok(sse_event))
                }
                Err(_) => None, // lagged — skip
            }
        });

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

// ---------- DELETE /datasources/{id}/discover/{job_id} — cancel ----------

pub async fn cancel_discovery(
    _claims: AdminClaims,
    State(state): State<AdminState>,
    Path((_ds_id, job_id)): Path<(Uuid, String)>,
) -> Result<StatusCode, ApiErr> {
    let cancelled = state.job_store.lock().await.cancel(&job_id);
    if cancelled {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiErr::not_found("Job not found or not running"))
    }
}

// ---------- GET /datasources/{id}/discover/{job_id} — poll status ----------

pub async fn discovery_status(
    _claims: AdminClaims,
    State(state): State<AdminState>,
    Path((_ds_id, job_id)): Path<(Uuid, String)>,
) -> Result<Json<JobStatusResponse>, ApiErr> {
    let store = state.job_store.lock().await;
    let job = store.get(&job_id).ok_or_else(|| ApiErr::not_found("Job not found"))?;

    Ok(Json(JobStatusResponse {
        job_id: job.id.clone(),
        action: job.action.clone(),
        status: job.status.as_str().to_string(),
        result: job.result.clone(),
        error: job.error.clone(),
    }))
}

// ---------- GET /datasources/{id}/catalog — read stored catalog ----------

pub async fn get_catalog(
    _claims: AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CatalogResponse>, ApiErr> {
    // Verify data source exists
    data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let schemas_with_tables: Vec<(discovered_schema::Model, Vec<discovered_table::Model>)> =
        discovered_schema::Entity::find()
            .filter(discovered_schema::Column::DataSourceId.eq(id))
            .find_with_related(discovered_table::Entity)
            .all(&state.db)
            .await
            .map_err(ApiErr::internal)?;

    let mut schema_responses = Vec::new();

    for (schema, tables) in schemas_with_tables {
        let mut table_responses = Vec::new();

        for table in tables {
            let columns: Vec<discovered_column::Model> = discovered_column::Entity::find()
                .filter(discovered_column::Column::DiscoveredTableId.eq(table.id))
                .all(&state.db)
                .await
                .map_err(ApiErr::internal)?;

            let column_responses: Vec<CatalogColumnResponse> = columns
                .into_iter()
                .map(|c| CatalogColumnResponse {
                    id: c.id,
                    column_name: c.column_name,
                    ordinal_position: c.ordinal_position,
                    data_type: c.data_type,
                    is_nullable: c.is_nullable,
                    column_default: c.column_default,
                    arrow_type: c.arrow_type,
                })
                .collect();

            table_responses.push(CatalogTableResponse {
                id: table.id,
                table_name: table.table_name,
                table_type: table.table_type,
                is_selected: table.is_selected,
                columns: column_responses,
            });
        }

        schema_responses.push(CatalogSchemaResponse {
            id: schema.id,
            schema_name: schema.schema_name,
            is_selected: schema.is_selected,
            tables: table_responses,
        });
    }

    Ok(Json(CatalogResponse { schemas: schema_responses }))
}
