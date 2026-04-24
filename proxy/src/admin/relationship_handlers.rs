//! Admin API handlers for `table_relationship`, `column_anchor`, and
//! `fk-suggestions` (live `pg_constraint` introspection).
//!
//! See `docs/security-vectors.md` → "Transitive tenancy bypass" for the
//! security model. `table_relationship` is catalog metadata (join-capable
//! FK tuples). `column_anchor` pins, per `(child_table, resolved_column)`,
//! which relationship the row-filter rewriter should walk to resolve a
//! missing column.
//!
//! Validation invariants enforced here:
//! - Relationships: referenced columns exist in `discovered_column`. The
//!   parent column *should* be a PK or single-column unique to preserve the
//!   at-most-one-parent-per-child guarantee the rewriter depends on, but
//!   this is NOT verified server-side on the direct POST path — only the
//!   UI-facing `fk-suggestions` endpoint filters by PK/unique. Each
//!   `create_relationship` emits a structured `tracing::info!` documenting
//!   the trust assumption so operators can correlate logs with the audit
//!   trail. See the vector-73 Status section for the full trade-off.
//! - Anchors: the referenced relationship's `child_table_id` matches the
//!   anchor's; delete-relationship is blocked while any anchor references it.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use std::collections::HashSet;
use uuid::Uuid;

use crate::admin::admin_audit::{AuditAction, AuditedTxn};
use crate::admin::catalog_handlers::{column_anchor_uuid, table_relationship_uuid};
use crate::admin::dto::{
    ColumnAnchorResponse, CreateColumnAnchorRequest, CreateTableRelationshipRequest,
    FkSuggestionResponse, ListRelationshipsQuery, TableRelationshipResponse,
};
use crate::admin::jwt::AdminClaims;
use crate::admin::{AdminState, ApiErr};
use crate::discovery;
use crate::engine::DataSourceConfig;
use crate::entity::{
    column_anchor, data_source, discovered_column, discovered_schema, discovered_table,
    table_relationship,
};

// ---------- helpers ----------

/// Load a discovered table row by id, scoped to the given datasource.
/// Returns `ApiErr::not_found` if the table is unknown or belongs to a
/// different datasource.
async fn load_table_in_datasource(
    state: &AdminState,
    datasource_id: Uuid,
    table_id: Uuid,
) -> Result<(discovered_table::Model, discovered_schema::Model), ApiErr> {
    let table = discovered_table::Entity::find_by_id(table_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Table not found"))?;
    let schema = discovered_schema::Entity::find_by_id(table.discovered_schema_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::internal("Schema row missing for discovered_table"))?;
    if schema.data_source_id != datasource_id {
        return Err(ApiErr::not_found("Table not found in this datasource"));
    }
    Ok((table, schema))
}

/// Verify a column exists on a discovered table.
///
/// Comparison is byte-for-byte against `discovered_column.column_name`.
/// PostgreSQL stores quoted identifiers with preserved case (`"MyCol"`) and
/// unquoted identifiers folded to lowercase (`mycol`), and discovery copies
/// the column name straight from `information_schema.columns` without
/// normalization — so callers must pass the exact spelling discovery saw.
/// An admin who types `mycol` when the discovered name is `"MyCol"` gets
/// a 422 here rather than a silent mismatch at query time.
async fn require_column(
    state: &AdminState,
    table_id: Uuid,
    column_name: &str,
) -> Result<(), ApiErr> {
    let exists = discovered_column::Entity::find()
        .filter(discovered_column::Column::DiscoveredTableId.eq(table_id))
        .filter(discovered_column::Column::ColumnName.eq(column_name))
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .is_some();
    if !exists {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "Column '{column_name}' not found on the selected table \
                 (column names are case-sensitive and must match exactly what \
                 discovery stored — check for quoted-identifier casing)"
            ),
        ));
    }
    Ok(())
}

