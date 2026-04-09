//! Shared test infrastructure for proxy integration tests.
//!
//! Provides a `ProxyTestServer` that spins up:
//! - A shared Postgres container (one per test binary, via background thread)
//! - An in-memory SQLite admin DB (per test)
//! - A real `ProxyHandler` on a random TCP port (per test)
//! - An `axum_test::TestServer` wrapping the admin API (per test)

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum_test::TestServer;
use sea_orm::Database;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_postgres::{NoTls, SimpleQueryMessage};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Shared test constants
// ---------------------------------------------------------------------------

/// Default password used for test users in protocol tests.
#[allow(dead_code)]
pub const TEST_PASS: &str = "TestPass1!";

// ---------------------------------------------------------------------------
// Shared query helpers
// ---------------------------------------------------------------------------

/// Extract data rows from a `simple_query` result, returning each row as a
/// `Vec<String>` (NULL values become the literal string `"NULL"`).
#[allow(dead_code)]
pub fn extract_rows(msgs: &[SimpleQueryMessage]) -> Vec<Vec<String>> {
    msgs.iter()
        .filter_map(|m| {
            if let SimpleQueryMessage::Row(r) = m {
                let ncols = r.len();
                let row: Vec<String> = (0..ncols)
                    .map(|i| r.get(i).unwrap_or("NULL").to_string())
                    .collect();
                Some(row)
            } else {
                None
            }
        })
        .collect()
}

use migration::MigratorTrait as _;
use proxy::admin::discovery_job::JobStore;
use proxy::admin::{AdminState, admin_router};
use proxy::auth::Auth;
use proxy::engine::EngineCache;
use proxy::handler::ProxyHandler;
use proxy::hooks::policy::PolicyHook;
use proxy::server::process_socket_with_idle_timeout;

// ---------------------------------------------------------------------------
// Shared WASM runtime (one per test binary)
// ---------------------------------------------------------------------------

fn shared_wasm_runtime() -> Arc<proxy::decision::wasm::WasmDecisionRuntime> {
    static RUNTIME: OnceLock<Arc<proxy::decision::wasm::WasmDecisionRuntime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| Arc::new(proxy::decision::wasm::WasmDecisionRuntime::new().unwrap()))
        .clone()
}

// ---------------------------------------------------------------------------
// Shared Postgres container (one per test binary)
// ---------------------------------------------------------------------------

pub struct SharedPostgres {
    pub url: String,
    pub host: String,
    pub port: u16,
    /// Keeps the container alive. Dropped on normal process exit → container stops.
    /// For abnormal exits (SIGKILL), the container label lets us find and clean orphans.
    _container: testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
}

static SHARED_PG: OnceLock<Option<SharedPostgres>> = OnceLock::new();

/// Returns a reference to the shared Postgres container, or `None` if Docker
/// is not available.  The container is started lazily on first call and kept
/// alive for the lifetime of the test process.
pub fn shared_postgres() -> Option<&'static SharedPostgres> {
    SHARED_PG
        .get_or_init(|| {
            // Use a background thread with its own tokio runtime so we never
            // call `block_on` inside an existing `#[tokio::test]` runtime.
            let (tx, rx) = std::sync::mpsc::sync_channel::<Option<SharedPostgres>>(1);
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(_) => {
                        let _ = tx.send(None);
                        return;
                    }
                };
                rt.block_on(async {
                    use testcontainers::core::ImageExt;
                    use testcontainers::runners::AsyncRunner;
                    use testcontainers_modules::postgres::Postgres;

                    let container = match Postgres::default()
                        .with_label("com.betweenrows.test", "true")
                        .start()
                        .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("testcontainers: could not start Postgres: {e}");
                            let _ = tx.send(None);
                            return;
                        }
                    };
                    let port = match container.get_host_port_ipv4(5432).await {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("testcontainers: could not get port: {e}");
                            let _ = tx.send(None);
                            return;
                        }
                    };
                    let _ = tx.send(Some(SharedPostgres {
                        url: format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
                        host: "127.0.0.1".to_string(),
                        port,
                        _container: container,
                    }));
                    // Keep the runtime alive so the container's async drop works on process exit.
                    std::future::pending::<()>().await;
                });
            });
            // Wait up to 60 s for the container to start.
            rx.recv_timeout(Duration::from_secs(60)).unwrap_or(None)
        })
        .as_ref()
}

