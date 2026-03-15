# Proxy — Rust PostgreSQL Wire Protocol Proxy

## Key Versions
pgwire 0.38, DataFusion 52, axum 0.8, SeaORM 1, tokio-postgres 0.7, argon2 0.5, aes-gcm 0.10, jsonwebtoken 9, arrow-pg 0.12

## Key Files
- `src/server.rs` — `process_socket_with_idle_timeout()` (replaces pgwire's `process_socket`; adds idle + startup timeouts)
- `src/admin/mod.rs` — `AdminState`, `ApiErr`, `admin_router()`
- `src/admin/jwt.rs` — `AdminClaims` / `AuthClaims` extractors
- `src/admin/datasource_types.rs` — `split_config`, `merge_config`, `get_type_defs`
- `src/admin/discovery_job.rs` — `JobStore`, `DiscoveryJob`, `DiscoveryEvent`, `DiscoveryRequest`
- `src/engine/mod.rs` — `EngineCache`, `VirtualCatalogProvider`, `build_arrow_schema()`, `arrow_type_to_string()`
- `src/engine/rewrite.rs` — `rewrite_statement()` AST visitor (pg_catalog table qualification, schema-stripped function calls)
- `src/hooks/read_only.rs` — `ReadOnlyHook` (allowlist: Query, Show*, Explain*)
- `src/hooks/policy.rs` — `PolicyHook` (row filters, column masks, column access, audit logging)
- `src/admin/policy_handlers.rs` — policy CRUD + assignment endpoints
- `src/admin/audit_handlers.rs` — `GET /audit/queries`
- `src/admin/policy_yaml.rs` — YAML export/import
- `src/discovery/` — `DiscoveryProvider` trait + Postgres impl
- `src/crypto.rs` — AES-256-GCM `encrypt_json` / `decrypt_json`
- `../migration/src/lib.rs` — `Migrator` (7 migrations)

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

### `AdminState.policy_hook` is `Option<Arc<PolicyHook>>`
Policy CRUD handlers call `state.policy_hook.invalidate_datasource(&ds.name).await` (and `invalidate_user`) after mutations. It's `Option` so the admin server can be constructed without a hook in tests, but in production it is always `Some`.

### `AdminState.proxy_handler` is `Option<Arc<ProxyHandler>>`
Policy CRUD handlers also call `state.proxy_handler.rebuild_contexts_for_datasource(&ds.name)` after each policy mutation. This rebuilds the per-connection `SessionContext` for all active connections on that datasource in the background, so schema visibility changes (column deny/allow) take effect immediately without requiring a reconnect. It is `Option` so tests can construct `AdminState` with `proxy_handler: None`.

## Key Patterns
- **Always-stream response**: `handler.rs` always returns `Response::Query(encode_dataframe(...))` for every statement that reaches DataFusion — no `is_select` enumeration. The only exception is `Statement::Explain`, which is intercepted before streaming and reformatted into PostgreSQL's single `"QUERY PLAN"` column via `execute_explain()`. Arrow → pgwire encoding is handled by `arrow-pg`.
- **Hook ordering**: `ReadOnlyHook` runs first (blocks writes with SQLSTATE 25006), then `PolicyHook`. Hooks run in both simple and extended query paths. The allowlist in `ReadOnlyHook` must be reviewed before adding new `Statement` variants.
- `ApiErr` implements `IntoResponse` → JSON `{"error": "..."}` error bodies
- `AdminClaims` / `AuthClaims` use `FromRequestParts<S> where AdminState: FromRef<S>`; `AdminClaims` also checks `is_admin == true`
- Cache invalidation: `engine_cache.invalidate(name)` after catalog operations (keeps shared pool). `engine_cache.invalidate_all(name)` after datasource edit/delete (removes pool too). Never swap these — see README § Performance.
- Discovery jobs: one active job per datasource enforced by `JobStore.active_by_ds`; cancellation via `CancellationToken` passed through all `DiscoveryProvider` methods
- Catalog UUID v5 key format: `"{parent_uuid}:{child_name}"` — same natural key → same ID → re-discovery is a safe upsert
- Idle timeout: `process_socket_with_idle_timeout` in `server.rs` replaces `pgwire::tokio::process_socket`. Env var `BR_IDLE_TIMEOUT_SECS` (default 900). Tests use `tokio::time::pause()` + `advance()` — do not add real `sleep()` calls in server tests.

## PolicyHook

`PolicyHook` replaces the old hardcoded `RLSHook`. It loads policies from the DB, caches per `(datasource_id, username)` for 60 seconds, and applies three obligation types:

- **row_filter** — `Filter(expr)` node injected below the matching `TableScan` via `transform_up`. Template variables (`{user.tenant}`, `{user.username}`, `{user.id}`) are substituted as `Expr::Literal` after parsing — never interpolated as raw SQL.
- **column_mask** — replaces the column `Expr` in the top-level `Projection` with an aliased mask expression. Parsed synchronously via `sql_ast_to_df_expr(..., Some(ctx))` — sqlparser converts the mask template to a DataFusion `Expr` using the session's `FunctionRegistry` for built-in function lookup (RIGHT, LEFT, UPPER, LOWER, CONCAT, COALESCE, etc.). No standalone SQL plan is created.
- **column_access deny** — strips denied columns from the top-level `Projection`. Wildcards (`schema: "*"`, `table: "*"`) match any schema/table.

**Deny policies** short-circuit on the first match — query is rejected with a descriptive error before plan execution.

**access_mode**: If the datasource is `"policy_required"`, tables with no matching permit policy get `Filter(lit(false))` injected → empty results, no upstream round-trip.

**Cache invalidation**: call `policy_hook.invalidate_datasource(&name)` after any policy or datasource mutation. Call `policy_hook.invalidate_user(&user_id)` after user tenant/deactivation changes. Also call `proxy_handler.rebuild_contexts_for_datasource(&name)` after policy mutations so active connections immediately see the updated schema (column visibility changes without reconnect).

**Audit logging**: after each query, `PolicyHook` spawns a `tokio::spawn` task to insert a `query_audit_log` row asynchronously. The row captures `original_query`, `rewritten_query`, `policies_applied` (JSON with name+version snapshot), `client_ip`, and `client_info` (application_name from pgwire startup params).

## Testing

Data security and robustness are core product requirements. Every feature must ship with comprehensive unit and integration tests covering happy paths, edge cases, and security boundaries. Aim for best-in-class coverage — not just "it works", but "it cannot be bypassed".

### Unit tests (`src/**`)
Inline `#[cfg(test)]` modules in each source file. No external dependencies — run with `cargo test --lib`.

### Integration tests (`tests/`)
Two test binaries: `policy_enforcement.rs` (security/policy scenarios) and `protocol.rs` (pgwire protocol). Shared infrastructure in `tests/support/mod.rs`.

Run with `cargo test --test policy_enforcement` or `cargo test --test protocol`. Require Docker — skipped gracefully via `require_postgres!()` if unavailable.

**`ProxyTestServer::start()`** spins up a complete isolated stack per test:
- One shared `testcontainers` Postgres container per test binary (`OnceLock`) — not per-test.
- Fresh in-memory SQLite admin DB per test with all migrations applied.
- Real `ProxyHandler` on a random TCP port with a live pgwire accept loop.
- `axum_test::TestServer` wrapping the admin API for HTTP calls.

**Conventions:**
- Each test uses a unique upstream schema name (e.g. `"t1_rowfilt"`, `"tc_rf01"`) to avoid collisions during parallel execution.
- TC-prefixed tests map to security vector numbers in `docs/permission-security-tests.md`.
- Template vars in `filter_expression` must not be quoted: `tenant = {user.tenant}` ✓, `tenant = '{user.tenant}'` ✗.

### Documentation requirements (non-optional)
After completing any feature or adding tests, always update:
- **`docs/permission-security-tests.md`** — add a new vector entry for any new attack surface or bypass that was tested (Vector → Bug/Defense → Test format).
- **`docs/permission-system.md`** — keep the conceptual model, obligation descriptions, and examples in sync with the current implementation.

## Bug Fix Protocol
Use TDD: write the failing test(s) first to reproduce the bug, then fix the code until they pass. Never fix first and test after.

1. Write unit and integration tests that reproduce the bug and fail on the current code. Cover all relevant edge cases — add as many tests as needed, not just one of each.
2. Fix the code until the test passes.
3. Security-related bugs (policy bypass, access control, injection) MUST also be documented in `docs/permission-security-tests.md` following the existing Vector → Bug → Defense → Test format.

## Known Issues
- **regclass / regproc not supported** — `datafusion-table-providers` drops these columns. Catalog stores `arrow_type = NULL`; `build_arrow_schema` skips them.
- **json/jsonb wire type** — json/jsonb columns are announced as `TEXT` (arrow-pg maps `Utf8` → `Type::TEXT`) in the pgwire RowDescription. Data is correct; some GUI tools won't show a JSON-specific editor.
- **`->>` / `->` operator precedence** — sqlparser 0.59 gives `->>` lower precedence than `=`, so `col->>'key' = 'val'` is misparsed as `col ->> ('key' = 'val')`. In practice this is masked because the filter is pushed down to upstream PostgreSQL before DataFusion evaluates it. Visible in `EXPLAIN` output as a planning error. Workaround: add explicit parens `(col->>'key') = 'val'`. Will be fixed when DataFusion upgrades to sqlparser 0.60+.

## JSON / JSONB Support
- Both `json` and `jsonb` columns map to Arrow `Utf8` via `UnsupportedTypeAction::String` on the pool (set in both `discovery/postgres.rs` and `engine/mod.rs`).
- `datafusion-functions-json` is registered on every `SessionContext` via `register_all()` — provides `->`, `->>`, `?` operators and all JSON UDFs.
- `BetweenRowsPostgresDialect` in `engine/mod.rs` unparses JSON UDFs back to native PG operators for filter pushdown. Wire type is still `VARCHAR`.
- **Pushdown coverage**: `json_as_text`, `json_get_str`, `json_get`, `json_get_json`, `json_contains` are pushed down. Other UDFs (e.g. `json_length`, `json_keys`) are not — DataFusion evaluates them in-process after fetching the rows.
