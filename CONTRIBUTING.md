# Development Guide

## Build from Source

```bash
# Proxy (Rust)
cargo build -p proxy
cargo test -p proxy

# Admin UI (React)
cd admin-ui && npm install && npm run dev
# → http://localhost:5173

# Production UI bundle
cd admin-ui && npm run build
```

Hot reload:
```bash
cargo watch -x "run -p proxy"
```

## Pre-commit Hook

`.githooks/pre-commit` runs `cargo fmt --check`, `cargo clippy`, and `admin-ui` tests. Enable once per clone:

```bash
git config core.hooksPath .githooks
```

## Project Structure

```
betweenrows/
├── Cargo.toml                        workspace root (proxy, migration crates)
├── migration/src/                    SeaORM migrations (41 total)
├── docs/                             User-facing documentation
│   ├── permission-system.md          Policy system user guide
│   ├── security-vectors.md           Security attack vectors & test plan
│   ├── permission-stories.md         Detailed permission use cases
│   └── roadmap.md                    Project roadmap and backlog
├── scripts/demo_ecommerce/           Demo schema + seed data
├── admin-ui/                         React admin console
│   └── src/
│       ├── api/                      axios + fetch-event-source clients
│       ├── auth/                     AuthContext, ProtectedRoute, LoginPage
│       ├── components/               Layout, DataSourceForm, CatalogDiscoveryWizard,
│       │                             PolicyForm, PolicyAssignmentPanel, RoleForm,
│       │                             RoleMemberPanel, RoleInheritancePanel, AuditTimeline, …
│       ├── pages/                    Users*, DataSources*, DataSourceCatalogPage,
│       │                             Policies*, Roles*, QueryAuditPage
│       └── types/                    TypeScript interfaces
└── proxy/src/
    ├── main.rs                       entry point: CLI, DB init, EngineCache, servers
    ├── server.rs                     process_socket_with_idle_timeout (idle + startup timeouts)
    ├── handler.rs                    pgwire StartupHandler + query handlers
    ├── auth.rs                       Argon2 auth, user creation
    ├── crypto.rs                     AES-256-GCM encrypt/decrypt
    ├── admin/                        REST API: mod, dto, jwt, handlers, discovery_job,
    │                                 policy_handlers, role_handlers, audit_handlers,
    │                                 admin_audit
    ├── discovery/                    DiscoveryProvider trait + Postgres impl
    ├── entity/                       SeaORM entities (proxy_user, data_source, role,
    │                                 role_member, role_inheritance, data_source_access,
    │                                 policy, policy_assignment, policy_version,
    │                                 admin_audit_log, query_audit_log, …)
    ├── role_resolver.rs              BFS role resolution, cycle detection, effective assignments
    ├── engine/mod.rs                 EngineCache, VirtualCatalogProvider, build_arrow_schema()
    └── hooks/                        QueryHook trait, ReadOnlyHook, PolicyHook
```

## Architecture

```
psql / app
    ↓  PostgreSQL wire protocol (port 5434)
BetweenRows (Rust)
    ├─ Authenticates user (Argon2id)
    ├─ Checks data source access (data_source_access table — direct, role-based, or all)
    ├─ Runs query hook pipeline:
    │      ReadOnlyHook  — blocks writes (SQLSTATE 25006)
    │      PolicyHook    — row filters, column masks, column access control
    └─ Executes via DataFusion + tokio-postgres federation
    ↓
Upstream PostgreSQL
```

## Tech Stack

| Layer | Library | Version |
|---|---|---|
| Protocol | pgwire | 0.38 |
| Query engine | DataFusion | 52 |
| PG federation | datafusion-table-providers | 0.10 |
| Async runtime | Tokio | 1 |
| Admin store | SeaORM + SQLite/PG | 1 |
| Password hashing | argon2 (Argon2id) | 0.5 |
| Secret encryption | aes-gcm (AES-256-GCM) | 0.10 |
| Admin REST API | axum + tower-http | 0.8 / 0.6 |
| Admin auth | jsonwebtoken (HMAC-SHA256) | 9 |
| CLI | clap | 4 |
| Admin UI | React 19 + Vite 6 + Tailwind 4 + TanStack Query 5 | — |

