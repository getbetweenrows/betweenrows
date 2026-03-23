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
- `src/hooks/policy.rs` — `PolicyHook` (five policy types: row_filter, column_mask, column_allow, column_deny, table_deny; audit logging)
- `src/admin/policy_handlers.rs` — policy CRUD + assignment endpoints (scope: user/role/all)
- `src/admin/role_handlers.rs` — role CRUD, member management, inheritance, datasource role access (GET+PUT), effective members; `get_role()` includes `datasource_access` (direct + inherited) and `policy_assignments` (direct + inherited)
- `src/admin/admin_audit.rs` — `AuditAction` enum, `AuditedTxn` transactional audit wrapper, `audit_log()` (internal)
- `src/admin/audit_handlers.rs` — `GET /audit/queries`, `GET /audit/admin`
- `src/role_resolver.rs` — BFS role resolution, cycle detection, depth check, effective assignments/access/policies/members
- `src/discovery/` — `DiscoveryProvider` trait + Postgres impl
- `src/crypto.rs` — AES-256-GCM `encrypt_json` / `decrypt_json`
- `../migration/src/lib.rs` — `Migrator` (41 migrations)

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

`PolicyHook` replaces the old hardcoded `RLSHook`. It loads policies from the DB, caches per `(datasource_id, username)` for 60 seconds, and applies five policy types:

- **row_filter** — `Filter(expr)` node injected below the matching `TableScan` via `transform_up`. Template variables (`{user.tenant}`, `{user.username}`, `{user.id}`) are substituted as `Expr::Literal` after parsing — never interpolated as raw SQL. Multiple `row_filter` policies are AND-combined (intersection, not union).
- **column_mask** — mask `Projection` injected above each matching `TableScan` via `apply_column_mask_at_scan` (`transform_up`). Replaces the masked column with the mask expression, aliased with `alias_qualified` to preserve the table qualifier. Parsed synchronously via `sql_ast_to_df_expr(..., Some(ctx))` — sqlparser converts the mask template to a DataFusion `Expr` using the session's `FunctionRegistry` for built-in function lookup (RIGHT, LEFT, UPPER, LOWER, CONCAT, COALESCE, etc.). Scan-level enforcement prevents CTE/subquery alias bypass. Masks are cleared from `column_masks` after scan-level application to prevent double-masking.
- **column_allow** — specifies which columns a user may see for matching tables. In `policy_required` mode, a `column_allow` policy is the only type that grants table access; without one, the table receives `Filter(lit(false))`.
- **column_deny** — enforced at two levels: (1) visibility-level via `compute_user_visibility` / `build_user_context` — denied columns removed from per-user schema at connect time; (2) defense-in-depth via top-level `Projection` in `apply_projection_qualified`. Does NOT short-circuit the query. If all selected columns are stripped, returns SQLSTATE `42501` (insufficient_privilege).
- **table_deny** — denied tables are removed from the catalog at connection time (404-not-403 principle). Queries fail with "table not found" rather than "access denied" to avoid leaking metadata about the existence of denied tables. Audit status is "error", not "denied".

**Policy type encodes effect**: `column_deny` and `table_deny` are deny types (`policy_type.is_deny() == true`); the others are permit types. There is no separate `effect` field.

**access_mode**: If the datasource is `"policy_required"`, tables with no matching `column_allow` policy get `Filter(lit(false))` injected → empty results, no upstream round-trip.

**Cache invalidation**: call `policy_hook.invalidate_datasource(&name)` after any policy or datasource mutation. Call `policy_hook.invalidate_user(user_id)` after user tenant/deactivation changes. Also call `proxy_handler.rebuild_contexts_for_datasource(&name)` after policy mutations so active connections immediately see the updated schema (column visibility changes without reconnect). For role changes, call `proxy_handler.rebuild_contexts_for_user(user_id)` for each affected user (resolved via `role_resolver::resolve_all_role_members`).

**Enforcement order in `apply_policies`**: (1) `apply_column_mask_at_scan` — mask Projection above TableScan, (2) `apply_row_filters` — Filter below mask Projection but above TableScan, (3) `apply_projection_qualified` — top-level Projection for allow/deny. Masks must run before filters so that `transform_up` places the Filter between TableScan and the mask Projection. This ensures row filters evaluate against raw (unmasked) data. Swapping this order is a security bug — see vector 32 in `docs/permission-security-tests.md`.

