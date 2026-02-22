use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post, put},
    Router,
};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

use crate::auth::Auth;
use crate::engine::EngineCache;

pub mod auth_handlers;
pub mod catalog_handlers;
pub mod datasource_handlers;
pub mod datasource_types;
pub mod discovery_job;
pub mod dto;
pub mod jwt;
pub mod user_handlers;

// ---------- shared state ----------

#[derive(Clone)]
pub struct AdminState {
    pub auth: Arc<Auth>,
    pub db: DatabaseConnection,
    pub jwt_secret: String,
    pub jwt_expiry_hours: u64,
    pub engine_cache: Arc<EngineCache>,
    pub master_key: [u8; 32],
    /// In-memory job registry for async discovery operations.
    pub job_store: Arc<Mutex<discovery_job::JobStore>>,
}

// ---------- error type ----------

/// A JSON error response: `{"error": "..."}` with an HTTP status.
pub struct ApiErr(StatusCode, String);

impl ApiErr {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self(status, msg.into())
    }

    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, msg.into())
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.1 });
        (self.0, Json(body)).into_response()
    }
}

// ---------- router ----------

pub fn admin_router(state: AdminState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .nest("/api/v1", api_v1())
        .layer(cors)
        .with_state(state)
}

fn api_v1() -> Router<AdminState> {
    Router::new()
        // auth
        .route("/auth/login", post(auth_handlers::login))
        .route("/auth/me", get(auth_handlers::me))
        // users
        .route(
            "/users",
            get(user_handlers::list_users).post(user_handlers::create_user),
        )
        .route(
            "/users/{id}",
            get(user_handlers::get_user)
                .put(user_handlers::update_user)
                .delete(user_handlers::delete_user),
        )
        .route("/users/{id}/password", put(user_handlers::change_password))
        // data source types
        .route(
            "/datasource-types",
            get(datasource_handlers::list_datasource_types),
        )
        // data sources
        .route(
            "/datasources",
            get(datasource_handlers::list_datasources).post(datasource_handlers::create_datasource),
        )
        .route(
            "/datasources/{id}",
            get(datasource_handlers::get_datasource)
                .put(datasource_handlers::update_datasource)
                .delete(datasource_handlers::delete_datasource),
        )
        .route(
            "/datasources/{id}/test",
            post(datasource_handlers::test_datasource),
        )
        .route(
            "/datasources/{id}/users",
            get(datasource_handlers::get_datasource_users)
                .put(datasource_handlers::set_datasource_users),
        )
        // async discovery jobs
        .route(
            "/datasources/{id}/discover",
            post(catalog_handlers::submit_discovery),
        )
        .route(
            "/datasources/{id}/discover/{job_id}",
            get(catalog_handlers::discovery_status)
                .delete(catalog_handlers::cancel_discovery),
        )
        .route(
            "/datasources/{id}/discover/{job_id}/events",
            get(catalog_handlers::discovery_events),
        )
        // catalog (fast local DB read)
        .route(
            "/datasources/{id}/catalog",
            get(catalog_handlers::get_catalog),
        )
}