## Security

### Access Control Architecture

Access control is enforced **before** any query reaches the engine:

1. `validate_data_source()` — datasource must exist and be active
2. `check_access(user_id, datasource_name)` — user must have access via `data_source_access` (direct, role-based, or all-scoped)
3. If either check fails → `FATAL` PG error, connection rejected before `get_ctx()` is ever called

### Why the Shared Pool Is Safe

The upstream connection pool carries **no user identity** — it is pure TCP connectivity to the upstream Postgres server. All identity and access decisions are made at the pgwire auth layer (steps 1–2 above), not at the pool layer.

Per-user isolation is enforced by:
- **Data plane** — `data_source_access` allowlist (no matching row → connection rejected). Access can be granted directly to a user, via role membership (including inherited roles), or to all users.
- **Policy hook** — per-query row filters, column masks, and access controls injected via DataFusion's logical plan tree, based on the authenticated user's policy assignments (direct, role-based, or wildcard)
- **Virtual catalog** — the stored catalog is an allowlist; tables/columns not explicitly saved are invisible to the engine

The shared pool is safe for all authorized users of a datasource: Pool = "how to talk to upstream". Auth + RLS = "what this user can see". These are orthogonal.

### Policy Enforcement Resistance

`PolicyHook` injects row filters and column transforms at the DataFusion logical plan level via `transform_up`. The filter is applied below the `TableScan` node — it cannot be bypassed by table aliases, CTEs, or subqueries, since DataFusion inlines those into the plan before transformation.

Template variable substitution (`{user.username}`, `{user.id}`, custom attributes like `{user.tenant}`, etc.) uses parse-then-substitute: the filter expression is parsed into a `DataFusion Expr` tree first, then placeholder identifiers are replaced with typed `Expr::Literal` values. The user's values never pass through the SQL parser, preventing injection even if the value contains SQL syntax.

### Permissions Model

BetweenRows enforces a two-layer access control model:

**Management plane** — controlled by `is_admin` flag. Admins manage users, data sources, policies, and catalogs via the Admin API. Non-admins have no Admin API access.

**Data plane** — controlled by two independent mechanisms:
1. *Connection access* — `data_source_access` entries. A user can connect to a datasource via direct assignment, role membership (including inherited roles), or all-user scope. Being an admin does **not** automatically grant data plane access.
2. *Query policy* — `PolicyHook` applies row filters, column masks, and column access controls per-query based on assigned policies (direct, role-based, or all-scoped). If the datasource `access_mode` is `"policy_required"`, tables with no matching permit policy return empty results. Policies can reference built-in identity fields (`{user.username}`, `{user.id}`) and custom user attributes (`{user.KEY}`, e.g., `{user.tenant}`) for attribute-based access control (ABAC). Optional decision functions (JavaScript/WASM) provide programmable policy gates.

See `docs/permission-system.md` for the full policy system user guide.

## Performance

### Arrow Type Alignment (query time)

During catalog discovery, column types are captured using `datafusion-table-providers`' own `get_schema()` function rather than a manual PG-to-Arrow mapping. This guarantees that the stored schema matches exactly what the library produces at query time.

**Why it matters:** an earlier hand-written `pg_type_to_arrow()` mapped `numeric` → `Decimal128(38,10)` and `timestamp` → `Timestamp(Microsecond)`, but the library internally uses `Decimal128(38,20)` and `Timestamp(Nanosecond)`. The mismatch triggered a full schema-cast on every result batch, adding 12–23 s to queries returning ~2 k rows. With `get_schema()`, stored types and runtime types are identical — no cast overhead.

**Do not** replace this with a manual PG type map. If new PG types need support, add them to `parse_arrow_type()` / `arrow_type_to_string()` in `engine/mod.rs` alongside a round-trip test.

### Lazy Connection Pool

The upstream Postgres connection pool (`LazyPool` in `engine/mod.rs`) is **not** created when a client connects — it is created on the first query that touches a user table. Catalog queries (`pg_catalog`, `information_schema`) work instantly without an upstream connection.

