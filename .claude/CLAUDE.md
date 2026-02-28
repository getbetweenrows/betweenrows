# Project Instructions

## Use Context7 by Default
Always use context7 when I need code generation, setup or configuration steps, or library/API documentation. Automatically use the Context7 MCP tools to resolve library id and get library docs without me having to explicitly ask.

## Project Overview
QueryProxy is a Rust PostgreSQL wire protocol proxy. See `README.md` for full details.

Key versions: pgwire 0.38, DataFusion 51, axum 0.8, SeaORM 1, tokio-postgres 0.7, argon2 0.5, aes-gcm 0.10, jsonwebtoken 9. Admin UI: React 19, Vite 6, Tailwind 4, TanStack Query 5, react-router-dom 7.

## Key Files
- `proxy/src/admin/mod.rs` — `AdminState`, `ApiErr`, `admin_router()`
- `proxy/src/admin/jwt.rs` — `AdminClaims` / `AuthClaims` extractors
- `proxy/src/admin/datasource_types.rs` — `split_config`, `merge_config`, `get_type_defs`
- `proxy/src/admin/discovery_job.rs` — `JobStore`, `DiscoveryJob`, `DiscoveryEvent`, `DiscoveryRequest`
- `proxy/src/engine/mod.rs` — `EngineCache`, `VirtualCatalogProvider`, `build_arrow_schema()`, `arrow_type_to_string()`
- `proxy/src/hooks/read_only.rs` — `ReadOnlyHook` (allowlist: Query, Show*, Explain*)
- `proxy/src/hooks/rls.rs` — `RLSHook`
- `proxy/src/discovery/` — `DiscoveryProvider` trait + Postgres impl
- `proxy/src/crypto.rs` — AES-256-GCM `encrypt_json` / `decrypt_json`
- `migration/src/lib.rs` — `Migrator` (4 migrations)
- `admin-ui/vite.config.ts` — proxies `/api` → `http://localhost:5435`

## Critical Gotchas

### Axum 0.8 Path Params
Use `{id}` **not** `:id` in route definitions. `:id` panics at runtime: "Path segments must not start with `:`".

### PostgresConnectionPool Key Names (datafusion-table-providers)
The pool reads `"db"` (not `"dbname"`) and `"pass"` (not `"password"`). Wrong keys are silently dropped → connection fails. Correct keys: `"host"`, `"user"`, `"db"`, `"pass"`, `"port"`, `"sslmode"`. See `build_postgres_params()` in `engine/mod.rs`.

### `Vec<Box<dyn ToSql + Sync>>` is not `Send`
Use `Vec<String>` and cast inline to `&(dyn ToSql + Sync)` refs — these refs are `Send` because `dyn ToSql + Sync: Sync`.

### `QueryHook::handle_query` signature
Must use `&(dyn ClientInfo + Sync)` (not `&dyn ClientInfo`) for the async future to be `Send`.

### All PKs are UUIDs — never i32
Handlers use `Path<Uuid>`. UUID v7 for operational entities (`Uuid::now_v7()`); UUID v5 for catalog entities (deterministic via `CATALOG_NS` in `catalog_handlers.rs`).

### Arrow type serialization
Always get Arrow types from the library's `get_schema()` during discovery — this guarantees the stored type matches what the library produces at query time. To persist a `DataType` to the DB call `arrow_type_to_string(&dt)` from `engine/mod.rs`; to read it back call `parse_arrow_type(s)`. Never hand-write Arrow type strings, and never build a manual PG-name → `DataType` mapping — a mismatch between stored and runtime types silently causes a full schema-cast on every result batch, which adds 10–20 s to large queries.

### `AdminState` carries `job_store`, not `discovery_locks`
`job_store: Arc<Mutex<discovery_job::JobStore>>` replaced the old `discovery_locks: Arc<Mutex<HashSet<i32>>>`.

## Key Patterns
- **Hook ordering**: `ReadOnlyHook` runs first (blocks writes with SQLSTATE 25006), then `RLSHook`. Hooks run in both simple and extended query paths. The allowlist in `ReadOnlyHook` must be reviewed before adding new `Statement` variants.
- `ApiErr` implements `IntoResponse` → JSON `{"error": "..."}` error bodies
- `AdminClaims` / `AuthClaims` use `FromRequestParts<S> where AdminState: FromRef<S>`; `AdminClaims` also checks `is_admin == true`
- Cache invalidation: `engine_cache.invalidate(name)` after catalog operations (keeps shared pool). `engine_cache.invalidate_all(name)` after datasource edit/delete (removes pool too). Never swap these — see README § Performance.
- Discovery jobs: one active job per datasource enforced by `JobStore.active_by_ds`; cancellation via `CancellationToken` passed through all `DiscoveryProvider` methods
- Catalog UUID v5 key format: `"{parent_uuid}:{child_name}"` — same natural key → same ID → re-discovery is a safe upsert

## Pre-commit Hook
A `.githooks/pre-commit` script runs `cargo fmt --check` and `cargo clippy -p proxy -- -D warnings`. Enable it once per clone:
```bash
git config core.hooksPath .githooks
```

## Known Issues
- **JSONB / regclass / regproc not supported** — `datafusion-table-providers` silently drops these columns (`UnsupportedTypeAction::Warn`). Catalog stores `arrow_type = NULL`; `build_arrow_schema` skips them.
