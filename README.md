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

After deploying, the Admin UI is available at `https://<app-name>.fly.dev`.

### Set required secrets (after deploy)

```sh
fly secrets set \
  BR_ENCRYPTION_KEY=$(openssl rand -hex 32) \
  BR_ADMIN_JWT_SECRET=$(openssl rand -hex 32) \
  BR_ADMIN_PASSWORD=<strong-password>
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
    ├─ Checks data source access (user_data_source table)
    ├─ Runs query hook pipeline (RLS, masking, …)
    └─ Executes via DataFusion + tokio-postgres federation
    ↓
Upstream PostgreSQL
```

## Tech Stack

| Layer | Library | Version |
|---|---|---|
| Protocol | pgwire | 0.38 |
| Query engine | DataFusion | 51 |
| PG federation | datafusion-table-providers | 0.9 |
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
├── migration/src/                    SeaORM migrations (4 total)
├── admin-ui/                         React admin console
│   └── src/
│       ├── api/                      axios + fetch-event-source clients
│       ├── auth/                     AuthContext, ProtectedRoute, LoginPage
│       ├── components/               Layout, DataSourceForm, CatalogDiscoveryWizard, …
│       ├── pages/                    Users*, DataSources*, DataSourceCatalogPage
│       └── types/                    TypeScript interfaces
└── proxy/src/
    ├── main.rs                       entry point: CLI, DB init, EngineCache, servers
    ├── handler.rs                    pgwire StartupHandler + query handlers
    ├── auth.rs                       Argon2 auth, user creation
    ├── crypto.rs                     AES-256-GCM encrypt/decrypt
    ├── admin/                        REST API: mod, dto, jwt, handlers, discovery_job
    ├── discovery/                    DiscoveryProvider trait + Postgres impl
    ├── entity/                       SeaORM entities (proxy_user, data_source, …)
    ├── engine/mod.rs                 EngineCache, VirtualCatalogProvider, build_arrow_schema()
    ├── hooks/                        QueryHook trait, RLS hook
    ├── sql_rewrite.rs                PG AST compatibility visitor
    └── arrow_conversion.rs           Arrow → pgwire batch encoding
```

## Quick Start

```bash
# 1. Start the proxy (auto-creates proxy_admin.db, seeds admin/admin)
cargo run -p proxy

# 2. Start the Admin UI (separate terminal)
cd admin-ui && npm run dev
# → http://localhost:5173  (login: admin / admin)

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
| `BR_CORS_ALLOWED_ORIGINS` | *(empty, same-origin only)* | Comma-separated list of allowed CORS origins for the Admin API |
| `RUST_LOG` | `info` | Log filter (standard Rust/tracing convention) |

## Connecting via pgwire

The `database` field in the connection string must match the `name` of a configured data source, and the user must be assigned to it:

```bash
psql "postgresql://<user>:<password>@127.0.0.1:5434/<datasource-name>"
```

Data sources are configured via the Admin UI (`/datasources`) or REST API — not via env vars.

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

### Background Warmup

After authentication succeeds in `handler.rs`, a background task pre-builds the `SessionContext` (DB queries to load the stored catalog) and eagerly initialises the `LazyPool`. This amortises first-query latency during the window between the client's auth handshake and its first query.

## Security

### Access Control Architecture

Access control is enforced **before** any query reaches the engine:

1. `validate_data_source()` — datasource must exist and be active
2. `check_access(user_id, datasource_name)` — user must have an explicit `user_data_source` row
3. If either check fails → `FATAL` PG error, connection rejected before `get_ctx()` is ever called

### Why the Shared Pool Is Safe

The upstream connection pool carries **no user identity** — it is pure TCP connectivity to the upstream Postgres server. All identity and access decisions are made at the pgwire auth layer (steps 1–2 above), not at the pool layer.

Per-user isolation is enforced by:
- **Data plane** — `user_data_source` allowlist (no row → connection rejected)
- **RLS hook** — per-query `WHERE tenant = '<value>'` filter injected via DataFusion's logical plan tree, based on the authenticated user's tenant metadata
- **Virtual catalog** — the stored catalog is an allowlist; tables/columns not explicitly saved are invisible to the engine

The shared pool is safe for all authorized users of a datasource: Pool = "how to talk to upstream". Auth + RLS = "what this user can see". These are orthogonal.

### RLS Bypass Resistance

The `RLSHook` uses AST-level table detection (`SystemTableVisitor`) — a query is only exempt from the tenant filter if its `FROM` clause references a schema-qualified system table (e.g. `pg_catalog.pg_class`). String literals like `WHERE name = 'pg_catalog'` do **not** exempt a query. The filter is injected below the `TableScan` node in the logical plan, not as a SQL string.

### Permissions Model

QueryProxy enforces a two-layer access control model:

**Management plane** — controlled by `is_admin` flag. Admins manage users, data sources, and catalogs via the Admin API. Non-admins have no Admin API access.

**Data plane** — controlled by explicit `user_data_source` assignments. A user can only connect to a data source with an explicit row in `user_data_source`. Being an admin does **not** automatically grant data plane access. Admins are auto-assigned to data sources they create.

Connection flow:
1. Client connects: `psql -d <datasource_name> -U <username>`
2. Proxy authenticates (Argon2id)
3. Proxy validates data source exists and is active
4. Proxy checks `user_data_source` — denied if no row
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
| PUT | `/datasources/{id}/users` | Replace user assignments |

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

## Catalog Workflow

Before a data source is queryable via pgwire, a catalog must be saved. The UI wizard guides through three steps:

1. **Discover schemas** — submit `discover_schemas`, watch SSE, select which schemas to include
2. **Discover tables** — submit `discover_tables` with selected schemas, select tables
3. **Discover columns** — submit `discover_columns` with selected tables, review column types
4. **Save** — submit `save_catalog` — persists selections to DB, invalidates engine cache

To detect schema drift after upstream changes, submit `sync_catalog`. The result is stored in `data_source.last_sync_result` and shown in the UI as a green/blue/amber panel.

The catalog is an **allowlist** — the proxy can never expose tables or columns not explicitly saved. `data_source.last_sync_result` only reports drift; the admin decides when to re-run the wizard.

## Data Model

All primary keys are UUIDs. The admin store uses SQLite by default (configurable via `DATABASE_URL`).

```
proxy_user (id UUID, username, password_hash, tenant, is_admin, is_active, …)
data_source (id UUID, name, ds_type, config JSON, secure_config encrypted, is_active,
             last_sync_at, last_sync_result, …)
user_data_source (id UUID, user_id → proxy_user, data_source_id → data_source)
discovered_schema (id UUID v5, data_source_id, schema_name, is_selected)
discovered_table  (id UUID v5, discovered_schema_id, table_name, table_type, is_selected)
discovered_column (id UUID v5, discovered_table_id, column_name, ordinal_position,
                   data_type, is_nullable, column_default, arrow_type)
```

Catalog entity IDs (schemas, tables, columns) are deterministic UUID v5 fingerprints derived from their natural keys. Re-discovering the same upstream object always produces the same ID, so re-syncs are safe upserts.

## Development

```bash
cargo build -p proxy          # compile
cargo test -p proxy           # run tests (84 unit tests)
cd admin-ui && npm run build  # production UI bundle
```
