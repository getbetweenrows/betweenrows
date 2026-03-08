use clap::{Parser, Subcommand};
use migration::{Migrator, MigratorTrait};
use proxy::admin::{AdminState, admin_router};
use proxy::auth::Auth;
use proxy::engine::EngineCache;
use proxy::handler::ProxyHandler;
use proxy::hooks::policy::PolicyHook;
use proxy::server::process_socket_with_idle_timeout;
use rand_core::RngCore;
use sea_orm::Database;
use socket2::{SockRef, TcpKeepalive};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "proxy", about = "QueryProxy — PostgreSQL wire protocol proxy")]
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

    let cli = Cli::parse();

    // Connect to admin DB and run migrations
    let database_url = std::env::var("BR_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://proxy_admin.db?mode=rwc".to_string());

    tracing::info!(database = %redact_db_url(&database_url), "connecting to database");

    let db = Database::connect(&database_url).await?;
    Migrator::up(&db, None).await?;

    tracing::info!("database initialized");

    let auth = Arc::new(Auth::new(db.clone()));

    // Parse (or generate) ENCRYPTION_KEY
    let master_key = parse_or_generate_encryption_key();

    match cli.command {
        None | Some(Commands::Serve) => {
            serve(auth, db, master_key).await?;
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

/// Parse BR_ENCRYPTION_KEY env var as 64-char hex → [u8; 32].
/// If unset, generate a random key and warn (tokens will not survive restarts).
fn parse_or_generate_encryption_key() -> [u8; 32] {
    match std::env::var("BR_ENCRYPTION_KEY") {
        Ok(hex) => match parse_hex_key(&hex) {
            Ok(key) => key,
            Err(e) => {
                eprintln!(
                    "FATAL: BR_ENCRYPTION_KEY is invalid: {}. \
                         Fix the value or unset it to use a random key.",
                    e
                );
                std::process::exit(1);
            }
        },
        Err(_) => {
            tracing::warn!(
                "BR_ENCRYPTION_KEY not set — using a random key. \
                 Encrypted data will be unreadable after restart. \
                 Set BR_ENCRYPTION_KEY to a 64-char hex string (32 bytes) in production."
            );
            random_key()
        }
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
            .map_err(|_| format!("invalid hex character in ENCRYPTION_KEY at byte {}", i))?;
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

    // ── Engine cache ──────────────────────────────────────────────────────────
    let engine_cache = EngineCache::new(db.clone(), master_key);

    // ── Policy hook (shared between admin API and proxy handler) ──────────────
    let policy_hook = PolicyHook::new(db.clone());

    // ── Admin REST API ────────────────────────────────────────────────────────
    let jwt_secret = std::env::var("BR_ADMIN_JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!(
            "BR_ADMIN_JWT_SECRET not set — using a random secret. \
             Tokens will be invalidated on every restart."
        );
        let mut bytes = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut bytes);
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    });

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
