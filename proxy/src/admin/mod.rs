use axum::{
    Router,
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post, put},
};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::response::SetResponseHeaderLayer;

use crate::auth::Auth;
use crate::engine::EngineCache;
use crate::handler::ProxyHandler;
use crate::hooks::policy::PolicyHook;

pub mod admin_audit;
pub mod attribute_definition_handlers;
pub mod audit_handlers;
pub mod auth_handlers;
pub mod catalog_handlers;
pub mod datasource_handlers;
pub mod datasource_types;
pub mod decision_function_handlers;
pub mod discovery_job;
pub mod dto;
pub mod jwt;
pub mod policy_handlers;
pub mod role_handlers;
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
    /// PolicyHook reference for cache invalidation from admin API.
    pub policy_hook: Option<Arc<PolicyHook>>,
    /// ProxyHandler reference to rebuild per-connection SessionContexts after policy mutations.
    pub proxy_handler: Option<Arc<ProxyHandler>>,
    /// Shared WASM runtime for the admin test endpoint.
    pub wasm_runtime: Arc<crate::decision::wasm::WasmDecisionRuntime>,
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
        .route(
            "/health",
            get(|| async {
                Json(serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "commit": env!("GIT_COMMIT_SHORT"),
                }))
            }),
        )
        .nest("/api/v1", api_v1())
        .fallback_service(
            ServeDir::new("/usr/local/share/admin-ui")
                .fallback(ServeFile::new("/usr/local/share/admin-ui/index.html")),
        )
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
        // datasource policy assignments
        .route(
            "/datasources/{id}/policies",
            get(policy_handlers::list_datasource_policies).post(policy_handlers::assign_policy),
        )
        .route(
            "/datasources/{id}/policies/{assignment_id}",
            delete(policy_handlers::remove_assignment),
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
        // policies
        .route(
            "/policies/validate-expression",
            post(policy_handlers::validate_expression_handler),
        )
        .route(
            "/policies",
            get(policy_handlers::list_policies).post(policy_handlers::create_policy),
        )
        .route(
            "/policies/{id}",
            get(policy_handlers::get_policy)
                .put(policy_handlers::update_policy)
                .delete(policy_handlers::delete_policy),
        )
        // attribute definitions
        .route(
            "/attribute-definitions",
            get(attribute_definition_handlers::list_attribute_definitions)
                .post(attribute_definition_handlers::create_attribute_definition),
        )
        .route(
            "/attribute-definitions/{id}",
            get(attribute_definition_handlers::get_attribute_definition)
                .put(attribute_definition_handlers::update_attribute_definition)
                .delete(attribute_definition_handlers::delete_attribute_definition),
        )
        // decision functions
        .route(
            "/decision-functions",
            get(decision_function_handlers::list_decision_functions)
                .post(decision_function_handlers::create_decision_function),
        )
        .route(
            "/decision-functions/{id}",
            get(decision_function_handlers::get_decision_function)
                .put(decision_function_handlers::update_decision_function)
                .delete(decision_function_handlers::delete_decision_function),
        )
        .route(
            "/decision-functions/test",
            post(decision_function_handlers::test_decision_fn),
        )
        // roles
        .route(
            "/roles",
            get(role_handlers::list_roles).post(role_handlers::create_role),
        )
        .route(
            "/roles/{id}",
            get(role_handlers::get_role)
                .put(role_handlers::update_role)
                .delete(role_handlers::delete_role),
        )
        .route(
            "/roles/{id}/effective-members",
            get(role_handlers::get_effective_members),
        )
        .route("/roles/{id}/impact", get(role_handlers::get_role_impact))
        .route("/roles/{id}/members", post(role_handlers::add_members))
        .route(
            "/roles/{id}/members/{user_id}",
            delete(role_handlers::remove_member),
        )
        .route("/roles/{id}/parents", post(role_handlers::add_parent))
        .route(
            "/roles/{id}/parents/{parent_id}",
            delete(role_handlers::remove_parent),
        )
        // datasource role access
        .route(
            "/datasources/{id}/access/roles",
            get(role_handlers::get_datasource_role_access)
                .put(role_handlers::set_datasource_role_access),
        )
        // audit log
        .route("/audit/queries", get(audit_handlers::list_audit_logs))
        .route("/audit/admin", get(audit_handlers::list_admin_audit_logs))
        // effective policies
        .route(
            "/users/{id}/effective-policies",
            get(policy_handlers::get_effective_policies),
        )
}
