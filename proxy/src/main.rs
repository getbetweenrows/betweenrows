use clap::{Parser, Subcommand};
use migration::{Migrator, MigratorTrait};
use proxy::admin::{AdminState, admin_router};
use proxy::auth::Auth;
use proxy::engine::EngineCache;
use proxy::handler::ProxyHandler;
use proxy::hooks::policy::PolicyHook;
use proxy::server::process_socket_with_idle_timeout;
use rand_core::RngCore;
use sea_orm::{ColumnTrait, ConnectOptions, Database, EntityTrait, QueryFilter};
use socket2::{SockRef, TcpKeepalive};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "proxy", about = "BetweenRows — Data access governance")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy server (default)
    Serve,
    /// Manage proxy users
    User {
        #[command(subcommand)]
        action: UserAction,
    },
}

#[derive(Subcommand)]
enum UserAction {
    /// Create a new proxy user
    Create {
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: String,
        #[arg(long)]
        tenant: String,
        #[arg(long, action = clap::ArgAction::SetTrue)]
        admin: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Init structured logging (respects RUST_LOG; defaults to info)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load .env if present
    dotenvy::dotenv().ok();

    eprintln!("╔══════════════════════════════╗");
    eprintln!("║  BetweenRows v{:<14}║", env!("CARGO_PKG_VERSION"));
    eprintln!("║  Data access governance      ║");
    eprintln!("╚══════════════════════════════╝");

    let cli = Cli::parse();

    // Connect to admin DB and run migrations
    let database_url = std::env::var("BR_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://proxy_admin.db?mode=rwc".to_string());

    tracing::info!(database = %redact_db_url(&database_url), "connecting to database");

    let mut opt = ConnectOptions::new(&database_url);
    let sqlx_debug = tracing::level_filters::LevelFilter::current()
        >= tracing::level_filters::LevelFilter::DEBUG;
    opt.sqlx_logging(sqlx_debug);
    let db = Database::connect(opt).await?;
    Migrator::up(&db, None).await?;

    // Recompile any decision functions that have JS source but no WASM bytecode
    // (e.g. after migration). This only shells out to the javy CLI — it does not
    // require the WasmDecisionRuntime created later in serve().
    recompile_decision_functions(&db).await;

    tracing::info!("database initialized");

    let auth = Arc::new(Auth::new(db.clone()));

    // Resolve secrets (env var → persisted file → generate & persist)
    let data_dir = data_dir_from_database_url();
    let state_dir = data_dir.join(".betweenrows");
    tracing::info!(path = %data_dir.display(), "data directory");
    warn_if_data_dir_not_persistent(&data_dir, &state_dir);
    let master_key = resolve_hex_secret("BR_ENCRYPTION_KEY", &state_dir.join("encryption_key"));

    match cli.command {
        None | Some(Commands::Serve) => {
            serve(auth, db, master_key, &state_dir).await?;
        }
        Some(Commands::User { action }) => {
            handle_user_action(auth, action).await?;
        }
    }

    Ok(())
}

/// Redact the password from a database URL for safe logging.
/// Strips query params and replaces inline password: `scheme://user:pass@host` → `scheme://user:****@host`.
fn redact_db_url(url: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    if let Some(at) = base.rfind('@')
        && let Some(scheme_end) = base.find("://")
    {
        let userinfo = &base[scheme_end + 3..at];
        if let Some(colon) = userinfo.find(':') {
            let user = &userinfo[..colon];
            let rest = &base[at..];
            return format!("{}://{}:****{}", &base[..scheme_end], user, rest);
        }
    }
    base.to_string()
}

/// Resolve a hex secret: env var → persisted file → generate & persist.
///
/// 1. If `env_var` is set, parse it as 64-char hex and use it (production path).
/// 2. If not set, check `file_path` — if it exists, load from file.
/// 3. If file doesn't exist, generate a random 32-byte key, write it to `file_path`, and use it.
///
/// Cases 2 and 3 log a warning recommending the env var for production.
fn resolve_hex_secret(env_var: &str, file_path: &std::path::Path) -> [u8; 32] {
    // 1. Env var takes priority
    if let Ok(hex) = std::env::var(env_var) {
        match parse_hex_key(&hex) {
            Ok(key) => return key,
            Err(e) => {
                eprintln!(
                    "FATAL: {env_var} is invalid: {e}. \
                     Fix the value or unset it to use a file-based key."
                );
                std::process::exit(1);
            }
        }
    }

    // 2. Try loading from persisted file
    if file_path.exists() {
        match std::fs::read_to_string(file_path) {
            Ok(hex) => match parse_hex_key(hex.trim()) {
                Ok(key) => {
                    tracing::warn!(
                        "{env_var} not set — loaded from {}. \
                         Set {env_var} to a 64-char hex string for production.",
                        file_path.display()
                    );
                    return key;
                }
                Err(e) => {
                    eprintln!(
                        "FATAL: persisted key at {} is invalid: {e}. \
                         Delete the file or set {env_var} explicitly.",
                        file_path.display()
                    );
                    std::process::exit(1);
                }
            },
            Err(e) => {
                tracing::warn!(
                    "Could not read {}: {e} — generating a new key.",
                    file_path.display()
                );
            }
        }
    }

    // 3. Generate, persist, and return
    let key = random_key();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();

    if let Some(parent) = file_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(file_path, &hex) {
        Ok(()) => {
            tracing::warn!(
                "{env_var} not set — generated and saved to {}. \
                 Set {env_var} to a 64-char hex string for production.",
                file_path.display()
            );
        }
        Err(e) => {
            tracing::warn!(
                "{env_var} not set and could not persist to {}: {e}. \
                 Using an ephemeral key — data will be lost on restart. \
                 Set {env_var} to a 64-char hex string for production.",
                file_path.display()
            );
        }
    }
    key
}

/// Resolve a string secret from env var → file → generate.
///
/// Unlike `resolve_hex_secret`, this accepts any non-empty string as the env var value
/// (no hex constraint). Used for secrets that don't need to be exactly 32 bytes (e.g. JWT).
/// When auto-generating, produces a 64-char hex string for good entropy.
fn resolve_string_secret(env_var: &str, file_path: &std::path::Path) -> String {
    // 1. Env var takes priority — accept any non-empty string
    if let Ok(val) = std::env::var(env_var) {
        let val = val.trim().to_string();
        if val.is_empty() {
            eprintln!(
                "FATAL: {env_var} is set but empty. \
                 Fix the value or unset it to use a file-based key."
            );
            std::process::exit(1);
        }
        return val;
    }

    // 2. Try loading from persisted file
    if file_path.exists() {
        match std::fs::read_to_string(file_path) {
            Ok(val) => {
                let val = val.trim().to_string();
                if !val.is_empty() {
                    tracing::warn!(
                        "{env_var} not set — loaded from {}. \
                         Set {env_var} for production.",
                        file_path.display()
                    );
                    return val;
                }
                tracing::warn!(
                    "Persisted key at {} is empty — generating a new key.",
                    file_path.display()
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Could not read {}: {e} — generating a new key.",
                    file_path.display()
                );
            }
        }
    }

    // 3. Generate, persist, and return
    let key = random_key();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();

    if let Some(parent) = file_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(file_path, &hex) {
        Ok(()) => {
            tracing::warn!(
                "{env_var} not set — generated and saved to {}. \
                 Set {env_var} for production.",
                file_path.display()
            );
        }
        Err(e) => {
            tracing::warn!(
                "{env_var} not set and could not persist to {}: {e}. \
                 Using an ephemeral key — tokens will be invalid on restart. \
                 Set {env_var} for production.",
                file_path.display()
            );
        }
    }
    hex
}

/// Derive the data directory from BR_ADMIN_DATABASE_URL.
/// For SQLite URLs like `sqlite:///data/proxy_admin.db?mode=rwc`, returns `/data`.
/// Falls back to the current directory.
fn data_dir_from_database_url() -> std::path::PathBuf {
    let url = std::env::var("BR_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://proxy_admin.db?mode=rwc".to_string());

    // Strip query params
    let path_part = url.split('?').next().unwrap_or(&url);

    // Strip sqlite:// prefix
    if let Some(stripped) = path_part.strip_prefix("sqlite://") {
        let db_path = std::path::Path::new(stripped);
        if let Some(parent) = db_path.parent()
            && !parent.as_os_str().is_empty()
        {
            return parent.to_path_buf();
        }
    }

    std::path::PathBuf::from(".")
}

/// Warn if the data directory looks like an ephemeral container filesystem.
///
/// The `.betweenrows/` state directory acts as a persistence marker. If the data
/// directory contains DB files but `.betweenrows/` is missing, it means the state
/// dir was lost between restarts — the volume likely wasn't mounted.
fn warn_if_data_dir_not_persistent(data_dir: &std::path::Path, state_dir: &std::path::Path) {
    if state_dir.exists() {
        return;
    }

    let db_exists = data_dir
        .read_dir()
        .ok()
        .and_then(|mut entries| {
            entries.find(|e| {
                e.as_ref()
                    .map(|e| e.file_name().to_string_lossy().ends_with(".db"))
                    .unwrap_or(false)
            })
        })
        .is_some();

    if db_exists {
        tracing::warn!(
            "Data directory {} may not be persistent — \
             .betweenrows/ state directory was not found alongside existing data. \
             Ensure a volume is mounted (e.g., -v betweenrows_data:{}) \
             to avoid data loss on container restart.",
            data_dir.display(),
            data_dir.display(),
        );
    }
}

fn parse_hex_key(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err(format!(
            "expected 64 hex chars (32 bytes), got {}",
            hex.len()
        ));
    }
    let mut key = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let byte_str =
            std::str::from_utf8(chunk).map_err(|_| "invalid UTF-8 in hex string".to_string())?;
        key[i] = u8::from_str_radix(byte_str, 16)
            .map_err(|_| format!("invalid hex character at byte {i}"))?;
    }
    Ok(key)
}

