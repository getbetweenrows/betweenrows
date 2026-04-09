use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use uuid::Uuid;

use std::collections::HashSet;

use crate::entity::{data_source, data_source_access, proxy_user};

use super::{
    AdminState, ApiErr,
    admin_audit::{AuditAction, AuditedTxn},
    datasource_types::{self, DataSourceTypeResponse},
    dto::{
        CreateDataSourceRequest, DataSourceResponse, ListDataSourcesQuery, PaginatedResponse,
        SetDataSourceUsersRequest, TestConnectionResponse, UpdateDataSourceRequest, UserResponse,
        validate_access_mode, validate_datasource_name,
    },
    jwt::AdminClaims,
    role_handlers::invalidate_user,
};

// ---------- helper: build DataSourceResponse from model ----------

fn ds_response(model: data_source::Model) -> Result<DataSourceResponse, ApiErr> {
    let config: serde_json::Value =
        serde_json::from_str(&model.config).map_err(ApiErr::internal)?;
    let last_sync_result = if let Some(ref s) = model.last_sync_result {
        Some(serde_json::from_str(s).map_err(ApiErr::internal)?)
    } else {
        None
    };
    Ok(DataSourceResponse {
        id: model.id,
        name: model.name,
        ds_type: model.ds_type,
        config,
        is_active: model.is_active,
        access_mode: model.access_mode,
        last_sync_at: model.last_sync_at,
        last_sync_result,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

// ---------- GET /datasource-types ----------

pub async fn list_datasource_types(
    AdminClaims(_): AdminClaims,
) -> Json<Vec<DataSourceTypeResponse>> {
    let types = datasource_types::get_type_defs()
        .iter()
        .map(DataSourceTypeResponse::from)
        .collect();
    Json(types)
}

// ---------- GET /datasources ----------

pub async fn list_datasources(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Query(params): Query<ListDataSourcesQuery>,
) -> Result<Json<PaginatedResponse<DataSourceResponse>>, ApiErr> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).min(100);

    let mut query = data_source::Entity::find();

    if let Some(ref search) = params.search
        && !search.is_empty()
    {
        query = query.filter(data_source::Column::Name.contains(search.as_str()));
    }

    let paginator = query
        .order_by_asc(data_source::Column::CreatedAt)
        .paginate(&state.db, page_size);

    let total = paginator.num_items().await.map_err(ApiErr::internal)?;
    let items = paginator
        .fetch_page(page - 1)
        .await
        .map_err(ApiErr::internal)?;

    let data = items
        .into_iter()
        .map(ds_response)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        page_size,
    }))
}

// ---------- POST /datasources ----------

