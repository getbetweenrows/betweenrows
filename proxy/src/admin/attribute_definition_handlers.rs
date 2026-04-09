use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, Statement,
};
use uuid::Uuid;

use crate::entity::{attribute_definition, proxy_user};

use super::{
    AdminState, ApiErr,
    admin_audit::{AuditAction, AuditedTxn},
    dto::{
        AttributeDefinitionResponse, CreateAttributeDefinitionRequest,
        DeleteAttributeDefinitionQuery, ListAttributeDefinitionsQuery, PaginatedResponse,
        UpdateAttributeDefinitionRequest, validate_attribute_definition,
    },
    jwt::AdminClaims,
};

// ---------- GET /attribute-definitions ----------

pub async fn list_attribute_definitions(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Query(params): Query<ListAttributeDefinitionsQuery>,
) -> Result<Json<PaginatedResponse<AttributeDefinitionResponse>>, ApiErr> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).clamp(1, 200);

    let mut query = attribute_definition::Entity::find();
    if let Some(ref et) = params.entity_type {
        query = query.filter(attribute_definition::Column::EntityType.eq(et.as_str()));
    }

    let total = query
        .clone()
        .count(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let items = query
        .order_by_asc(attribute_definition::Column::Key)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let data: Vec<AttributeDefinitionResponse> = items.into_iter().map(Into::into).collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        page_size,
    }))
}

// ---------- POST /attribute-definitions ----------