fn random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut key);
    key
}

async fn serve(
    auth: Arc<Auth>,
    db: sea_orm::DatabaseConnection,
    master_key: [u8; 32],
    state_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Auto-seed default admin if no users exist
    if auth.count_users().await? == 0 {
        let admin_user = std::env::var("BR_ADMIN_USER").unwrap_or_else(|_| "admin".to_string());
        let admin_pass = match std::env::var("BR_ADMIN_PASSWORD") {
            Ok(p) if !p.is_empty() => p,
            _ => {
                eprintln!(
                    "FATAL: BR_ADMIN_PASSWORD is not set. \
                     Set this environment variable to a strong password before starting."
                );
                std::process::exit(1);
            }
        };
        let admin_tenant =
            std::env::var("BR_ADMIN_TENANT").unwrap_or_else(|_| "default".to_string());

        tracing::warn!(
            username = %admin_user,
            tenant = %admin_tenant,
            "No users found — seeding default admin."
        );
        auth.create_user(&admin_user, &admin_pass, &admin_tenant, true)
            .await?;
    }

    // ── WASM runtime (single shared instance for all decision function evaluation) ──
    let wasm_runtime = Arc::new(
        proxy::decision::wasm::WasmDecisionRuntime::new().expect("Failed to create WASM runtime"),
    );

    // ── Engine cache ──────────────────────────────────────────────────────────
    let engine_cache = EngineCache::new(db.clone(), master_key, wasm_runtime.clone());

    // ── Policy hook (shared between admin API and proxy handler) ──────────────
    let policy_hook = PolicyHook::new(db.clone(), wasm_runtime.clone());

    // ── Admin REST API ────────────────────────────────────────────────────────
    let jwt_secret = resolve_string_secret("BR_ADMIN_JWT_SECRET", &state_dir.join("jwt_secret"));

    let jwt_expiry_hours: u64 = std::env::var("BR_ADMIN_JWT_EXPIRY_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);

    let admin_bind_addr =
        std::env::var("BR_ADMIN_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:5435".to_string());

    // ── pgwire proxy handler (created before AdminState so it can be shared) ──
    let handler = Arc::new(ProxyHandler::new(
        auth.clone(),
        engine_cache.clone(),
        policy_hook.clone(),
    ));

    let admin_state = AdminState {
        auth: auth.clone(),
        db: db.clone(),
        jwt_secret,
        jwt_expiry_hours,
        engine_cache: engine_cache.clone(),
        master_key,
        job_store: Arc::new(tokio::sync::Mutex::new(
            proxy::admin::discovery_job::JobStore::new(),
        )),
        policy_hook: Some(policy_hook.clone()),
        proxy_handler: Some(handler.clone()),
        wasm_runtime: wasm_runtime.clone(),
    };

    let admin_listener = TcpListener::bind(&admin_bind_addr).await?;
    tracing::info!(addr = %admin_bind_addr, "Admin API online");

    tokio::spawn(async move {
        axum::serve(admin_listener, admin_router(admin_state))
            .await
            .expect("admin server failed");
    });

    let bind_addr =
        std::env::var("BR_PROXY_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:5434".to_string());

    let idle_timeout_secs: u64 = std::env::var("BR_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(900);
    let idle_timeout = Duration::from_secs(idle_timeout_secs);
    tracing::info!(
        secs = idle_timeout_secs,
        "Idle connection timeout configured"
    );

    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(60))
        .with_interval(Duration::from_secs(10));

    let listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!(addr = %bind_addr, "Proxy online");

    loop {
        let (incoming_socket, _) = listener.accept().await?;

        if let Err(e) = SockRef::from(&incoming_socket).set_tcp_keepalive(&keepalive) {
            tracing::warn!(error = %e, "Failed to set TCP keepalive");
        }

        let peer_addr = incoming_socket.peer_addr().ok();

        // Assign a unique connection ID and register it so on_startup can look it up.
        // If peer_addr is unavailable, register_connection still generates an ID but
        // on_startup will fail gracefully since it can't find the peer addr in pending_conn_ids.
        let conn_id = peer_addr
            .map(|addr| handler.register_connection(addr))
            .unwrap_or_else(|| handler.alloc_connection_id());

        let handler_clone = handler.clone();
        tokio::spawn(async move {
            if let Err(e) = process_socket_with_idle_timeout(
                incoming_socket,
                handler_clone.clone(),
                idle_timeout,
            )
            .await
            {
                tracing::error!(error = %e, "Connection error");
            }
            // Clean up connection state regardless of how the connection ended
            // (normal close, auth failure, idle timeout, or error).
            handler_clone.cleanup_connection(conn_id, peer_addr);
        });
    }
}

async fn handle_user_action(
    auth: Arc<Auth>,
    action: UserAction,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        UserAction::Create {
            username,
            password,
            tenant,
            admin,
        } => {
            auth.create_user(&username, &password, &tenant, admin)
                .await?;
            tracing::info!(username = %username, tenant = %tenant, is_admin = admin, "Created user");
        }
    }
    Ok(())
}