async fn ds_name(state: &AdminState, datasource_id: Uuid) -> Result<String, ApiErr> {
    let ds = data_source::Entity::find_by_id(datasource_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;
    Ok(ds.name)
}

async fn invalidate_caches(state: &AdminState, ds_name: &str) {
    state.engine_cache.invalidate(ds_name).await;
    if let Some(hook) = &state.policy_hook {
        hook.invalidate_datasource(ds_name).await;
    }
    if let Some(ph) = &state.proxy_handler {
        ph.rebuild_contexts_for_datasource(ds_name);
    }
}

async fn build_relationship_response(
    state: &AdminState,
    model: &table_relationship::Model,
) -> Result<TableRelationshipResponse, ApiErr> {
    let (child_table, child_schema) =
        load_table_in_datasource(state, model.data_source_id, model.child_table_id).await?;
    let (parent_table, parent_schema) =
        load_table_in_datasource(state, model.data_source_id, model.parent_table_id).await?;
    Ok(TableRelationshipResponse {
        id: model.id,
        data_source_id: model.data_source_id,
        child_table_id: model.child_table_id,
        child_table_name: child_table.table_name,
        child_schema_name: child_schema.schema_name,
        child_column_name: model.child_column_name.clone(),
        parent_table_id: model.parent_table_id,
        parent_table_name: parent_table.table_name,
        parent_schema_name: parent_schema.schema_name,
        parent_column_name: model.parent_column_name.clone(),
        created_at: model.created_at,
        created_by: model.created_by,
    })
}

async fn build_anchor_response(
    state: &AdminState,
    model: &column_anchor::Model,
) -> Result<ColumnAnchorResponse, ApiErr> {
    let (child_table, _) =
        load_table_in_datasource(state, model.data_source_id, model.child_table_id).await?;
    Ok(ColumnAnchorResponse {
        id: model.id,
        data_source_id: model.data_source_id,
        child_table_id: model.child_table_id,
        child_table_name: child_table.table_name,
        resolved_column_name: model.resolved_column_name.clone(),
        relationship_id: model.relationship_id,
        actual_column_name: model.actual_column_name.clone(),
        designated_at: model.designated_at,
        designated_by: model.designated_by,
    })
}

// ---------- GET /datasources/{id}/relationships ----------

pub async fn list_relationships(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(datasource_id): Path<Uuid>,
    Query(params): Query<ListRelationshipsQuery>,
) -> Result<Json<Vec<TableRelationshipResponse>>, ApiErr> {
    // Verify datasource exists so we can return a clean 404 if misrouted.
    ds_name(&state, datasource_id).await?;

    let mut query = table_relationship::Entity::find()
        .filter(table_relationship::Column::DataSourceId.eq(datasource_id));
    if let Some(child) = params.child_table {
        query = query.filter(table_relationship::Column::ChildTableId.eq(child));
    }

    let rows = query
        .order_by_asc(table_relationship::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(build_relationship_response(&state, &r).await?);
    }
    Ok(Json(out))
}

// ---------- POST /datasources/{id}/relationships ----------

pub async fn create_relationship(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(datasource_id): Path<Uuid>,
    Json(body): Json<CreateTableRelationshipRequest>,
) -> Result<(StatusCode, Json<TableRelationshipResponse>), ApiErr> {
    let ds = data_source::Entity::find_by_id(datasource_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    if body.child_table_id == body.parent_table_id {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "child_table and parent_table must differ",
        ));
    }

    // Both tables must be in this datasource.
    let (child_tbl, child_schema_row) =
        load_table_in_datasource(&state, datasource_id, body.child_table_id).await?;
    let (parent_tbl, parent_schema_row) =
        load_table_in_datasource(&state, datasource_id, body.parent_table_id).await?;

    // Columns must exist in the catalog.
    require_column(&state, body.child_table_id, &body.child_column_name).await?;
    require_column(&state, body.parent_table_id, &body.parent_column_name).await?;

    let id = table_relationship_uuid(
        datasource_id,
        body.child_table_id,
        &body.child_column_name,
        body.parent_table_id,
        &body.parent_column_name,
    );

    // Idempotent: if the exact tuple already exists, return it.
    if let Some(existing) = table_relationship::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
    {
        let resp = build_relationship_response(&state, &existing).await?;
        return Ok((StatusCode::OK, Json(resp)));
    }

    let now = Utc::now().naive_utc();
    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let model = table_relationship::ActiveModel {
        id: Set(id),
        data_source_id: Set(datasource_id),
        child_table_id: Set(body.child_table_id),
        child_column_name: Set(body.child_column_name.clone()),
        parent_table_id: Set(body.parent_table_id),
        parent_column_name: Set(body.parent_column_name.clone()),
        created_at: Set(now),
        created_by: Set(Some(claims.sub)),
    }
    .insert(&*txn)
    .await
    .map_err(ApiErr::internal)?;

    txn.audit(
        "table_relationship",
        id,
        AuditAction::Create,
        claims.sub,
        serde_json::json!({
            "after": {
                "data_source_id": datasource_id,
                "child_table_id": body.child_table_id,
                "child_column_name": body.child_column_name,
                "parent_table_id": body.parent_table_id,
                "parent_column_name": body.parent_column_name,
            }
        }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

    // Defense-in-depth: document the trust assumption that the parent column
    // is a PK or single-column unique. The server does not verify this on the
    // direct POST path (only `fk-suggestions` filters by PK/unique); a
    // non-unique parent will cause the row-filter rewriter's INNER JOIN to
    // fan child rows. See vector 73 Status in `docs/security-vectors.md`.
    tracing::info!(
        datasource = %ds.name,
        relationship_id = %id,
        child = %format!("{}.{}", child_schema_row.schema_name, child_tbl.table_name),
        child_column = %body.child_column_name,
        parent = %format!("{}.{}", parent_schema_row.schema_name, parent_tbl.table_name),
        parent_column = %body.parent_column_name,
        "table_relationship created; parent column is assumed PK or single-column unique (not server-verified)"
    );

    invalidate_caches(&state, &ds.name).await;

    let resp = build_relationship_response(&state, &model).await?;
    Ok((StatusCode::CREATED, Json(resp)))
}

// ---------- DELETE /datasources/{id}/relationships/{rel_id} ----------

pub async fn delete_relationship(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path((datasource_id, rel_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let ds = data_source::Entity::find_by_id(datasource_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let rel = table_relationship::Entity::find_by_id(rel_id)
        .filter(table_relationship::Column::DataSourceId.eq(datasource_id))
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Relationship not found"))?;

    // Block delete if any column_anchor references this relationship.
    let referencing = column_anchor::Entity::find()
        .filter(column_anchor::Column::RelationshipId.eq(rel_id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;
    if !referencing.is_empty() {
        return Err(ApiErr::conflict(format!(
            "This relationship is used by {} column anchor(s); remove the anchors first.",
            referencing.len()
        )));
    }

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    txn.audit(
        "table_relationship",
        rel_id,
        AuditAction::Delete,
        claims.sub,
        serde_json::json!({
            "before": {
                "data_source_id": rel.data_source_id,
                "child_table_id": rel.child_table_id,
                "child_column_name": rel.child_column_name,
                "parent_table_id": rel.parent_table_id,
                "parent_column_name": rel.parent_column_name,
            }
        }),
    );

    let active: table_relationship::ActiveModel = rel.into();
    active.delete(&*txn).await.map_err(ApiErr::internal)?;

    txn.commit().await.map_err(ApiErr::internal)?;

    invalidate_caches(&state, &ds.name).await;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- GET /datasources/{id}/column-anchors ----------

pub async fn list_column_anchors(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(datasource_id): Path<Uuid>,
) -> Result<Json<Vec<ColumnAnchorResponse>>, ApiErr> {
    ds_name(&state, datasource_id).await?;

    let rows = column_anchor::Entity::find()
        .filter(column_anchor::Column::DataSourceId.eq(datasource_id))
        .order_by_asc(column_anchor::Column::DesignatedAt)
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(build_anchor_response(&state, &r).await?);
    }
    Ok(Json(out))
}

// ---------- POST /datasources/{id}/column-anchors ----------

pub async fn create_column_anchor(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(datasource_id): Path<Uuid>,
    Json(body): Json<CreateColumnAnchorRequest>,
) -> Result<(StatusCode, Json<ColumnAnchorResponse>), ApiErr> {
    let ds = data_source::Entity::find_by_id(datasource_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    if body.resolved_column_name.trim().is_empty() {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "resolved_column_name must be non-empty",
        ));
    }

    // XOR: exactly one of `relationship_id` / `actual_column_name` must be set.
    // The two shapes are mutually exclusive — FK walk vs same-table alias.
    let actual_column_name = body
        .actual_column_name
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match (body.relationship_id, actual_column_name.as_ref()) {
        (Some(_), Some(_)) => {
            return Err(ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Provide exactly one of 'relationship_id' or 'actual_column_name', not both",
            ));
        }
        (None, None) => {
            return Err(ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Provide exactly one of 'relationship_id' (FK walk) or \
                 'actual_column_name' (same-table alias)",
            ));
        }
        _ => {}
    }

    // Child table must belong to this datasource.
    load_table_in_datasource(&state, datasource_id, body.child_table_id).await?;

    // Relationship shape: validate the relationship exists and matches the
    // child table. Alias shape: no save-time check on `actual_column_name`
    // (per design — query-time deny-wins is the safety net).
    if let Some(rel_id) = body.relationship_id {
        let rel = table_relationship::Entity::find_by_id(rel_id)
            .one(&state.db)
            .await
            .map_err(ApiErr::internal)?
            .ok_or_else(|| ApiErr::not_found("Relationship not found"))?;
        if rel.data_source_id != datasource_id {
            return Err(ApiErr::not_found(
                "Relationship does not belong to this datasource",
            ));
        }
        if rel.child_table_id != body.child_table_id {
            return Err(ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "relationship.child_table_id does not match anchor.child_table_id",
            ));
        }
    }

    let id = column_anchor_uuid(
        datasource_id,
        body.child_table_id,
        &body.resolved_column_name,
    );

    if column_anchor::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .is_some()
    {
        return Err(ApiErr::conflict(format!(
            "An anchor for column '{}' on this table already exists.",
            body.resolved_column_name
        )));
    }

    let now = Utc::now().naive_utc();
    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let model = column_anchor::ActiveModel {
        id: Set(id),
        data_source_id: Set(datasource_id),
        child_table_id: Set(body.child_table_id),
        resolved_column_name: Set(body.resolved_column_name.clone()),
        relationship_id: Set(body.relationship_id),
        actual_column_name: Set(actual_column_name.clone()),
        designated_at: Set(now),
        designated_by: Set(claims.sub),
    }
    .insert(&*txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict(format!(
                "An anchor for column '{}' on this table already exists.",
                body.resolved_column_name
            ))
        } else {
            ApiErr::internal(e)
        }
    })?;

    let mut after = serde_json::Map::new();
    after.insert(
        "data_source_id".to_string(),
        serde_json::json!(datasource_id),
    );
    after.insert(
        "child_table_id".to_string(),
        serde_json::json!(body.child_table_id),
    );
    after.insert(
        "resolved_column_name".to_string(),
        serde_json::json!(body.resolved_column_name),
    );
    if let Some(rel_id) = body.relationship_id {
        after.insert("relationship_id".to_string(), serde_json::json!(rel_id));
    }
    if let Some(actual) = &actual_column_name {
        after.insert("actual_column_name".to_string(), serde_json::json!(actual));
    }

    txn.audit(
        "column_anchor",
        id,
        AuditAction::Create,
        claims.sub,
        serde_json::json!({ "after": after }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

    invalidate_caches(&state, &ds.name).await;

    let resp = build_anchor_response(&state, &model).await?;
    Ok((StatusCode::CREATED, Json(resp)))
}

// ---------- DELETE /datasources/{id}/column-anchors/{anchor_id} ----------

pub async fn delete_column_anchor(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path((datasource_id, anchor_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiErr> {
    let ds = data_source::Entity::find_by_id(datasource_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let anchor = column_anchor::Entity::find_by_id(anchor_id)
        .filter(column_anchor::Column::DataSourceId.eq(datasource_id))
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Column anchor not found"))?;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    txn.audit(
        "column_anchor",
        anchor_id,
        AuditAction::Delete,
        claims.sub,
        serde_json::json!({
            "before": {
                "data_source_id": anchor.data_source_id,
                "child_table_id": anchor.child_table_id,
                "resolved_column_name": anchor.resolved_column_name,
                "relationship_id": anchor.relationship_id,
            }
        }),
    );

    let active: column_anchor::ActiveModel = anchor.into();
    active.delete(&*txn).await.map_err(ApiErr::internal)?;

    txn.commit().await.map_err(ApiErr::internal)?;

    invalidate_caches(&state, &ds.name).await;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- GET /datasources/{id}/fk-suggestions ----------

/// Live `pg_constraint` introspection. Returns single-column FKs whose parent
/// column is a PK or single-column unique, filtered to tables in the current
/// discovered catalog. Marks each suggestion with `already_added` so the UI
/// can grey out or hide suggestions that the admin has already promoted.
pub async fn fk_suggestions(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(datasource_id): Path<Uuid>,
) -> Result<Json<Vec<FkSuggestionResponse>>, ApiErr> {
    let ds = data_source::Entity::find_by_id(datasource_id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    // Load the current catalog of selected (schema, table) pairs so the
    // discovery SQL can filter to what the admin actually chose.
    let schemas: Vec<discovered_schema::Model> = discovered_schema::Entity::find()
        .filter(discovered_schema::Column::DataSourceId.eq(datasource_id))
        .filter(discovered_schema::Column::IsSelected.eq(true))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;
    let schema_id_to_name: std::collections::HashMap<Uuid, String> = schemas
        .iter()
        .map(|s| (s.id, s.schema_name.clone()))
        .collect();
    let schema_ids: Vec<Uuid> = schemas.iter().map(|s| s.id).collect();

    if schema_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let tables: Vec<discovered_table::Model> = discovered_table::Entity::find()
        .filter(discovered_table::Column::DiscoveredSchemaId.is_in(schema_ids))
        .filter(discovered_table::Column::IsSelected.eq(true))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;
    if tables.is_empty() {
        return Ok(Json(vec![]));
    }

    // (schema_name, table_name) -> discovered_table.id for lookup after introspection.
    let mut table_id_by_pair: std::collections::HashMap<(String, String), Uuid> =
        std::collections::HashMap::new();
    let mut provider_tables: Vec<(String, String)> = Vec::with_capacity(tables.len());
    for t in &tables {
        let schema_name = match schema_id_to_name.get(&t.discovered_schema_id) {
            Some(n) => n.clone(),
            None => continue,
        };
        provider_tables.push((schema_name.clone(), t.table_name.clone()));
        table_id_by_pair.insert((schema_name, t.table_name.clone()), t.id);
    }

    // Live-introspect FKs.
    let cfg = DataSourceConfig::from_model(&ds, &state.master_key).map_err(ApiErr::internal)?;
    let provider = discovery::create_provider(&ds.ds_type, cfg)
        .map_err(|e| ApiErr::internal(e.to_string()))?;
    let cancel = tokio_util::sync::CancellationToken::new();
    let discovered = provider
        .discover_foreign_keys(&provider_tables, &cancel)
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))?;

    // Build "already added" set keyed by (child_table_id, child_col, parent_table_id, parent_col).
    let existing: Vec<table_relationship::Model> = table_relationship::Entity::find()
        .filter(table_relationship::Column::DataSourceId.eq(datasource_id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;
    let added: HashSet<(Uuid, String, Uuid, String)> = existing
        .into_iter()
        .map(|r| {
            (
                r.child_table_id,
                r.child_column_name,
                r.parent_table_id,
                r.parent_column_name,
            )
        })
        .collect();

    // Map each introspected FK to catalog ids, dropping any whose endpoints
    // aren't in the selected catalog (shouldn't happen since the SQL filters,
    // but defensive).
    let mut out: Vec<FkSuggestionResponse> = Vec::new();
    for fk in discovered {
        let child_id =
            match table_id_by_pair.get(&(fk.child_schema.clone(), fk.child_table.clone())) {
                Some(id) => *id,
                None => continue,
            };
        let parent_id =
            match table_id_by_pair.get(&(fk.parent_schema.clone(), fk.parent_table.clone())) {
                Some(id) => *id,
                None => continue,
            };

        let key = (
            child_id,
            fk.child_column.clone(),
            parent_id,
            fk.parent_column.clone(),
        );
        let already_added = added.contains(&key);

        out.push(FkSuggestionResponse {
            child_table_id: child_id,
            child_schema_name: fk.child_schema,
            child_table_name: fk.child_table,
            child_column_name: fk.child_column,
            parent_table_id: parent_id,
            parent_schema_name: fk.parent_schema,
            parent_table_name: fk.parent_table,
            parent_column_name: fk.parent_column,
            fk_constraint_name: fk.fk_constraint_name,
            already_added,
        });
    }

    Ok(Json(out))
}