pub async fn create_attribute_definition(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Json(body): Json<CreateAttributeDefinitionRequest>,
) -> Result<(StatusCode, Json<AttributeDefinitionResponse>), ApiErr> {
    let allowed_values_ref: Option<Vec<String>> = body.allowed_values.clone();
    let av_slice = allowed_values_ref.as_deref();

    validate_attribute_definition(
        &body.key,
        &body.entity_type,
        &body.value_type,
        body.default_value.as_deref(),
        av_slice,
    )
    .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    let now = Utc::now().naive_utc();
    let id = Uuid::now_v7();

    let allowed_values_json = body
        .allowed_values
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(ApiErr::internal)?;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let model = attribute_definition::ActiveModel {
        id: Set(id),
        key: Set(body.key.clone()),
        entity_type: Set(body.entity_type.clone()),
        display_name: Set(body.display_name.clone()),
        value_type: Set(body.value_type.clone()),
        default_value: Set(body.default_value.clone()),
        allowed_values: Set(allowed_values_json),
        description: Set(body.description.clone()),
        created_by: Set(claims.sub),
        updated_by: Set(claims.sub),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&*txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict(format!(
                "Attribute definition '{}' already exists for entity type '{}'",
                body.key, body.entity_type
            ))
        } else {
            ApiErr::internal(e)
        }
    })?;

    txn.audit(
        "attribute_definition",
        id,
        AuditAction::Create,
        claims.sub,
        serde_json::json!({
            "after": {
                "key": model.key,
                "entity_type": model.entity_type,
                "value_type": model.value_type,
                "display_name": model.display_name,
                "default_value": model.default_value,
                "allowed_values": model.allowed_values,
                "description": model.description,
            }
        }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

    Ok((StatusCode::CREATED, Json(model.into())))
}

// ---------- GET /attribute-definitions/{id} ----------

pub async fn get_attribute_definition(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AttributeDefinitionResponse>, ApiErr> {
    let def = attribute_definition::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Attribute definition not found"))?;

    Ok(Json(def.into()))
}

// ---------- PUT /attribute-definitions/{id} ----------

pub async fn update_attribute_definition(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAttributeDefinitionRequest>,
) -> Result<Json<AttributeDefinitionResponse>, ApiErr> {
    let def = attribute_definition::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Attribute definition not found"))?;

    // Resolve final values for validation (key and entity_type are immutable)
    let final_value_type = body.value_type.as_deref().unwrap_or(&def.value_type);
    let final_default = match &body.default_value {
        Some(dv) => dv.as_deref(),
        None => def.default_value.as_deref(),
    };
    let existing_av: Option<Vec<String>> = def
        .allowed_values
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let final_av: Option<&[String]> = match &body.allowed_values {
        Some(av) => av.as_deref(),
        None => existing_av.as_deref(),
    };

    validate_attribute_definition(
        &def.key,
        &def.entity_type,
        final_value_type,
        final_default,
        final_av,
    )
    .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    let original_value_type = def.value_type.clone();
    let original_default_value = def.default_value.clone();
    let key = def.key.clone();
    let entity_type = def.entity_type.clone();

    let now = Utc::now().naive_utc();

    let mut changes_before = serde_json::Map::new();
    let mut changes_after = serde_json::Map::new();

    let mut active: attribute_definition::ActiveModel = def.clone().into();

    if let Some(ref dn) = body.display_name {
        changes_before.insert("display_name".into(), serde_json::json!(def.display_name));
        changes_after.insert("display_name".into(), serde_json::json!(dn));
        active.display_name = Set(dn.clone());
    }
    if let Some(ref vt) = body.value_type {
        changes_before.insert("value_type".into(), serde_json::json!(def.value_type));
        changes_after.insert("value_type".into(), serde_json::json!(vt));
        active.value_type = Set(vt.clone());
    }
    if let Some(ref dv) = body.default_value {
        changes_before.insert("default_value".into(), serde_json::json!(def.default_value));
        changes_after.insert("default_value".into(), serde_json::json!(dv));
        active.default_value = Set(dv.clone());
    }
    if let Some(ref av) = body.allowed_values {
        changes_before.insert(
            "allowed_values".into(),
            serde_json::json!(def.allowed_values),
        );
        changes_after.insert("allowed_values".into(), serde_json::json!(av));
        let json = av
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(ApiErr::internal)?;
        active.allowed_values = Set(json);
    }
    if let Some(ref desc) = body.description {
        changes_before.insert("description".into(), serde_json::json!(def.description));
        changes_after.insert("description".into(), serde_json::json!(desc));
        active.description = Set(desc.clone());
    }
    active.updated_by = Set(claims.sub);
    active.updated_at = Set(now);

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let updated = active.update(&*txn).await.map_err(ApiErr::internal)?;

    txn.audit(
        "attribute_definition",
        id,
        AuditAction::Update,
        claims.sub,
        serde_json::json!({ "before": changes_before, "after": changes_after }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

    // Cache invalidation: if value_type or default_value changed, stale data needs flushing.
    // default_value now affects query-time behavior (used as fallback for missing attributes).
    let value_type_changed =
        body.value_type.is_some() && body.value_type.as_deref() != Some(&original_value_type);
    let default_value_changed = body.default_value.is_some()
        && match &body.default_value {
            Some(dv) => *dv != original_default_value,
            None => false,
        };
    if (value_type_changed || default_value_changed) && entity_type == "user" {
        invalidate_users_with_attribute(&state, &key).await;
    }

    Ok(Json(updated.into()))
}

// ---------- DELETE /attribute-definitions/{id} ----------

pub async fn delete_attribute_definition(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Query(params): Query<DeleteAttributeDefinitionQuery>,
) -> Result<StatusCode, ApiErr> {
    let def = attribute_definition::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Attribute definition not found"))?;

    // Count affected entities
    let affected_count = if def.entity_type == "user" {
        count_users_with_attribute(&state.db, &def.key).await?
    } else {
        0 // Other entity types not wired up yet
    };

    if affected_count > 0 && !params.force {
        return Err(ApiErr::conflict(format!(
            "{} {}(s) have attribute '{}'. Use ?force=true to delete and remove from all entities.",
            affected_count, def.entity_type, def.key
        )));
    }

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    // Cascade: remove the key from all affected entities' JSON
    if affected_count > 0 && def.entity_type == "user" {
        validate_json_path_key(&def.key)?;

        let backend = txn.get_database_backend();
        let sql = match backend {
            sea_orm::DatabaseBackend::Sqlite => format!(
                "UPDATE proxy_user SET attributes = json_remove(attributes, '$.{}') WHERE json_extract(attributes, '$.{}') IS NOT NULL",
                def.key, def.key
            ),
            sea_orm::DatabaseBackend::Postgres => format!(
                "UPDATE proxy_user SET attributes = (attributes::jsonb - '{}')::text WHERE attributes::jsonb ? '{}'",
                def.key, def.key
            ),
            _ => {
                return Err(ApiErr::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Unsupported database backend",
                ));
            }
        };
        txn.execute(Statement::from_string(backend, sql))
            .await
            .map_err(ApiErr::internal)?;
    }

    txn.audit(
        "attribute_definition",
        id,
        AuditAction::Delete,
        claims.sub,
        serde_json::json!({
            "before": {
                "key": def.key,
                "entity_type": def.entity_type,
                "value_type": def.value_type,
                "display_name": def.display_name,
                "default_value": def.default_value,
                "allowed_values": def.allowed_values,
                "description": def.description,
                "affected_count": affected_count,
            }
        }),
    );

    let active: attribute_definition::ActiveModel = def.clone().into();
    active.delete(&*txn).await.map_err(ApiErr::internal)?;

    txn.commit().await.map_err(ApiErr::internal)?;

    // Cache invalidation: if we removed attributes from users, invalidate their sessions
    if affected_count > 0 && def.entity_type == "user" {
        invalidate_users_with_attribute(&state, &def.key).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------- helpers ----------

/// Defense-in-depth: re-validate that a key is safe for JSON path interpolation
/// before using it in format!() SQL. The key is already validated at creation time
/// by `validate_attribute_definition`, but we re-check before any SQL interpolation.
fn validate_json_path_key(key: &str) -> Result<(), ApiErr> {
    let valid = !key.is_empty()
        && key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(ApiErr::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Invalid attribute key for JSON path: '{key}'"),
        ));
    }
    Ok(())
}

async fn count_users_with_attribute(db: &impl ConnectionTrait, key: &str) -> Result<u64, ApiErr> {
    validate_json_path_key(key)?;

    let backend = db.get_database_backend();
    let sql = match backend {
        sea_orm::DatabaseBackend::Sqlite => format!(
            "SELECT COUNT(*) as cnt FROM proxy_user WHERE json_extract(attributes, '$.{}') IS NOT NULL",
            key
        ),
        sea_orm::DatabaseBackend::Postgres => format!(
            "SELECT COUNT(*) as cnt FROM proxy_user WHERE attributes::jsonb ? '{}'",
            key
        ),
        _ => {
            return Err(ApiErr::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unsupported database backend",
            ));
        }
    };
    let result = db
        .query_one(Statement::from_string(backend, sql))
        .await
        .map_err(ApiErr::internal)?;

    match result {
        Some(row) => {
            let cnt: i64 = row.try_get_by_index(0).map_err(ApiErr::internal)?;
            Ok(cnt as u64)
        }
        None => Ok(0),
    }
}

async fn invalidate_users_with_attribute(state: &AdminState, _key: &str) {
    // After json_remove we can't efficiently query who was affected, so invalidate all users.
    let users = match proxy_user::Entity::find().all(&state.db).await {
        Ok(u) => u,
        Err(_) => return,
    };

    for user in users {
        if let Some(hook) = &state.policy_hook {
            hook.invalidate_user(user.id).await;
        }
        if let Some(ph) = &state.proxy_handler {
            ph.rebuild_contexts_for_user(user.id);
        }
    }
}