/// Skip the current test if the shared Postgres container is not available.
#[macro_export]
macro_rules! require_postgres {
    () => {
        match support::shared_postgres() {
            Some(pg) => pg,
            None => {
                eprintln!("Skipping test: Docker / Postgres container not available");
                return;
            }
        }
    };
}

/// Returns true if `javy` CLI is on PATH.
#[allow(dead_code)]
pub fn javy_available() -> bool {
    std::process::Command::new("javy")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Skip the current test if javy CLI is not available.
#[macro_export]
macro_rules! require_javy {
    () => {
        if !support::javy_available() {
            eprintln!("Skipping test: javy CLI not available");
            return;
        }
    };
}

// ---------------------------------------------------------------------------
// ProxyTestServer — per-test infrastructure
// ---------------------------------------------------------------------------

pub struct ProxyTestServer {
    pub admin: TestServer,
    pub proxy_port: u16,
    pub admin_token: String,
    _accept_handle: JoinHandle<()>,
}

const JWT_SECRET: &str = "integration-test-jwt-secret-key!";
const MASTER_KEY: [u8; 32] = [0u8; 32];
const ADMIN_USER: &str = "admin";
const ADMIN_PASS: &str = "Admin1234!";

impl ProxyTestServer {
    /// Start a new, isolated test server.
    ///
    /// `_schema_prefix` is currently unused but reserved for future per-test
    /// Postgres schema isolation (all tests already use unique schema names).
    pub async fn start() -> Self {
        // 1. In-memory SQLite admin DB
        let db = Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();

        // 2. Create admin user
        let auth = Arc::new(Auth::new(db.clone()));
        auth.create_user(ADMIN_USER, ADMIN_PASS, true)
            .await
            .unwrap();

        // 3. WASM runtime + engine cache + policy hook
        let wasm_runtime = shared_wasm_runtime();
        let engine_cache = EngineCache::new(db.clone(), MASTER_KEY, wasm_runtime.clone());
        let policy_hook = PolicyHook::new(db.clone(), wasm_runtime.clone());

        // 4. ProxyHandler
        let handler = Arc::new(ProxyHandler::new(
            auth.clone(),
            engine_cache.clone(),
            policy_hook.clone(),
        ));

        // 5. AdminState
        let admin_state = AdminState {
            auth: auth.clone(),
            db: db.clone(),
            jwt_secret: JWT_SECRET.to_string(),
            jwt_expiry_hours: 1,
            engine_cache: engine_cache.clone(),
            master_key: MASTER_KEY,
            job_store: Arc::new(tokio::sync::Mutex::new(JobStore::new())),
            policy_hook: Some(policy_hook.clone()),
            proxy_handler: Some(handler.clone()),
            wasm_runtime,
        };

        // 6. axum-test TestServer for the admin API
        let router = admin_router(admin_state);
        let admin = TestServer::new(router);

        // 7. Bind random port for pgwire proxy
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = listener.local_addr().unwrap().port();

        // 8. Spawn accept loop (mirrors main.rs)
        let handler_for_loop = handler.clone();
        let accept_handle = tokio::spawn(async move {
            let idle_timeout = Duration::from_secs(300);
            loop {
                let (socket, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let peer_addr = socket.peer_addr().ok();
                let conn_id = peer_addr
                    .map(|addr| handler_for_loop.register_connection(addr))
                    .unwrap_or_else(|| handler_for_loop.alloc_connection_id());

                let h = handler_for_loop.clone();
                tokio::spawn(async move {
                    let _ = process_socket_with_idle_timeout(socket, h.clone(), idle_timeout).await;
                    h.cleanup_connection(conn_id, peer_addr);
                });
            }
        });

        // 9. Login to get admin JWT
        let login_resp: axum_test::TestResponse = admin
            .post("/api/v1/auth/login")
            .json(&json!({
                "username": ADMIN_USER,
                "password": ADMIN_PASS,
            }))
            .await;
        login_resp.assert_status_ok();
        let token = login_resp.json::<Value>()["token"]
            .as_str()
            .unwrap()
            .to_string();

        Self {
            admin,
            proxy_port,
            admin_token: token,
            _accept_handle: accept_handle,
        }
    }

    // -- Admin API helpers --

    /// Create a datasource pointing at the shared Postgres container.
    pub async fn create_datasource(&self, name: &str, access_mode: &str) -> Uuid {
        let pg = shared_postgres().expect("postgres required");
        let resp = self
            .admin
            .post("/api/v1/datasources")
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "name": name,
                "ds_type": "postgres",
                "config": {
                    "host": pg.host,
                    "port": pg.port,
                    "database": "postgres",
                    "username": "postgres",
                    "password": "postgres",
                    "sslmode": "disable"
                },
                "access_mode": access_mode
            }))
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        let id_str = resp.json::<Value>()["id"].as_str().unwrap().to_string();
        id_str.parse::<Uuid>().unwrap()
    }

    /// Run discovery (discover schemas → discover tables → discover columns → save catalog)
    /// against the shared Postgres container for the given schema names.
    pub async fn discover(&self, ds_id: Uuid, schema_names: &[&str]) {
        // Step 1: Discover schemas
        let schemas_data = self
            .run_discovery_job(ds_id, json!({"action": "discover_schemas"}))
            .await;
        let schemas_arr = schemas_data.as_array().unwrap();

        // Step 2: Discover tables for requested schemas
        let schemas_to_discover: Vec<String> = schemas_arr
            .iter()
            .filter_map(|s| {
                let name = s["schema_name"].as_str().unwrap();
                if schema_names.contains(&name) {
                    Some(name.to_string())
                } else {
                    None
                }
            })
            .collect();

        let tables_data = self
            .run_discovery_job(
                ds_id,
                json!({"action": "discover_tables", "schemas": schemas_to_discover}),
            )
            .await;
        let tables_arr = tables_data.as_array().unwrap();

        // Step 3: Discover columns
        let table_refs: Vec<Value> = tables_arr
            .iter()
            .map(|t| {
                json!({
                    "schema": t["schema_name"].as_str().unwrap(),
                    "table": t["table_name"].as_str().unwrap(),
                })
            })
            .collect();

        let _columns_data = self
            .run_discovery_job(
                ds_id,
                json!({"action": "discover_columns", "tables": table_refs}),
            )
            .await;

        // Step 4: Save catalog — select all discovered schemas/tables/columns
        let save_schemas: Vec<Value> = tables_arr
            .iter()
            .fold(
                std::collections::HashMap::<String, Vec<Value>>::new(),
                |mut acc, t| {
                    let schema = t["schema_name"].as_str().unwrap().to_string();
                    let table = json!({
                        "table_name": t["table_name"].as_str().unwrap(),
                        "table_type": t["table_type"].as_str().unwrap_or("BASE TABLE"),
                        "is_selected": true,
                    });
                    acc.entry(schema).or_default().push(table);
                    acc
                },
            )
            .into_iter()
            .map(|(schema_name, tables)| {
                json!({
                    "schema_name": schema_name,
                    "is_selected": true,
                    "tables": tables,
                })
            })
            .collect();

        self.run_discovery_job(
            ds_id,
            json!({"action": "save_catalog", "schemas": save_schemas}),
        )
        .await;
    }

    /// Submit a discovery job and poll until completion, returning the result data.
    async fn run_discovery_job(&self, ds_id: Uuid, request: Value) -> Value {
        // Submit
        let resp = self
            .admin
            .post(&format!("/api/v1/datasources/{ds_id}/discover"))
            .authorization_bearer(&self.admin_token)
            .json(&request)
            .await;
        resp.assert_status(axum::http::StatusCode::ACCEPTED);
        let job_id = resp.json::<Value>()["job_id"].as_str().unwrap().to_string();

        // Poll until done (max 30s)
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let status_resp = self
                .admin
                .get(&format!("/api/v1/datasources/{ds_id}/discover/{job_id}"))
                .authorization_bearer(&self.admin_token)
                .await;
            status_resp.assert_status_ok();
            let body = status_resp.json::<Value>();
            let status = body["status"].as_str().unwrap();

            match status {
                "completed" => {
                    return body["result"].clone();
                }
                "failed" => {
                    panic!(
                        "Discovery job failed: {}",
                        body["error"].as_str().unwrap_or("unknown")
                    );
                }
                "running" => {
                    if tokio::time::Instant::now() > deadline {
                        panic!("Discovery job timed out after 30s");
                    }
                }
                other => panic!("Unexpected job status: {other}"),
            }
        }
    }

    /// Create a non-admin user via the admin API and assign them to a datasource.
    pub async fn create_user(&self, username: &str, password: &str, ds_id: Uuid) -> Uuid {
        // Create user
        let resp = self
            .admin
            .post("/api/v1/users")
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "username": username,
                "password": password,
                "is_admin": false,
            }))
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        let user_id: Uuid = resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();

        // Assign user to datasource (add to existing users list)
        // First get current users
        let current_resp = self
            .admin
            .get(&format!("/api/v1/datasources/{ds_id}/users"))
            .authorization_bearer(&self.admin_token)
            .await;
        current_resp.assert_status_ok();
        let mut user_ids: Vec<Uuid> = current_resp
            .json::<Value>()
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|u| u["id"].as_str()?.parse().ok())
            .collect();
        user_ids.push(user_id);

        let assign_resp = self
            .admin
            .put(&format!("/api/v1/datasources/{ds_id}/users"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({ "user_ids": user_ids }))
            .await;
        assign_resp.assert_status(axum::http::StatusCode::NO_CONTENT);

        user_id
    }

    /// Create a policy and assign it to a datasource.
    pub async fn create_and_assign_policy(
        &self,
        name: &str,
        policy_type: &str,
        targets: Vec<Value>,
        definition: Option<Value>,
        ds_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Uuid {
        self.create_and_assign_policy_enabled(
            name,
            policy_type,
            targets,
            definition,
            ds_id,
            user_id,
            true,
        )
        .await
    }

    /// Create a policy (with is_enabled control) and assign it to a datasource.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_and_assign_policy_enabled(
        &self,
        name: &str,
        policy_type: &str,
        targets: Vec<Value>,
        definition: Option<Value>,
        ds_id: Uuid,
        user_id: Option<Uuid>,
        is_enabled: bool,
    ) -> Uuid {
        // Create policy
        let resp = self
            .admin
            .post("/api/v1/policies")
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "name": name,
                "policy_type": policy_type,
                "is_enabled": is_enabled,
                "targets": targets,
                "definition": definition,
            }))
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        let policy_id: Uuid = resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();

        // Assign to datasource
        let assign_resp = self
            .admin
            .post(&format!("/api/v1/datasources/{ds_id}/policies"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "policy_id": policy_id,
                "user_id": user_id,
            }))
            .await;
        assign_resp.assert_status(axum::http::StatusCode::CREATED);

        policy_id
    }

    /// Shortcut: create a row_filter policy.
    #[allow(dead_code)]
    pub async fn create_row_filter(
        &self,
        name: &str,
        schema: &str,
        table: &str,
        filter: &str,
        ds_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Uuid {
        self.create_and_assign_policy(
            name,
            "row_filter",
            vec![json!({"schemas": [schema], "tables": [table]})],
            Some(json!({"filter_expression": filter})),
            ds_id,
            user_id,
        )
        .await
    }

    /// Shortcut: create a column_allow policy.
    #[allow(dead_code)]
    pub async fn create_column_allow(
        &self,
        name: &str,
        schema: &str,
        table: &str,
        columns: &[&str],
        ds_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Uuid {
        self.create_and_assign_policy(
            name,
            "column_allow",
            vec![json!({"schemas": [schema], "tables": [table], "columns": columns})],
            None,
            ds_id,
            user_id,
        )
        .await
    }

    /// Shortcut: create a column_deny policy.
    #[allow(dead_code)]
    pub async fn create_column_deny(
        &self,
        name: &str,
        schema: &str,
        table: &str,
        columns: &[&str],
        ds_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Uuid {
        self.create_and_assign_policy(
            name,
            "column_deny",
            vec![json!({"schemas": [schema], "tables": [table], "columns": columns})],
            None,
            ds_id,
            user_id,
        )
        .await
    }

    /// Shortcut: create a column_mask policy.
    #[allow(dead_code, clippy::too_many_arguments)]
    pub async fn create_column_mask(
        &self,
        name: &str,
        schema: &str,
        table: &str,
        column: &str,
        mask: &str,
        ds_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Uuid {
        self.create_and_assign_policy(
            name,
            "column_mask",
            vec![json!({"schemas": [schema], "tables": [table], "columns": [column]})],
            Some(json!({"mask_expression": mask})),
            ds_id,
            user_id,
        )
        .await
    }

    /// Shortcut: create a table_deny policy.
    #[allow(dead_code)]
    pub async fn create_table_deny(
        &self,
        name: &str,
        schema: &str,
        table: &str,
        ds_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Uuid {
        self.create_and_assign_policy(
            name,
            "table_deny",
            vec![json!({"schemas": [schema], "tables": [table]})],
            None,
            ds_id,
            user_id,
        )
        .await
    }

    /// Execute SQL directly on the upstream Postgres container (not through proxy).
    pub async fn seed_upstream(&self, sql: &str) {
        let pg = shared_postgres().expect("postgres required");
        let (client, conn) = tokio_postgres::connect(&pg.url, NoTls).await.unwrap();
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("seed_upstream: connection driver error: {e}");
            }
        });
        client.batch_execute(sql).await.unwrap();
    }

    /// Connect to the proxy as a specific user against a specific datasource.
    pub async fn connect_as(
        &self,
        username: &str,
        password: &str,
        datasource: &str,
    ) -> tokio_postgres::Client {
        let connstr = format!(
            "host=127.0.0.1 port={} user={} password={} dbname={} sslmode=disable",
            self.proxy_port, username, password, datasource,
        );
        let (client, conn) = tokio_postgres::connect(&connstr, NoTls).await.unwrap();
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("connect_as: connection driver error: {e}");
            }
        });
        client
    }

    /// Try to connect to the proxy; returns Err if the connection fails (auth, etc.)
    pub async fn try_connect_as(
        &self,
        username: &str,
        password: &str,
        datasource: &str,
    ) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
        let connstr = format!(
            "host=127.0.0.1 port={} user={} password={} dbname={} sslmode=disable",
            self.proxy_port, username, password, datasource,
        );
        let (client, conn) = tokio_postgres::connect(&connstr, NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("try_connect_as: connection driver error: {e}");
            }
        });
        Ok(client)
    }

    /// Create a non-admin user via the admin API WITHOUT assigning them to any datasource.
    /// Used for testing that unassigned users cannot access a datasource.
    #[allow(dead_code)]
    pub async fn create_user_unassigned(&self, username: &str, password: &str) -> Uuid {
        let resp = self
            .admin
            .post("/api/v1/users")
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "username": username,
                "password": password,
                "is_admin": false,
            }))
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    // -- RBAC helpers --

    /// Create a role via the admin API.
    #[allow(dead_code)]
    pub async fn create_role(&self, name: &str) -> Uuid {
        let resp = self
            .admin
            .post("/api/v1/roles")
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "name": name,
            }))
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    /// Add a user as a member of a role.
    #[allow(dead_code)]
    pub async fn add_role_member(&self, role_id: Uuid, user_id: Uuid) {
        let resp = self
            .admin
            .post(&format!("/api/v1/roles/{role_id}/members"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({ "user_ids": [user_id] }))
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);
    }

    /// Add a parent role to a child role (child inherits from parent).
    #[allow(dead_code)]
    pub async fn add_role_parent(&self, child_role_id: Uuid, parent_role_id: Uuid) {
        let resp = self
            .admin
            .post(&format!("/api/v1/roles/{child_role_id}/parents"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({ "parent_role_id": parent_role_id }))
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);
    }

    /// Set role-based datasource access.
    #[allow(dead_code)]
    pub async fn set_datasource_role_access(&self, ds_id: Uuid, role_ids: &[Uuid]) {
        let resp = self
            .admin
            .put(&format!("/api/v1/datasources/{ds_id}/access/roles"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({ "role_ids": role_ids }))
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);
    }

    /// Assign a policy to a datasource with role scope.
    #[allow(dead_code)]
    pub async fn assign_policy_to_role(
        &self,
        ds_id: Uuid,
        policy_id: Uuid,
        role_id: Uuid,
        priority: i32,
    ) {
        let resp = self
            .admin
            .post(&format!("/api/v1/datasources/{ds_id}/policies"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "policy_id": policy_id,
                "role_id": role_id,
                "scope": "role",
                "priority": priority,
            }))
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
    }

    /// Assign a policy to a datasource with scope='all'.
    #[allow(dead_code)]
    pub async fn assign_policy_to_all(&self, ds_id: Uuid, policy_id: Uuid, priority: i32) {
        let resp = self
            .admin
            .post(&format!("/api/v1/datasources/{ds_id}/policies"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "policy_id": policy_id,
                "scope": "all",
                "priority": priority,
            }))
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
    }

    /// Deactivate a role.
    #[allow(dead_code)]
    pub async fn deactivate_role(&self, role_id: Uuid) {
        let resp = self
            .admin
            .put(&format!("/api/v1/roles/{role_id}"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({ "is_active": false }))
            .await;
        resp.assert_status_ok();
    }

    /// Reactivate a role.
    #[allow(dead_code)]
    pub async fn reactivate_role(&self, role_id: Uuid) {
        let resp = self
            .admin
            .put(&format!("/api/v1/roles/{role_id}"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({ "is_active": true }))
            .await;
        resp.assert_status_ok();
    }

    /// Remove a user from a role.
    #[allow(dead_code)]
    pub async fn remove_role_member(&self, role_id: Uuid, user_id: Uuid) {
        let resp = self
            .admin
            .delete(&format!("/api/v1/roles/{role_id}/members/{user_id}"))
            .authorization_bearer(&self.admin_token)
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);
    }

    /// Remove direct user access from a datasource (set empty user list).
    #[allow(dead_code)]
    pub async fn remove_user_from_datasource(&self, ds_id: Uuid, user_id: Uuid) {
        // Get current users, remove the target
        let current_resp = self
            .admin
            .get(&format!("/api/v1/datasources/{ds_id}/users"))
            .authorization_bearer(&self.admin_token)
            .await;
        current_resp.assert_status_ok();
        let user_ids: Vec<Uuid> = current_resp
            .json::<Value>()
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|u| u["id"].as_str()?.parse().ok())
            .filter(|id: &Uuid| *id != user_id)
            .collect();

        let assign_resp = self
            .admin
            .put(&format!("/api/v1/datasources/{ds_id}/users"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({ "user_ids": user_ids }))
            .await;
        assign_resp.assert_status(axum::http::StatusCode::NO_CONTENT);
    }

    /// Delete a role.
    #[allow(dead_code)]
    pub async fn delete_role(&self, role_id: Uuid) {
        let resp = self
            .admin
            .delete(&format!("/api/v1/roles/{role_id}"))
            .authorization_bearer(&self.admin_token)
            .await;
        resp.assert_status_ok();
    }

    /// Create a policy without assigning it to a datasource. Returns the policy ID.
    #[allow(dead_code)]
    pub async fn create_policy(
        &self,
        name: &str,
        policy_type: &str,
        targets: Vec<Value>,
        definition: Option<Value>,
    ) -> Uuid {
        let resp = self
            .admin
            .post("/api/v1/policies")
            .authorization_bearer(&self.admin_token)
            .json(&json!({
                "name": name,
                "policy_type": policy_type,
                "is_enabled": true,
                "targets": targets,
                "definition": definition,
            }))
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    /// Create a user without assigning to any datasource; only role-based access.
    /// This creates a user via the admin API but does NOT add them to the datasource
    /// user list. They can still connect if they have role-based access.
    #[allow(dead_code)]
    pub async fn create_user_no_direct_access(&self, username: &str, password: &str) -> Uuid {
        self.create_user_unassigned(username, password).await
    }

    // -- ABAC helpers --

    /// Create an attribute definition.
    #[allow(dead_code)]
    pub async fn create_attribute_definition(
        &self,
        key: &str,
        entity_type: &str,
        value_type: &str,
        allowed_values: Option<Vec<&str>>,
    ) -> Uuid {
        self.create_attribute_definition_with_default(
            key,
            entity_type,
            value_type,
            allowed_values,
            None,
        )
        .await
    }

    /// Create an attribute definition with an optional default_value.
    #[allow(dead_code)]
    pub async fn create_attribute_definition_with_default(
        &self,
        key: &str,
        entity_type: &str,
        value_type: &str,
        allowed_values: Option<Vec<&str>>,
        default_value: Option<&str>,
    ) -> Uuid {
        let mut body = json!({
            "key": key,
            "entity_type": entity_type,
            "display_name": key,
            "value_type": value_type,
        });
        if let Some(av) = allowed_values {
            body["allowed_values"] = json!(av);
        }
        if let Some(dv) = default_value {
            body["default_value"] = json!(dv);
        }
        let resp = self
            .admin
            .post("/api/v1/attribute-definitions")
            .authorization_bearer(&self.admin_token)
            .json(&body)
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    /// Set attributes on a user (full replace).
    #[allow(dead_code)]
    pub async fn set_user_attributes(&self, user_id: Uuid, attributes: Value) {
        let resp = self
            .admin
            .put(&format!("/api/v1/users/{user_id}"))
            .authorization_bearer(&self.admin_token)
            .json(&json!({ "attributes": attributes }))
            .await;
        resp.assert_status_ok();
    }
}