/// Recompile any decision functions that have JS source but no WASM bytecode.
/// This happens after the migration clears old static WASM, ensuring all functions
/// are compiled in dynamic mode and ready for query-time evaluation.
async fn recompile_decision_functions(db: &sea_orm::DatabaseConnection) {
    use proxy::entity::decision_function::{self, Column, Entity as DecisionFunction};

    let functions = match DecisionFunction::find()
        .filter(Column::DecisionFn.is_not_null())
        .filter(Column::DecisionWasm.is_null())
        .all(db)
        .await
    {
        Ok(fns) => fns,
        Err(e) => {
            tracing::error!(error = %e, "Failed to query decision functions for recompilation");
            return;
        }
    };

    if functions.is_empty() {
        return;
    }

    tracing::info!(
        count = functions.len(),
        "Recompiling decision functions in dynamic mode"
    );

    for df in functions {
        let name = df.name.clone();
        match proxy::decision::wasm::compile_with_javy(&df.decision_fn).await {
            Ok(wasm_bytes) => {
                let mut model: decision_function::ActiveModel = df.into();
                model.decision_wasm = sea_orm::ActiveValue::Set(Some(wasm_bytes));
                if let Err(e) = EntityTrait::update(model).exec(db).await {
                    tracing::error!(name = %name, error = %e, "Failed to save recompiled WASM");
                }
            }
            Err(e) => {
                tracing::error!(
                    name = %name,
                    error = %e,
                    "Failed to recompile decision function"
                );
            }
        }
    }

    tracing::info!("Decision function recompilation complete");
}