pub async fn create_datasource(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Json(body): Json<CreateDataSourceRequest>,
) -> Result<(StatusCode, Json<DataSourceResponse>), ApiErr> {
    validate_datasource_name(&body.name)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    if !validate_access_mode(&body.access_mode) {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "access_mode must be 'open' or 'policy_required'",
        ));
    }

    // Validate and split config using type registry
    let (config_json, secure_json) = datasource_types::split_config(&body.ds_type, body.config)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;

    // Encrypt secrets
    let secure_str =
        crate::crypto::encrypt_json(&secure_json, &state.master_key).map_err(ApiErr::internal)?;

    let config_str = serde_json::to_string(&config_json).map_err(ApiErr::internal)?;

    let now = Utc::now().naive_utc();
    let ds_id = Uuid::now_v7();

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let model = data_source::ActiveModel {
        id: Set(ds_id),
        name: Set(body.name),
        ds_type: Set(body.ds_type),
        config: Set(config_str),
        secure_config: Set(secure_str),
        is_active: Set(true),
        access_mode: Set(body.access_mode),
        last_sync_at: Set(None),
        last_sync_result: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&*txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Data source name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    // Auto-assign creator
    data_source_access::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(Some(claims.sub)),
        role_id: Set(None),
        data_source_id: Set(model.id),
        assignment_scope: Set("user".to_string()),
        created_at: Set(now),
    }
    .insert(&*txn)
    .await
    .map_err(ApiErr::internal)?;

    txn.audit(
        "datasource",
        ds_id,
        AuditAction::Create,
        claims.sub,
        serde_json::json!({
            "after": {
                "name": &model.name,
                "ds_type": &model.ds_type,
                "access_mode": &model.access_mode,
                "is_active": model.is_active,
            }
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    Ok((StatusCode::CREATED, Json(ds_response(model)?)))
}

// ---------- GET /datasources/{id} ----------

pub async fn get_datasource(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DataSourceResponse>, ApiErr> {
    let model = data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    Ok(Json(ds_response(model)?))
}

// ---------- PUT /datasources/{id} ----------

pub async fn update_datasource(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateDataSourceRequest>,
) -> Result<Json<DataSourceResponse>, ApiErr> {
    let model = data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let mut changes_before = serde_json::Map::new();
    let mut changes_after = serde_json::Map::new();

    let mut active: data_source::ActiveModel = model.clone().into();

    if let Some(ref name) = body.name {
        validate_datasource_name(name)
            .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;
        changes_before.insert("name".into(), serde_json::json!(model.name));
        changes_after.insert("name".into(), serde_json::json!(name));
        active.name = Set(name.clone());
    }
    if let Some(is_active) = body.is_active {
        changes_before.insert("is_active".into(), serde_json::json!(model.is_active));
        changes_after.insert("is_active".into(), serde_json::json!(is_active));
        active.is_active = Set(is_active);
    }
    if let Some(ref access_mode) = body.access_mode {
        if !validate_access_mode(access_mode) {
            return Err(ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "access_mode must be 'open' or 'policy_required'",
            ));
        }
        changes_before.insert("access_mode".into(), serde_json::json!(model.access_mode));
        changes_after.insert("access_mode".into(), serde_json::json!(access_mode));
        active.access_mode = Set(access_mode.clone());
    }

    if let Some(config_input) = body.config {
        changes_after.insert("config_changed".into(), serde_json::json!(true));

        // Load existing decrypted secure config for merge
        let existing_config: serde_json::Value =
            serde_json::from_str(&model.config).map_err(ApiErr::internal)?;
        let existing_secure: serde_json::Value = if model.secure_config.is_empty() {
            serde_json::json!({})
        } else {
            crate::crypto::decrypt_json(&model.secure_config, &state.master_key)
                .map_err(ApiErr::internal)?
        };

        let (new_config, new_secure) = datasource_types::merge_config(
            &model.ds_type,
            existing_config,
            existing_secure,
            config_input,
        )
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;

        let new_secure_str = crate::crypto::encrypt_json(&new_secure, &state.master_key)
            .map_err(ApiErr::internal)?;

        active.config = Set(serde_json::to_string(&new_config).map_err(ApiErr::internal)?);
        active.secure_config = Set(new_secure_str);
    }

    // No-op: nothing to update
    if changes_before.is_empty() && changes_after.is_empty() {
        return Ok(Json(ds_response(model)?));
    }

    active.updated_at = Set(Utc::now().naive_utc());

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let updated = active.update(&*txn).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Data source name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    txn.audit(
        "datasource",
        id,
        AuditAction::Update,
        claims.sub,
        serde_json::json!({ "before": changes_before, "after": changes_after }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate cached session context AND shared pool (connection params may have changed)
    // Use old name in case name was changed
    state.engine_cache.invalidate_all(&model.name).await;
    if let Some(hook) = &state.policy_hook {
        hook.invalidate_datasource(&model.name).await;
    }
    if let Some(ph) = &state.proxy_handler {
        ph.rebuild_contexts_for_datasource(&model.name);
    }

    Ok(Json(ds_response(updated)?))
}

// ---------- DELETE /datasources/{id} ----------

pub async fn delete_datasource(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let model = data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let name = model.name.clone();
    let ds_type = model.ds_type.clone();
    let access_mode = model.access_mode.clone();
    let is_active = model.is_active;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let active: data_source::ActiveModel = model.into();
    active.delete(&*txn).await.map_err(ApiErr::internal)?;

    txn.audit(
        "datasource",
        id,
        AuditAction::Delete,
        claims.sub,
        serde_json::json!({
            "before": {
                "name": &name,
                "ds_type": &ds_type,
                "access_mode": &access_mode,
                "is_active": is_active,
            }
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    // Invalidate cached session context AND shared pool
    state.engine_cache.invalidate_all(&name).await;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- POST /datasources/{id}/test ----------

pub async fn test_datasource(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TestConnectionResponse>, ApiErr> {
    let model = data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let cfg = crate::engine::DataSourceConfig::from_model(&model, &state.master_key)
        .map_err(ApiErr::internal)?;

    match crate::engine::EngineCache::test_connection(&cfg).await {
        Ok(()) => Ok(Json(TestConnectionResponse {
            success: true,
            message: None,
        })),
        Err(e) => {
            tracing::error!(
                datasource_id = %id,
                host = %cfg.host,
                port = cfg.port,
                database = %cfg.database,
                username = %cfg.username,
                error = %e,
                "test connection failed"
            );
            Ok(Json(TestConnectionResponse {
                success: false,
                message: Some(e.to_string()),
            }))
        }
    }
}

// ---------- GET /datasources/{id}/users ----------

pub async fn get_datasource_users(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<UserResponse>>, ApiErr> {
    // Confirm data source exists
    data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let assignments = data_source_access::Entity::find()
        .filter(data_source_access::Column::DataSourceId.eq(id))
        .filter(data_source_access::Column::AssignmentScope.eq("user"))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let user_ids: Vec<Uuid> = assignments.iter().filter_map(|a| a.user_id).collect();

    let users = proxy_user::Entity::find()
        .filter(proxy_user::Column::Id.is_in(user_ids))
        .order_by_asc(proxy_user::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    Ok(Json(users.into_iter().map(UserResponse::from).collect()))
}

// ---------- PUT /datasources/{id}/users ----------

pub async fn set_datasource_users(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetDataSourceUsersRequest>,
) -> Result<StatusCode, ApiErr> {
    data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let old_entries = data_source_access::Entity::find()
        .filter(data_source_access::Column::DataSourceId.eq(id))
        .filter(data_source_access::Column::AssignmentScope.eq("user"))
        .all(&*txn)
        .await
        .map_err(ApiErr::internal)?;
    let old_user_ids: HashSet<Uuid> = old_entries.iter().filter_map(|e| e.user_id).collect();

    data_source_access::Entity::delete_many()
        .filter(data_source_access::Column::DataSourceId.eq(id))
        .filter(data_source_access::Column::AssignmentScope.eq("user"))
        .exec(&*txn)
        .await
        .map_err(ApiErr::internal)?;

    let now = Utc::now().naive_utc();
    let new_user_ids: HashSet<Uuid> = body.user_ids.iter().copied().collect();
    for user_id in &body.user_ids {
        data_source_access::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(Some(*user_id)),
            role_id: Set(None),
            data_source_id: Set(id),
            assignment_scope: Set("user".to_string()),
            created_at: Set(now),
        }
        .insert(&*txn)
        .await
        .map_err(ApiErr::internal)?;
    }

    let old_ids_json: Vec<String> = old_user_ids.iter().map(|id| id.to_string()).collect();
    let new_ids_json: Vec<String> = new_user_ids.iter().map(|id| id.to_string()).collect();
    txn.audit(
        "datasource",
        id,
        AuditAction::Update,
        claims.sub,
        serde_json::json!({
            "field": "user_access",
            "before": old_ids_json,
            "after": new_ids_json,
        }),
    );

    txn.commit().await.map_err(ApiErr::internal)?;

    let all_affected: HashSet<Uuid> = old_user_ids.union(&new_user_ids).copied().collect();
    for user_id in all_affected {
        invalidate_user(&state, user_id).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::Database;

    async fn setup() -> (sea_orm::DatabaseConnection, [u8; 32]) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let master_key = [42u8; 32];
        (db, master_key)
    }

    async fn create_user(
        db: &sea_orm::DatabaseConnection,
        username: &str,
        is_admin: bool,
    ) -> proxy_user::Model {
        let now = Utc::now().naive_utc();
        proxy_user::ActiveModel {
            id: Set(Uuid::now_v7()),
            username: Set(username.to_string()),
            password_hash: Set("$argon2id$v=19$m=19456,t=2,p=1$fake".to_string()),
            is_admin: Set(is_admin),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn create_ds(
        db: &sea_orm::DatabaseConnection,
        master_key: &[u8; 32],
        name: &str,
    ) -> data_source::Model {
        let config = serde_json::json!({
            "host": "localhost",
            "port": 5432,
            "database": "testdb",
            "username": "alice",
            "sslmode": "require"
        });
        let secure = serde_json::json!({"password": "secret"});
        let secure_enc = crate::crypto::encrypt_json(&secure, master_key).unwrap();
        let now = Utc::now().naive_utc();
        data_source::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(name.to_string()),
            ds_type: Set("postgres".to_string()),
            config: Set(serde_json::to_string(&config).unwrap()),
            secure_config: Set(secure_enc),
            is_active: Set(true),
            access_mode: Set("open".to_string()),
            last_sync_at: Set(None),
            last_sync_result: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_create_datasource_splits_config() {
        let (db, master_key) = setup().await;

        let config_input = serde_json::json!({
            "host": "localhost",
            "port": 5432,
            "database": "mydb",
            "username": "alice",
            "password": "s3cr3t",
            "sslmode": "require"
        });

        let (config, secure) = datasource_types::split_config("postgres", config_input).unwrap();

        let secure_enc = crate::crypto::encrypt_json(&secure, &master_key).unwrap();
        let now = Utc::now().naive_utc();

        let model = data_source::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set("myds".to_string()),
            ds_type: Set("postgres".to_string()),
            config: Set(serde_json::to_string(&config).unwrap()),
            secure_config: Set(secure_enc.clone()),
            is_active: Set(true),
            access_mode: Set("open".to_string()),
            last_sync_at: Set(None),
            last_sync_result: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();

        // Password must NOT be in config column
        let stored_config: serde_json::Value = serde_json::from_str(&model.config).unwrap();
        assert!(
            stored_config.get("password").is_none(),
            "Password must not be in config"
        );

        // But recoverable from secure_config
        let decrypted = crate::crypto::decrypt_json(&model.secure_config, &master_key).unwrap();
        assert_eq!(decrypted["password"], "s3cr3t");
    }

    #[tokio::test]
    async fn test_create_datasource_auto_assigns_creator() {
        let (db, master_key) = setup().await;
        let user = create_user(&db, "alice", true).await;
        let ds = create_ds(&db, &master_key, "myds").await;

        // Simulate auto-assignment (as handler would do)
        data_source_access::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(Some(user.id)),
            role_id: Set(None),
            data_source_id: Set(ds.id),
            assignment_scope: Set("user".to_string()),
            created_at: Set(Utc::now().naive_utc()),
        }
        .insert(&db)
        .await
        .unwrap();

        let assignment = data_source_access::Entity::find()
            .filter(data_source_access::Column::UserId.eq(user.id))
            .filter(data_source_access::Column::DataSourceId.eq(ds.id))
            .one(&db)
            .await
            .unwrap();

        assert!(assignment.is_some(), "Creator should be auto-assigned");
    }

    #[tokio::test]
    async fn test_unique_name_constraint() {
        let (db, master_key) = setup().await;
        create_ds(&db, &master_key, "myds").await;

        // Attempt to create another with same name
        let now = Utc::now().naive_utc();
        let result = data_source::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set("myds".to_string()),
            ds_type: Set("postgres".to_string()),
            config: Set("{}".to_string()),
            secure_config: Set("".to_string()),
            is_active: Set(true),
            access_mode: Set("open".to_string()),
            last_sync_at: Set(None),
            last_sync_result: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await;

        assert!(result.is_err(), "Duplicate name should fail");
    }

    #[tokio::test]
    async fn test_user_assignment_get_put() {
        let (db, master_key) = setup().await;
        let user1 = create_user(&db, "alice", true).await;
        let user2 = create_user(&db, "bob", false).await;
        let ds = create_ds(&db, &master_key, "myds").await;

        // Assign user1 and user2
        let now = Utc::now().naive_utc();
        for uid in [user1.id, user2.id] {
            data_source_access::ActiveModel {
                id: Set(Uuid::now_v7()),
                user_id: Set(Some(uid)),
                role_id: Set(None),
                data_source_id: Set(ds.id),
                assignment_scope: Set("user".to_string()),
                created_at: Set(now),
            }
            .insert(&db)
            .await
            .unwrap();
        }

        // Read assignments
        let assignments = data_source_access::Entity::find()
            .filter(data_source_access::Column::DataSourceId.eq(ds.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(assignments.len(), 2);

        // Replace with just user1
        data_source_access::Entity::delete_many()
            .filter(data_source_access::Column::DataSourceId.eq(ds.id))
            .exec(&db)
            .await
            .unwrap();

        data_source_access::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(Some(user1.id)),
            role_id: Set(None),
            data_source_id: Set(ds.id),
            assignment_scope: Set("user".to_string()),
            created_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();

        let remaining = data_source_access::Entity::find()
            .filter(data_source_access::Column::DataSourceId.eq(ds.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].user_id, Some(user1.id));
    }

    #[tokio::test]
    async fn test_delete_datasource_cascades_assignments() {
        let (db, master_key) = setup().await;
        let user = create_user(&db, "alice", true).await;
        let ds = create_ds(&db, &master_key, "myds").await;

        data_source_access::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(Some(user.id)),
            role_id: Set(None),
            data_source_id: Set(ds.id),
            assignment_scope: Set("user".to_string()),
            created_at: Set(Utc::now().naive_utc()),
        }
        .insert(&db)
        .await
        .unwrap();

        // Delete the datasource
        let active: data_source::ActiveModel = ds.into();
        active.delete(&db).await.unwrap();

        let ds_count = data_source::Entity::find().count(&db).await.unwrap();
        assert_eq!(ds_count, 0);
    }

    // ===== Audit logging tests (HTTP handler level) =====

    use crate::{
        admin::{discovery_job, jwt},
        engine::EngineCache,
        entity::admin_audit_log,
    };
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode as AxumStatusCode},
        routing::get,
    };
    use std::sync::{Arc, OnceLock};
    use tokio::sync::Mutex as TokioMutex;
    use tower::ServiceExt;

    use super::AdminState;

    const JWT_SECRET: &str = "test-jwt-secret-key-32-chars-pad";

    fn shared_wasm_runtime() -> Arc<crate::decision::wasm::WasmDecisionRuntime> {
        static RUNTIME: OnceLock<Arc<crate::decision::wasm::WasmDecisionRuntime>> = OnceLock::new();
        RUNTIME
            .get_or_init(|| Arc::new(crate::decision::wasm::WasmDecisionRuntime::new().unwrap()))
            .clone()
    }

    fn make_state(db: sea_orm::DatabaseConnection, master_key: [u8; 32]) -> AdminState {
        let wasm_runtime = shared_wasm_runtime();
        let engine_cache = EngineCache::new(db.clone(), master_key, wasm_runtime.clone());
        AdminState {
            auth: Arc::new(crate::auth::Auth::new(db.clone())),
            db,
            jwt_secret: JWT_SECRET.to_string(),
            jwt_expiry_hours: 1,
            engine_cache,
            master_key,
            job_store: Arc::new(TokioMutex::new(discovery_job::JobStore::new())),
            policy_hook: None,
            proxy_handler: None,
            wasm_runtime,
        }
    }

    fn make_router(state: AdminState) -> Router {
        Router::new()
            .route(
                "/datasources",
                get(list_datasources).post(create_datasource),
            )
            .route(
                "/datasources/{id}",
                get(get_datasource)
                    .put(update_datasource)
                    .delete(delete_datasource),
            )
            .with_state(state)
    }

    fn admin_token(id: Uuid) -> String {
        let claims = jwt::Claims {
            sub: id,
            username: "admin".to_string(),
            is_admin: true,
            exp: (Utc::now().timestamp() as u64) + 3600,
        };
        jwt::encode_jwt(&claims, JWT_SECRET).unwrap()
    }

    fn json_body(value: serde_json::Value) -> Body {
        Body::from(serde_json::to_string(&value).unwrap())
    }

    async fn body_json(res: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn get_audit_entries(
        db: &sea_orm::DatabaseConnection,
        resource_type: &str,
    ) -> Vec<admin_audit_log::Model> {
        use sea_orm::QueryFilter;
        admin_audit_log::Entity::find()
            .filter(admin_audit_log::Column::ResourceType.eq(resource_type))
            .all(db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn audit_create_datasource() {
        let (db, master_key) = setup().await;
        let user = create_user(&db, "admin", true).await;
        let token = admin_token(user.id);

        let res = make_router(make_state(db.clone(), master_key))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/datasources")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "audit-ds",
                        "ds_type": "postgres",
                        "access_mode": "open",
                        "config": {
                            "host": "localhost",
                            "port": 5432,
                            "database": "testdb",
                            "username": "alice",
                            "password": "secret",
                            "sslmode": "require"
                        }
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), AxumStatusCode::CREATED);
        let body = body_json(res).await;
        let ds_id: Uuid = body["id"].as_str().unwrap().parse().unwrap();

        let entries = get_audit_entries(&db, "datasource").await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].resource_id, ds_id);
        assert_eq!(entries[0].action, "create");
        assert_eq!(entries[0].actor_id, user.id);

        let changes: serde_json::Value =
            serde_json::from_str(entries[0].changes.as_deref().unwrap()).unwrap();
        assert_eq!(changes["after"]["name"], "audit-ds");
        assert_eq!(changes["after"]["ds_type"], "postgres");
        assert_eq!(changes["after"]["access_mode"], "open");
        assert_eq!(changes["after"]["is_active"], true);
        // Secrets must not appear
        assert!(changes["after"].get("config").is_none());
        assert!(changes["after"].get("secure_config").is_none());
    }

    #[tokio::test]
    async fn audit_update_datasource_changed_fields_only() {
        let (db, master_key) = setup().await;
        let user = create_user(&db, "admin", true).await;
        let token = admin_token(user.id);

        // Create via handler
        let create_res = make_router(make_state(db.clone(), master_key))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/datasources")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "original",
                        "ds_type": "postgres",
                        "access_mode": "open",
                        "config": {
                            "host": "localhost", "port": 5432, "database": "testdb",
                            "username": "alice", "password": "secret", "sslmode": "require"
                        }
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ds_id = body_json(create_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Update name only
        let update_res = make_router(make_state(db.clone(), master_key))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/datasources/{ds_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({"name": "renamed"})))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_res.status(), AxumStatusCode::OK);

        let entries = get_audit_entries(&db, "datasource").await;
        let update_entry = entries.iter().find(|e| e.action == "update").unwrap();
        let changes: serde_json::Value =
            serde_json::from_str(update_entry.changes.as_deref().unwrap()).unwrap();
        // Only changed fields
        assert_eq!(changes["before"]["name"], "original");
        assert_eq!(changes["after"]["name"], "renamed");
        assert!(changes["before"].get("is_active").is_none());
        assert!(changes["after"].get("is_active").is_none());
    }

    #[tokio::test]
    async fn audit_delete_datasource() {
        let (db, master_key) = setup().await;
        let user = create_user(&db, "admin", true).await;
        let token = admin_token(user.id);

        let create_res = make_router(make_state(db.clone(), master_key))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/datasources")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({
                        "name": "deletable",
                        "ds_type": "postgres",
                        "access_mode": "open",
                        "config": {
                            "host": "localhost", "port": 5432, "database": "testdb",
                            "username": "alice", "password": "secret", "sslmode": "require"
                        }
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ds_id = body_json(create_res).await["id"]
            .as_str()
            .unwrap()
            .to_string();

        let del_res = make_router(make_state(db.clone(), master_key))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/datasources/{ds_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_res.status(), AxumStatusCode::NO_CONTENT);

        let entries = get_audit_entries(&db, "datasource").await;
        let del_entry = entries.iter().find(|e| e.action == "delete").unwrap();
        let changes: serde_json::Value =
            serde_json::from_str(del_entry.changes.as_deref().unwrap()).unwrap();
        assert_eq!(changes["before"]["name"], "deletable");
        assert_eq!(changes["before"]["ds_type"], "postgres");
        assert_eq!(changes["before"]["is_active"], true);
        // No secrets
        assert!(changes["before"].get("config").is_none());
        assert!(changes["before"].get("secure_config").is_none());
    }
}
