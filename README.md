# QueryProxy

A high-performance PostgreSQL wire protocol proxy in Rust. Sits between clients and upstream Postgres backends, providing query governance (row-level security, data masking) via a hook pipeline. Data sources and users are managed through a REST Admin API and React UI.

## Deploy to Fly.io

[![Deploy to Fly](https://fly.io/launch/deploy.svg)](https://fly.io/launch?source=https://github.com/getbetweenrows/betweenrows)

Provisions a Fly.io VM, builds the Docker image (including the admin UI), mounts a 1 GB
persistent volume for the SQLite database, and starts both services:

| Service | External | Internal | Protocol |
|---------|----------|----------|----------|
| Admin UI + REST API | `:80` / `:443` | `:5435` | HTTP/S |
| pgwire (PostgreSQL) | `:5432` (IPv6) | `:5434` | raw TCP |

After deploying:

| Endpoint | URL / address |
|----------|---------------|
| Admin UI | `https://<app-name>.fly.dev` |
| Admin REST API | `https://<app-name>.fly.dev/api/...` |
| PostgreSQL proxy | `<app-name>.fly.dev:5432` |

**Order of operations matters** — follow these steps in sequence.

### 1. Create the app and volume (one-time)

```sh
fly launch --no-deploy --copy-config --name <your-app-name>
fly volumes create betweenrows_data --size 1 --region <region>
```

### 2. Set secrets before first deploy

The app will crash-loop on startup without `BR_ADMIN_PASSWORD`. Set secrets **before** deploying:

```sh
fly secrets set \
  BR_ENCRYPTION_KEY=$(openssl rand -hex 32) \
  BR_ADMIN_JWT_SECRET=$(openssl rand -hex 32) \
  BR_ADMIN_PASSWORD=<strong-password>
```

### 3. Allocate IP addresses (one-time)

Without IP addresses the app is unreachable from most networks. Shared IPv4 is free:

```sh
flyctl ips allocate-v4 --shared
flyctl ips allocate-v6
```

### 4. Make the GHCR package public

CI/CD deploys via `flyctl deploy --image ghcr.io/getbetweenrows/betweenrows:latest` require the package to be public. In GitHub: **Packages → betweenrows → Package settings → Change visibility → Public**.

### 5. Deploy

```sh
fly deploy --image ghcr.io/getbetweenrows/betweenrows:latest --app <your-app-name>
```

## Upgrading

Pull the latest image and redeploy:

```sh
fly deploy --image ghcr.io/getbetweenrows/betweenrows:latest --app <your-app-name>
```

Or if your `fly.toml` already references the image, just:

```sh
fly deploy
```

### Connecting via pgwire

The pgwire port is accessible for free via **IPv6** (most modern clients resolve it automatically):

```sh
psql "postgresql://admin:<password>@<app-name>.fly.dev:5432/<datasource-name>"
```

**macOS: if the connection times out**, check whether IPv6 is configured:

```sh
ifconfig | grep "inet6" | grep -v "::1" | grep -v "fe80"
```

If that returns nothing, your machine has no routable IPv6 address. Re-enable it:

```sh
sudo networksetup -setv6automatic Wi-Fi
```

Then confirm it's working with `ping6 google.com` and retry the connection.

For **IPv4-only** environments (no IPv6 support), tunnel via WireGuard:

```sh
fly proxy 5432:5434 --app <app-name>
psql "postgresql://admin:<password>@127.0.0.1:5432/<datasource-name>"
```

Or allocate a dedicated IPv4 ($2/mo):

```sh
fly ips allocate-v4 --app <app-name>
```

## How It Works

```
psql / app
    ↓  PostgreSQL wire protocol (port 5434)
QueryProxy (Rust)
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

## Quick Start

```bash
# 1. Start the proxy (auto-creates proxy_admin.db, seeds admin user)
#    BR_ADMIN_PASSWORD is required on first boot (set your own password)
BR_ADMIN_PASSWORD=<your-password> cargo run -p proxy

# 2. Start the Admin UI (separate terminal)
cd admin-ui && npm run dev
# → http://localhost:5173  (login: admin / <your-password>)

# 3. Add a data source via the UI, then connect:
psql "postgresql://admin:admin@127.0.0.1:5434/<datasource-name>"
```

Hot reload:
```bash
cargo watch -x "run -p proxy"
```

Docker:
```bash
docker compose up                                                   # dev (hot reload)
docker compose -f compose.yaml -f compose.prod.yaml up --build     # prod
```

Additional CLI commands:
```bash
cargo run -p proxy -- user create --username alice --password secret --tenant acme
```

## Configuration

| Env var | Default | Description |
|---|---|---|
| `BR_ADMIN_DATABASE_URL` | `sqlite://proxy_admin.db?mode=rwc` | SeaORM connection URL (use `postgres://…` for shared backend) |
| `BR_PROXY_BIND_ADDR` | `127.0.0.1:5434` | pgwire listen address |
| `BR_ADMIN_BIND_ADDR` | `127.0.0.1:5435` | Admin REST API listen address |
| `BR_ENCRYPTION_KEY` | *(random, warns)* | 64-char hex — AES-256-GCM key for secrets at rest. **Set in prod.** |
| `BR_ADMIN_JWT_SECRET` | *(random, warns)* | HMAC-SHA256 signing key. **Set in prod** or tokens invalidate on restart. |
| `BR_ADMIN_JWT_EXPIRY_HOURS` | `24` | JWT lifetime |
| `BR_ADMIN_USER` | `admin` | Auto-seed username |
| `BR_ADMIN_PASSWORD` | *(required on first boot)* | Auto-seed password. **Must be set** when no users exist in DB. |
| `BR_ADMIN_TENANT` | `default` | Auto-seed tenant |
| `BR_IDLE_TIMEOUT_SECS` | `900` (15 min) | Close pgwire connections that receive no messages for this many seconds. Enables Fly.io `auto_stop_machines` to work correctly with GUI clients (e.g. TablePlus) that hold idle connections indefinitely. Set to `0` to disable (not recommended on Fly.io). |
| `BR_CORS_ALLOWED_ORIGINS` | *(empty, same-origin only)* | Comma-separated list of allowed CORS origins for the Admin API |
| `RUST_LOG` | `info` | Log filter (standard Rust/tracing convention) |

## Connecting via pgwire

The `database` field in the connection string must match the `name` of a configured data source, and the user must be assigned to it:

```bash
psql "postgresql://<user>:<password>@127.0.0.1:5434/<datasource-name>"
```

Data sources are configured via the Admin UI (`/datasources`) or REST API — not via env vars.

## Performance

### Idle Connection Timeout

pgwire 0.38 has no built-in idle timeout — `socket.next().await` blocks indefinitely after authentication. This prevents Fly.io `auto_stop_machines` from ever triggering when a GUI client like TablePlus is open, because the VM only stops when it has zero connections.

`proxy/src/server.rs` replaces pgwire's `process_socket` with a custom message loop (`process_socket_with_idle_timeout`) that adds a `tokio::select!` branch racing each `socket.next()` against a `sleep(idle_timeout)`. The timer resets after every received message — a running query does not count as idle.

Default timeout is 15 minutes (`BR_IDLE_TIMEOUT_SECS=900`). TCP keepalive (60 s time, 10 s interval) is also set on each accepted socket to detect dead connections from crashed clients or network failures.

When idle timeout fires, a log line is emitted at `INFO` level:
```
Idle connection timed out after 900s
```

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

### Performance Regression Testing

There is currently no automated performance regression suite. Micro-benchmarks
(Criterion) were evaluated but rejected: the hot-path functions they would cover
(AST rewrite, RLS filter injection, schema construction) are sub-microsecond
operations — regressions there are invisible against real query latency.

Meaningful regression detection requires integration-level tests against a real
Postgres instance that can verify filter pushdown is still active, connection pool
reuse is intact, and end-to-end query latency stays within bounds. This is planned
for a future iteration.

### Background Warmup

After authentication succeeds in `handler.rs`, a background task pre-builds the `SessionContext` (DB queries to load the stored catalog) and eagerly initialises the `LazyPool`. This amortises first-query latency during the window between the client's auth handshake and its first query.

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

Template variable substitution (`{user.tenant}`, etc.) uses parse-then-substitute: the filter expression is parsed into a `DataFusion Expr` tree first, then placeholder identifiers are replaced with typed `Expr::Literal` values. The user's tenant/username never passes through the SQL parser, preventing injection even if the value contains SQL syntax.

### Permissions Model

QueryProxy enforces a two-layer access control model:

**Management plane** — controlled by `is_admin` flag. Admins manage users, data sources, policies, and catalogs via the Admin API. Non-admins have no Admin API access.

**Data plane** — controlled by two independent mechanisms:
1. *Connection access* — `data_source_access` entries. A user can connect to a datasource via direct assignment, role membership (including inherited roles), or all-user scope. Being an admin does **not** automatically grant data plane access.
2. *Query policy* — `PolicyHook` applies row filters, column masks, and column access controls per-query based on assigned policies (direct, role-based, or all-scoped). If the datasource `access_mode` is `"policy_required"`, tables with no matching permit policy return empty results. Policies can reference built-in identity fields (`{user.tenant}`, `{user.username}`, `{user.id}`) and custom user attributes (`{user.KEY}`) for attribute-based access control (ABAC). Optional decision functions (JavaScript/WASM) provide programmable policy gates.

See `docs/permission-system.md` for the full policy system user guide.

Connection flow:
1. Client connects: `psql -d <datasource_name> -U <username>`
2. Proxy authenticates (Argon2id)
3. Proxy validates data source exists and is active
4. Proxy checks `data_source_access` — denied if no matching row (direct, role, or all scope)
5. Background task pre-warms `SessionContext` + pool
6. First query: fast path — context and pool already ready

## Admin REST API

Base: `http://localhost:5435/api/v1`

All endpoints require `Authorization: Bearer <token>` (obtained from `/auth/login`) except login itself. All IDs are UUIDs.

### Auth

| Method | Path | Description |
|--------|------|-------------|
| POST | `/auth/login` | Get JWT → `{ token, user }` |
| GET | `/auth/me` | Current user info |

### Users

| Method | Path | Description |
|--------|------|-------------|
| GET | `/users` | List users (paginated) |
| POST | `/users` | Create user |
| GET | `/users/{id}` | Get user |
| PUT | `/users/{id}` | Update user |
| DELETE | `/users/{id}` | Delete user |
| PUT | `/users/{id}/password` | Change password |

### Data Sources

| Method | Path | Description |
|--------|------|-------------|
| GET | `/datasource-types` | Supported types + field definitions |
| GET | `/datasources` | List data sources (paginated) |
| POST | `/datasources` | Create data source |
| GET | `/datasources/{id}` | Get data source |
| PUT | `/datasources/{id}` | Update data source |
| DELETE | `/datasources/{id}` | Delete data source |
| POST | `/datasources/{id}/test` | Test upstream connection |
| GET | `/datasources/{id}/users` | List assigned users |
| PUT | `/datasources/{id}/users` | Replace user assignments (user-scoped access) |
| PUT | `/datasources/{id}/access/roles` | Set role-based access `{ role_ids: [uuid] }` |

### Roles

| Method | Path | Description |
|--------|------|-------------|
| GET | `/roles` | List roles (paginated, searchable) |
| POST | `/roles` | Create role `{ name, description? }` |
| GET | `/roles/{id}` | Get role + members + inheritance + policy assignments |
| PUT | `/roles/{id}` | Update name/description/is_active |
| DELETE | `/roles/{id}` | Delete role → returns impact `{ affected_users, affected_assignments }` |
| GET | `/roles/{id}/effective-members` | All users inheriting policies (direct + inherited), with source |
| GET | `/roles/{id}/impact` | Preview impact of deleting this role |
| POST | `/roles/{id}/members` | Add members `{ user_ids: [uuid] }` |
| DELETE | `/roles/{id}/members/{user_id}` | Remove member |
| POST | `/roles/{id}/parents` | Add parent `{ parent_role_id }` (cycle detection + depth check) |
| DELETE | `/roles/{id}/parents/{parent_id}` | Remove parent |

### Catalog Discovery

Discovery is **async and non-blocking**. Every operation submits a job and returns immediately; progress is streamed via SSE.

| Method | Path | Description |
|--------|------|-------------|
| POST | `/datasources/{id}/discover` | Submit discovery job → `{ job_id }` (202) |
| GET | `/datasources/{id}/discover/{job_id}/events` | SSE stream of progress + result |
| GET | `/datasources/{id}/discover/{job_id}` | Poll job status |
| DELETE | `/datasources/{id}/discover/{job_id}` | Cancel running job |
| GET | `/datasources/{id}/catalog` | Read stored catalog (fast, no upstream) |

**Submit body** (`action` field selects the operation):

```json
{ "action": "discover_schemas" }

{ "action": "discover_tables", "schemas": ["public", "analytics"] }

{ "action": "discover_columns", "tables": [{"schema": "public", "table": "orders"}] }

{ "action": "save_catalog", "schemas": [{ "schema_name": "public", "is_selected": true,
    "tables": [{ "table_name": "orders", "table_type": "TABLE", "is_selected": true }] }] }

{ "action": "sync_catalog" }
```

**SSE event stream** (each line is `data: <json>`):

```
data: {"type":"progress","phase":"connecting","detail":"Connecting to upstream…"}
data: {"type":"progress","phase":"querying","detail":"Querying schemas…"}
data: {"type":"result","data":[{"schema_name":"public","is_already_selected":true}]}
data: {"type":"done"}
```

Only one discovery job may run per data source at a time — submitting a second returns `409 Conflict` with the active `job_id`.

### Policies

All policy endpoints require admin (`is_admin = true`).

| Method | Path | Description |
|--------|------|-------------|
| GET | `/policies` | List policies (paginated) |
| POST | `/policies` | Create policy |
| GET | `/policies/{id}` | Get policy + assignment count |
| PUT | `/policies/{id}` | Update policy (requires `version` for optimistic concurrency → 409 on conflict) |
| DELETE | `/policies/{id}` | Delete policy (cascades) |
| GET | `/policies/export` | Export all policies as YAML |
| POST | `/policies/import` | Import YAML (`?dry_run=true` to preview) |
| GET | `/datasources/{id}/policies` | List policy assignments for datasource |
| POST | `/datasources/{id}/policies` | Assign policy to datasource (scope: user/role/all) |
| DELETE | `/datasources/{id}/policies/{assignment_id}` | Remove assignment |

### Decision Functions

| Method | Path | Description |
|--------|------|-------------|
| GET | `/decision-functions` | List decision functions (paginated) |
| POST | `/decision-functions` | Create decision function |
| GET | `/decision-functions/{id}` | Get decision function |
| PUT | `/decision-functions/{id}` | Update decision function (optimistic concurrency) |
| DELETE | `/decision-functions/{id}` | Delete decision function |
| POST | `/decision-functions/{id}/test` | Test decision function with sample context |

### Attribute Definitions

| Method | Path | Description |
|--------|------|-------------|
| GET | `/attribute-definitions` | List definitions (`?entity_type=user` filter, paginated) |
| POST | `/attribute-definitions` | Create attribute definition |
| GET | `/attribute-definitions/{id}` | Get definition |
| PUT | `/attribute-definitions/{id}` | Update definition |
| DELETE | `/attribute-definitions/{id}` | Delete definition (`?force=true` to cascade-remove from entities) |

User attributes are set via `PUT /users/{id}` with an `attributes` field (full-replace semantics, validated against definitions). Attributes are available as `{user.KEY}` template variables in policy expressions and as `ctx.session.user.attributes` in decision functions.

### Audit Log

| Method | Path | Description |
|--------|------|-------------|
| GET | `/audit/queries` | Paginated query audit log (filter by user, datasource, date range, status) |
| GET | `/audit/admin` | Paginated admin audit log (filter by resource_type, resource_id, actor_id, date range) |

### Effective Policies

| Method | Path | Description |
|--------|------|-------------|
| GET | `/users/{id}/effective-policies?datasource_id=X` | All policies applying to user (with source annotation) |

## Catalog Workflow

Before a data source is queryable via pgwire, a catalog must be saved. The UI wizard guides through three steps:

1. **Discover schemas** — submit `discover_schemas`, watch SSE, select which schemas to include
2. **Discover tables** — submit `discover_tables` with selected schemas, select tables
3. **Discover columns** — submit `discover_columns` with selected tables; choose which columns to expose via a two-panel UI (scrollable table sidebar + column detail panel); unsupported Arrow types (e.g. JSONB, regclass) are shown greyed-out and cannot be selected
4. **Save** — submit `save_catalog` — persists schema/table/column selections to DB, invalidates engine cache

To detect schema drift after upstream changes, submit `sync_catalog`. The result is stored in `data_source.last_sync_result` and shown in the UI as a green/blue/amber panel.

The catalog is an **allowlist** — the proxy can never expose tables or columns not explicitly saved. `data_source.last_sync_result` only reports drift; the admin decides when to re-run the wizard.

## Data Model

All primary keys are UUIDs. The admin store uses SQLite by default (configurable via `DATABASE_URL`).

```
proxy_user         (id UUID, username, password_hash, tenant, is_admin, is_active, …)
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
                    execution_time_ms, client_ip, client_info, created_at)
```

Catalog entity IDs (schemas, tables, columns) are deterministic UUID v5 fingerprints derived from their natural keys. Re-discovering the same upstream object always produces the same ID, so re-syncs are safe upserts.

## Development

```bash
cargo build -p proxy          # compile
cargo test -p proxy           # run tests (213 unit tests + integration tests)
cd admin-ui && npm run build  # production UI bundle
```