This means:
- TablePlus / psql sidebar population (all `pg_catalog` queries) is instant.
- Clients that never issue user-table queries pay zero upstream connection cost.

**Do not** move pool creation back into `create_session_context_from_catalog()` or `EngineCache::get_context()`.

### Shared Pool Across Context Rebuilds

`EngineCache` stores one `Arc<LazyPool>` per datasource in a separate `pools` map. `invalidate(name)` (called after catalog re-discovery) removes only the `SessionContext`, keeping the pool. The next `get_context()` call reuses the existing pool rather than creating a new one.

`invalidate_all(name)` (called after datasource connection params are edited or the datasource is deleted) removes both the `SessionContext` and the pool.

**Do not** call `invalidate_all` after catalog operations. **Do not** call plain `invalidate` after datasource edit/delete — the pool would be stale.

### Idle Connection Timeout

pgwire 0.38 has no built-in idle timeout — `socket.next().await` blocks indefinitely after authentication. This prevents Fly.io `auto_stop_machines` from ever triggering when a GUI client like TablePlus is open, because the VM only stops when it has zero connections.

`proxy/src/server.rs` replaces pgwire's `process_socket` with a custom message loop (`process_socket_with_idle_timeout`) that adds a `tokio::select!` branch racing each `socket.next()` against a `sleep(idle_timeout)`. The timer resets after every received message — a running query does not count as idle.

Default timeout is 15 minutes (`BR_IDLE_TIMEOUT_SECS=900`). TCP keepalive (60 s time, 10 s interval) is also set on each accepted socket to detect dead connections from crashed clients or network failures.

### Background Warmup

After authentication succeeds in `handler.rs`, a background task pre-builds the `SessionContext` (DB queries to load the stored catalog) and eagerly initialises the `LazyPool`. This amortises first-query latency during the window between the client's auth handshake and its first query.

### Performance Regression Testing

There is currently no automated performance regression suite. Meaningful regression detection requires integration-level tests against a real Postgres instance that can verify filter pushdown is still active, connection pool reuse is intact, and end-to-end query latency stays within bounds. This is planned for a future iteration.

## Data Model

All primary keys are UUIDs. The admin store uses SQLite by default (configurable via `DATABASE_URL`).

```
proxy_user         (id UUID, username, password_hash, is_admin, is_active, attributes JSON, …)
data_source        (id UUID, name, ds_type, config JSON, secure_config encrypted,
                    is_active, access_mode, last_sync_at, last_sync_result, …)
data_source_access (id UUID, user_id?, role_id?, data_source_id, assignment_scope, …)
role               (id UUID, name UNIQUE, description, is_active, …)
role_member        (id UUID, role_id → role, user_id → proxy_user)
role_inheritance   (id UUID, parent_role_id → role, child_role_id → role)
discovered_schema  (id UUID v5, data_source_id, schema_name, is_selected)
discovered_table   (id UUID v5, discovered_schema_id, table_name, table_type, is_selected)
discovered_column  (id UUID v5, discovered_table_id, column_name, ordinal_position,
                    data_type, is_nullable, column_default, arrow_type)

policy             (id UUID v7, name, description, policy_type, is_enabled, version, targets JSON, definition JSON, …)
policy_version     (id UUID v7, policy_id, version, snapshot JSON, change_type, changed_by)
policy_assignment  (id UUID v7, policy_id, data_source_id, user_id?, role_id?,
                    assignment_scope, priority)
admin_audit_log    (id UUID v7, resource_type, resource_id, action, actor_id, changes JSON, created_at)
query_audit_log    (id UUID v7, user_id, username, data_source_id, datasource_name,
                    original_query, rewritten_query, policies_applied JSON,
                    execution_time_ms, client_info, created_at)
```

Catalog entity IDs (schemas, tables, columns) are deterministic UUID v5 fingerprints derived from their natural keys. Re-discovering the same upstream object always produces the same ID, so re-syncs are safe upserts.

## Docker (Development)

```bash
docker compose up                                                   # dev (hot reload)
docker compose -f compose.yaml -f compose.prod.yaml up --build     # prod
```