**Column-level policies must be enforced at scan level**: All column-level policies (deny, mask, and any future types) MUST be enforced at the `TableScan` level (visibility-level for deny, `transform_up` Projection for mask) to prevent CTE/subquery alias bypass. `SubqueryAlias` and CTE nodes change the DFSchema qualifier from the real table name to the alias, causing top-level-only matching to miss. Top-level `apply_projection_qualified` is defense-in-depth only.

**Audit logging**: after each query, `PolicyHook` spawns a `tokio::spawn` task to insert a `query_audit_log` row asynchronously. The row captures `original_query`, `rewritten_query`, `policies_applied` (JSON with name+version snapshot), `client_ip`, and `client_info` (application_name from pgwire startup params).

## RBAC (Role-Based Access Control)

`role_resolver.rs` contains all role resolution logic. Key functions:

- **`resolve_user_roles(db, user_id)`** — BFS from user's direct roles through `role_inheritance` to collect all ancestor active role IDs. Depth cap: 10. Skips inactive roles and their ancestors.
- **`resolve_effective_assignments(db, user_id, datasource_id)`** — returns all policy assignments matching the user: scope='all' OR (scope='user' AND user_id=X) OR (scope='role' AND role_id in user_roles). Deduplicates by policy_id, keeping lowest priority.
- **`resolve_datasource_access(db, user_id, datasource_id)`** — checks `data_source_access` for same three-scope pattern. Used by `check_access()` in `engine/mod.rs`.
- **`resolve_all_role_members(db, role_id)`** — BFS downward from role through child roles to collect all member user IDs. Used for cache invalidation.
- **`resolve_effective_members(db, role_id)`** — BFS downward, returns `EffectiveMemberEntry { user_id, username, source }` with "direct" or "via role '<child_name>'" source annotation. Source indicates which role the user is a *direct member of*, not the role being viewed.
- **`detect_cycle(db, parent_id, child_id)`** — BFS from proposed parent upward; returns true if child is reachable (would create cycle).

**`data_source_access`** replaces `user_data_source`. Supports scopes: user (direct user access), role (role-based access), all (everyone). The `user_data_source` table is dropped in migration 037.

**`policy_assignment`** now has `role_id: Option<Uuid>` and `assignment_scope: String` ("user"/"role"/"all"). The backfill migration 033 sets scope='all' where user_id IS NULL.

**Admin audit log** (`admin_audit_log` table) is append-only — no UPDATE/DELETE endpoints.

### Admin Audit Patterns
- **Always use `AuditedTxn`** (from `admin_audit.rs`) for handlers that mutate entities. It wraps a `DatabaseTransaction`, queues audit entries via `txn.audit(...)`, and writes them atomically on `txn.commit()`. This makes the correct pattern (audit inside the transaction) the only pattern.
- **`AuditedTxn::commit()` errors if no audit entries were queued** — prevents accidentally unaudited transactions. Use a plain `DatabaseTransaction` if you genuinely don't need audit (e.g., `create_datasource` auto-assign).
- **`audit_log()` is `pub(crate)`** — used internally by `AuditedTxn::commit()`. Handlers should not call it directly.
- **`audit_delete` / `audit_insert` have been removed** — replaced by direct entity operations + `txn.audit(...)`.
- Convention: log on the owning entity (role membership → role, policy assignment → policy).
- **Cache invalidation after commit**: always invalidate caches *after* `txn.commit()`, not before or inside the transaction. Collect affected user IDs before the transaction if needed (e.g., `remove_parent` collects members before removing the inheritance edge).

**Role handler cache invalidation pattern**:
- Member add/remove → `invalidate_user(affected_user_id)` + `rebuild_contexts_for_user(user_id)`
- Inheritance add/remove → `resolve_all_role_members` on child subtree, invalidate each
- Role deactivate/reactivate/delete → same as inheritance (all affected members)

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
- **`docs/permission-system.md`** — keep the conceptual model, policy type descriptions, and examples in sync with the current implementation.

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
