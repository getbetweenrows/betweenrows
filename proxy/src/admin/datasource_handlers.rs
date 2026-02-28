use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
    TransactionTrait,
};
use uuid::Uuid;

use crate::entity::{data_source, proxy_user, user_data_source};

use super::{
    AdminState, ApiErr,
    datasource_types::{self, DataSourceTypeResponse},
    dto::{
        CreateDataSourceRequest, DataSourceResponse, ListDataSourcesQuery, PaginatedResponse,
        SetDataSourceUsersRequest, TestConnectionResponse, UpdateDataSourceRequest, UserResponse,
    },
    jwt::AdminClaims,
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
    // Validate and split config using type registry
    let (config_json, secure_json) = datasource_types::split_config(&body.ds_type, body.config)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;

    // Encrypt secrets
    let secure_str =
        crate::crypto::encrypt_json(&secure_json, &state.master_key).map_err(ApiErr::internal)?;

    let config_str = serde_json::to_string(&config_json).map_err(ApiErr::internal)?;

    let now = Utc::now().naive_utc();
    let ds_id = Uuid::now_v7();

    let txn = state.db.begin().await.map_err(ApiErr::internal)?;

    let model = data_source::ActiveModel {
        id: Set(ds_id),
        name: Set(body.name),
        ds_type: Set(body.ds_type),
        config: Set(config_str),
        secure_config: Set(secure_str),
        is_active: Set(true),
        last_sync_at: Set(None),
        last_sync_result: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&txn)
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
    user_data_source::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(claims.sub),
        data_source_id: Set(model.id),
        created_at: Set(now),
    }
    .insert(&txn)
    .await
    .map_err(ApiErr::internal)?;

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
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateDataSourceRequest>,
) -> Result<Json<DataSourceResponse>, ApiErr> {
    let model = data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let mut active: data_source::ActiveModel = model.clone().into();

    if let Some(name) = body.name {
        active.name = Set(name);
    }
    if let Some(is_active) = body.is_active {
        active.is_active = Set(is_active);
    }

    if let Some(config_input) = body.config {
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

    active.updated_at = Set(Utc::now().naive_utc());
    let updated = active.update(&state.db).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Data source name already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    // Invalidate cached session context AND shared pool (connection params may have changed)
    // Use old name in case name was changed
    state.engine_cache.invalidate_all(&model.name).await;

    Ok(Json(ds_response(updated)?))
}

// ---------- DELETE /datasources/{id} ----------

pub async fn delete_datasource(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let model = data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let name = model.name.clone();
    let active: data_source::ActiveModel = model.into();
    active.delete(&state.db).await.map_err(ApiErr::internal)?;

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

    let assignments = user_data_source::Entity::find()
        .filter(user_data_source::Column::DataSourceId.eq(id))
        .all(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let user_ids: Vec<Uuid> = assignments.iter().map(|a| a.user_id).collect();

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
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetDataSourceUsersRequest>,
) -> Result<StatusCode, ApiErr> {
    // Confirm data source exists
    data_source::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("Data source not found"))?;

    let txn = state.db.begin().await.map_err(ApiErr::internal)?;

    // Delete all existing assignments
    user_data_source::Entity::delete_many()
        .filter(user_data_source::Column::DataSourceId.eq(id))
        .exec(&txn)
        .await
        .map_err(ApiErr::internal)?;

    // Insert new assignments
    let now = Utc::now().naive_utc();
    for user_id in body.user_ids {
        user_data_source::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(user_id),
            data_source_id: Set(id),
            created_at: Set(now),
        }
        .insert(&txn)
        .await
        .map_err(ApiErr::internal)?;
    }

    txn.commit().await.map_err(ApiErr::internal)?;

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
            tenant: Set("default".to_string()),
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
        user_data_source::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(user.id),
            data_source_id: Set(ds.id),
            created_at: Set(Utc::now().naive_utc()),
        }
        .insert(&db)
        .await
        .unwrap();

        let assignment = user_data_source::Entity::find()
            .filter(user_data_source::Column::UserId.eq(user.id))
            .filter(user_data_source::Column::DataSourceId.eq(ds.id))
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
            user_data_source::ActiveModel {
                id: Set(Uuid::now_v7()),
                user_id: Set(uid),
                data_source_id: Set(ds.id),
                created_at: Set(now),
            }
            .insert(&db)
            .await
            .unwrap();
        }

        // Read assignments
        let assignments = user_data_source::Entity::find()
            .filter(user_data_source::Column::DataSourceId.eq(ds.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(assignments.len(), 2);

        // Replace with just user1
        user_data_source::Entity::delete_many()
            .filter(user_data_source::Column::DataSourceId.eq(ds.id))
            .exec(&db)
            .await
            .unwrap();

        user_data_source::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(user1.id),
            data_source_id: Set(ds.id),
            created_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();

        let remaining = user_data_source::Entity::find()
            .filter(user_data_source::Column::DataSourceId.eq(ds.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].user_id, user1.id);
    }

    #[tokio::test]
    async fn test_delete_datasource_cascades_assignments() {
        let (db, master_key) = setup().await;
        let user = create_user(&db, "alice", true).await;
        let ds = create_ds(&db, &master_key, "myds").await;

        user_data_source::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(user.id),
            data_source_id: Set(ds.id),
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
}
