use axum::{
    Router,
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{get, post, put},
};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::response::SetResponseHeaderLayer;

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
    let allowed_origins: Vec<HeaderValue> = std::env::var("BR_CORS_ALLOWED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    let cors = if allowed_origins.is_empty() {
        CorsLayer::new() // no origins allowed = same-origin only
    } else {
        CorsLayer::new()
            .allow_origin(allowed_origins)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
            .allow_credentials(true)
    };

    Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .nest("/api/v1", api_v1())
        .fallback_service(ServeDir::new("/usr/local/share/admin-ui"))
        .layer(cors)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(NormalizePathLayer::trim_trailing_slash())
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
            get(catalog_handlers::discovery_status).delete(catalog_handlers::cancel_discovery),
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
