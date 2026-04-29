# Proxy — Rust PostgreSQL Wire Protocol Proxy

## Key Versions
pgwire 0.38, DataFusion 52, axum 0.8, SeaORM 1, tokio-postgres 0.7, argon2 0.5, aes-gcm 0.10, jsonwebtoken 9, arrow-pg 0.12

## Key Files
- `src/server.rs` — `process_socket_with_idle_timeout()` (replaces pgwire's `process_socket`; adds idle + startup timeouts)
- `src/admin/mod.rs` — `AdminState`, `ApiErr`, `admin_router()`
- `src/admin/jwt.rs` — `AdminClaims` / `AuthClaims` extractors
- `src/admin/datasource_types.rs` — `split_config`, `merge_config`, `get_type_defs`
- `src/admin/discovery_job.rs` — `JobStore`, `DiscoveryJob`, `DiscoveryEvent`, `DiscoveryRequest`
- `src/engine/mod.rs` — `EngineCache` (uses shared `Arc<WasmDecisionRuntime>` for visibility-level decision fn evaluation), `VirtualCatalogProvider`, `build_arrow_schema()`, `arrow_type_to_string()`, `ScanFilterProjectionFixRule` (ensures pushed-down filter columns are in scan projection), `EmptyProjectionFixRule` (prevents zero-column scan projections that cause schema mismatch with `SqlExec`)
- `src/engine/rewrite.rs` — `rewrite_statement()` AST visitor (pg_catalog table qualification, schema-stripped function calls)
- `src/hooks/read_only.rs` — `ReadOnlyHook` (allowlist: Query, Show*, Explain*)
- `src/hooks/policy.rs` — `PolicyHook` (five policy types: row_filter, column_mask, column_allow, column_deny, table_deny; audit logging). Also owns transitive column resolution for row filters via `RelationshipSnapshot` + `precompute_parent_scans`.
- `src/resolution/mod.rs`, `src/resolution/graph.rs` — column-anchor resolution engine. `build_column_resolution_plan()` walks admin-curated `table_relationship` + `column_anchor` chains (max depth 3) and rewrites a `TableScan` into `Project([target.*], Filter(rewritten, InnerJoin(target, parent_chain)))` when a row filter references a column that lives on a parent table. `RelationshipSnapshot` carries the per-datasource edges + anchors + column lists loaded once per session. Five failure modes (no anchor / depth exceeded / cycle / qualified parent ref / column missing from scan schema) all produce deny-wins via `lit(false)` with a structured `column_resolution_unresolved` warn log.
- `src/admin/policy_handlers.rs` — policy CRUD + assignment endpoints (scope: user/role/all). Also exposes `get_policy_anchor_coverage` (`GET /policies/{id}/anchor-coverage`): edit-time dry-run that, for each (assigned table × column referenced in the row-filter expression), reuses `RelationshipSnapshot` + `expr_column_names` to classify per-pair verdicts (on_table / anchor_walk / anchor_alias / missing_anchor / missing_column_on_alias_target). Each entry carries `schema` (effective name — alias if set, else upstream — what the proxy keys columns by at query time) **and** `schema_upstream` (raw upstream name) so the admin UI can render `alias (upstream)` consistently. Surfaces silent-deny cases before users notice empty result sets. Returns empty coverage for non-row-filter policy types.
- `src/admin/relationship_handlers.rs` — admin CRUD for `table_relationship` + `column_anchor` and the live `GET /datasources/{id}/fk-suggestions` endpoint (filters `pg_constraint` results to single-column FKs whose parent column is a PK or single-column unique; parent-column PK/unique is **not** re-verified on the `POST /datasources/{id}/relationships` path — see vector 73 Status for the accepted trade-off).
- `src/entity/table_relationship.rs`, `src/entity/column_anchor.rs` — SeaORM entities for the two admin-curated catalogs backing transitive resolution. `column_anchor` has a DB-enforced partial unique index on `(datasource, child_table, resolved_column_name)` so exactly one anchor routes any (table, column) pair.
- `src/admin/datasource_handlers.rs` — datasource CRUD endpoints (create, update, delete, test, user access, role access)
- `src/admin/user_handlers.rs` — user CRUD endpoints (create, update, delete, change_password)
- `src/admin/role_handlers.rs` — role CRUD, member management, inheritance, datasource role access (GET+PUT), effective members; `get_role()` includes `datasource_access` (direct + inherited) and `policy_assignments` (direct + inherited)
- `src/admin/admin_audit.rs` — `AuditAction` enum, `AuditedTxn` transactional audit wrapper, `audit_log()` (internal)
- `src/admin/audit_handlers.rs` — `GET /audit/queries`, `GET /audit/admin`
- `src/role_resolver.rs` — BFS role resolution, cycle detection, depth check, effective assignments/access/policies/members
- `src/discovery/` — `DiscoveryProvider` trait + Postgres impl. Trait methods: `discover_schemas`, `discover_tables`, `discover_columns`, `discover_foreign_keys` (live `pg_constraint` introspection fueling the admin UI's FK-suggestion picker; nothing persisted).
- `src/decision/` — WASM-based decision function runtime (`mod.rs`: `DecisionRuntime` trait + `DecisionResult`; `context.rs`: `SessionInfo`, `QueryMetadata`, `build_session_context`, `build_query_context`; `wasm.rs`: `WasmDecisionRuntime` backed by wasmtime + Javy dynamic mode — QuickJS plugin compiled once at startup, per-function bytecode ~1ms)
- `src/entity/decision_function.rs` — SeaORM entity for the `decision_function` table (id, name, decision_fn JS source, decision_wasm bytes, decision_config JSON, evaluate_context, on_error, log_level, is_enabled, version)
- `src/admin/decision_function_handlers.rs` — CRUD endpoints for decision functions (`GET/POST /decision-functions`, `GET/PUT/DELETE /decision-functions/{id}`)
- `src/entity/attribute_definition.rs` — SeaORM entity for the `attribute_definition` table (id, key, entity_type, display_name, value_type, default_value, allowed_values, description). Includes `validate_value()` and `parse_allowed_values()`.
- `src/admin/attribute_definition_handlers.rs` — CRUD endpoints for attribute definitions (`GET/POST /attribute-definitions`, `GET/PUT/DELETE /attribute-definitions/{id}`). Supports `?entity_type=user` filter and `?force=true` cascade delete via database-specific JSON operations (SQLite `json_remove()`, PostgreSQL `jsonb -`). Includes `validate_json_path_key()` defense-in-depth before SQL interpolation. Update handler invalidates caches when `value_type` changes.
- `src/crypto.rs` — AES-256-GCM `encrypt_json` / `decrypt_json`
- `../migration/src/lib.rs` — `Migrator`

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

### `AdminState.wasm_runtime` is `Arc<WasmDecisionRuntime>` (non-Option)
A single shared WASM runtime created once at startup in `main.rs` and passed to `EngineCache`, `PolicyHook`, and `AdminState`. Unlike `policy_hook` and `proxy_handler`, it is always required because `EngineCache` needs it unconditionally. Tests must provide a real instance (use `OnceLock` singletons to avoid re-creating per test).

### wasmtime `Module::new()` is a blocking JIT compile
`Module::new(&engine, bytes)` compiles WASM to native code (~1ms for small bytecode). Always call inside `spawn_blocking` — never on the async tokio runtime thread. See `WasmDecisionRuntime::evaluate_bytes()` in `decision/wasm.rs`.

## Key Patterns
- **Always-stream response**: `handler.rs` always returns `Response::Query(encode_dataframe(...))` for every statement that reaches DataFusion — no `is_select` enumeration. The only exception is `Statement::Explain`, which is intercepted before streaming and reformatted into PostgreSQL's single `"QUERY PLAN"` column via `execute_explain()`. Arrow → pgwire encoding is handled by `arrow-pg`.
- **Hook ordering**: `PolicyHook` runs first (audits write rejections, then returns `None`), then `ReadOnlyHook` (blocks writes with SQLSTATE 25006). Hooks run in both simple and extended query paths. The allowlist in `ReadOnlyHook` must be reviewed before adding new `Statement` variants.
- `ApiErr` implements `IntoResponse` → JSON `{"error": "..."}` error bodies
- `AdminClaims` / `AuthClaims` use `FromRequestParts<S> where AdminState: FromRef<S>`; `AdminClaims` also checks `is_admin == true`
- Cache invalidation: `engine_cache.invalidate(name)` after catalog operations (keeps shared pool). `engine_cache.invalidate_all(name)` after datasource edit/delete (removes pool too). Never swap these — see CONTRIBUTING.md § Performance.
- Discovery jobs: one active job per datasource enforced by `JobStore.active_by_ds`; cancellation via `CancellationToken` passed through all `DiscoveryProvider` methods
- Catalog UUID v5 key format: `"{parent_uuid}:{child_name}"` — same natural key → same ID → re-discovery is a safe upsert
- Idle timeout: `process_socket_with_idle_timeout` in `server.rs` replaces `pgwire::tokio::process_socket`. Env var `BR_IDLE_TIMEOUT_SECS` (default 900). Tests use `tokio::time::pause()` + `advance()` — do not add real `sleep()` calls in server tests.

## PolicyHook

`PolicyHook` replaces the old hardcoded `RLSHook`. It loads policies from the DB, caches per `(datasource_id, username)` for 60 seconds, and applies five policy types:

- **row_filter** — `Filter(expr)` node injected below the matching `TableScan` via `transform_up`. Two built-in template variables (`{user.username}`, `{user.id}`) plus custom user attributes (`{user.KEY}`, e.g., `{user.tenant}`) are substituted as typed `Expr::Literal` after parsing — never interpolated as raw SQL. Custom attributes produce typed literals based on their attribute definition's `value_type` (string→Utf8, integer→Int64, boolean→Boolean). Multiple `row_filter` policies are AND-combined (intersection, not union). When a filter references a column that isn't on the matched table, the rewriter consults the per-session `RelationshipSnapshot` (loaded once in `load_session` from `table_relationship` + `column_anchor`) and — if an anchor exists — resolves the reference in one of two shapes: **FK walk** (`AnchorShape::Relationship`) replaces the scan subtree with `Project([target.*], Filter(rewritten, InnerJoin(target, parent_chain)))`; **same-table alias** (`AnchorShape::Alias`) rewrites the filter expression inline to use the anchor's `actual_column_name` with no join. Exactly one shape per `(child_table, resolved_column)` — XOR enforced at the API. Parent `LogicalPlan`s for the FK-walk shape are pre-planned in `precompute_parent_scans` before `apply_row_filters` runs (since `transform_up` is synchronous) and cached on `SessionData.parent_scans_cache` for the session's lifetime. Any resolution failure (missing anchor, depth > 3, cycle, qualified parent ref, column missing from scan schema — including alias anchors pointing at a non-existent column) substitutes `Filter(lit(false))` with a `column_resolution_unresolved` warn log — never a DataFusion plan error.
- **column_mask** — mask `Projection` injected above each matching `TableScan` via `apply_column_mask_at_scan` (`transform_up`). Replaces the masked column with the mask expression, aliased with `alias_qualified` to preserve the table qualifier. Parsed synchronously via `sql_ast_to_df_expr(..., Some(ctx))` — sqlparser converts the mask template to a DataFusion `Expr` using the session's `FunctionRegistry` for built-in function lookup (RIGHT, LEFT, UPPER, LOWER, CONCAT, COALESCE, etc.). Scan-level enforcement prevents CTE/subquery alias bypass. Masks are cleared from `column_masks` after scan-level application to prevent double-masking.
- **column_allow** — specifies which columns a user may see for matching tables. In `policy_required` mode, a `column_allow` policy is the only type that grants table access; without one, the table receives `Filter(lit(false))`.
- **column_deny** — enforced at two levels: (1) visibility-level via `compute_user_visibility` / `build_user_context` — denied columns removed from per-user schema at connect time; (2) defense-in-depth via top-level `Projection` in `apply_projection_qualified`. Does NOT short-circuit the query. If all selected columns are stripped, returns SQLSTATE `42501` (insufficient_privilege).
- **table_deny** — denied tables are removed from the catalog at connection time (404-not-403 principle). Queries fail with "table not found" rather than "access denied" to avoid leaking metadata about the existence of denied tables. Audit status is "error", not "denied".

**Policy type encodes effect**: `column_deny` and `table_deny` are deny types (`policy_type.is_deny() == true`); the others are permit types. There is no separate `effect` field.

**access_mode**: If the datasource is `"policy_required"`, tables with no matching `column_allow` policy get `Filter(lit(false))` injected → empty results, no upstream round-trip.

**Cache invalidation**: call `policy_hook.invalidate_datasource(&name)` after any policy or datasource mutation. Call `policy_hook.invalidate_user(user_id)` after user attribute/deactivation changes. Also call `proxy_handler.rebuild_contexts_for_datasource(&name)` after policy mutations so active connections immediately see the updated schema (column visibility changes without reconnect). For role changes, call `proxy_handler.rebuild_contexts_for_user(user_id)` for each affected user (resolved via `role_resolver::resolve_all_role_members`).

**Enforcement order in `apply_policies`**: (1) `apply_column_mask_at_scan` — mask Projection above TableScan, (2) `apply_row_filters` — Filter below mask Projection but above TableScan, (3) `apply_projection_qualified` — top-level Projection for allow/deny. Masks must run before filters so that `transform_up` places the Filter between TableScan and the mask Projection. This ensures row filters evaluate against raw (unmasked) data. Swapping this order is a security bug — see vector 32 in `docs/security-vectors.md`.

**Column-level policies must be enforced at scan level**: All column-level policies (deny, mask, and any future types) MUST be enforced at the `TableScan` level (visibility-level for deny, `transform_up` Projection for mask) to prevent CTE/subquery alias bypass. `SubqueryAlias` and CTE nodes change the DFSchema qualifier from the real table name to the alias, causing top-level-only matching to miss. Top-level `apply_projection_qualified` is defense-in-depth only.

**Audit logging**: after each query, `PolicyHook` spawns a `tokio::spawn` task to insert a `query_audit_log` row asynchronously. The row captures `original_query`, `rewritten_query`, `policies_applied` (JSON with name+version snapshot including decision function results), and `client_info` (application_name from pgwire startup params).

### Decision Functions in PolicyHook

A single `Arc<WasmDecisionRuntime>` is created once at startup in `main.rs` and shared by `PolicyHook`, `EngineCache`, and `AdminState`. The runtime holds a `wasmtime::Engine` and a pre-compiled QuickJS plugin module (~869 KB). Decision functions are compiled in **Javy dynamic mode** — the JS source produces a small bytecode WASM (1-16 KB) that imports the QuickJS engine from the plugin module at runtime. This two-module linking approach compiles in ~1ms per function vs ~1-2s for the old static mode.

**`evaluate_decision_fn` flow** (called once per policy in `PolicyEffects::collect`):

1. If the policy has no `decision_function_id` → always fires (return `true`).
2. If the resolved `DecisionFunction` has `is_enabled = false` → always fires (gate disabled).
3. If `decision_wasm` is `None` or empty → always fires (not compiled yet).
4. If `evaluate_context = "query"` but the `DecisionEvalContext` has no `"query"` key → always fires (defensive fallback — in practice query context is always present at query time).
5. Call `wasm_runtime.evaluate_bytes(wasm_bytes, ctx, config, fuel_limit, log_level)` — this compiles the bytecode module (~1ms), spawns a blocking thread, instantiates the plugin, links the bytecode module, and runs it.
6. On success: cache the `DecisionResult`, return `result.fire`.
7. On error: log with `tracing::error!`, set `fire = (on_error == "deny")`, cache the error result, return `fire`.

**`DecisionEvalContext`** is built in `handle_query` and passed into `PolicyEffects::collect`. It carries:
- `decision_ctx: serde_json::Value` — the full context JSON (session or session+query depending on policies present)
- `wasm_runtime: &WasmDecisionRuntime` — shared WASM runtime reference

**`QueryMetadata` extraction** happens after DataFusion plans the query. The `LogicalPlan` is walked to extract `tables` (from `TableScan` nodes, filtering system schemas), `columns` (from the top-level output schema fields), `join_count` (from `Join` nodes), `has_aggregation` (from `Aggregate` nodes), `has_subquery` (from `SubqueryAlias` nodes), `has_where` (from `Filter` nodes), and `statement_type` (hardcoded `"SELECT"` since only SELECT reaches PolicyHook). This data populates the `"query"` key in the context JSON for `evaluate_context = "query"` functions.

**Module compilation** uses the `wasm_bytes` stored in the `decision_function` entity (loaded from DB during `load_session`). The stored bytes are dynamic-mode bytecode (1-16 KB), compiled at save time via `javy build -C dynamic`. At query time, `evaluate_decision_fn` compiles the bytecode module (~1ms) and links it with the pre-compiled plugin module. No module cache is needed since bytecode compilation is fast. On decision function update, `invalidate_for_decision_function()` clears the `SessionData` cache, so the next query reloads fresh `wasm_bytes` from DB.

**JS harness**: User JS is wrapped in a strict-mode IIFE that prevents global variable leaks. The IIFE validates that the user code defines an `evaluate(ctx, config)` function, then the harness reads stdin JSON, calls `evaluate()`, validates the result shape, and writes to stdout. Note: Javy 8.x routes `console.log` to stdout (fd 1) by default — `parse_stdout_result()` in `wasm.rs` handles this by extracting the JSON result from the last stdout line and treating preceding lines as log output.

**Visibility-level decision function evaluation**: `EngineCache` uses the same shared `Arc<WasmDecisionRuntime>` for evaluating decision functions at connect time in `compute_user_visibility()`. Policies with `affects_visibility() == true` (column_allow, column_deny, table_deny) have their decision functions evaluated via `evaluate_visibility_decision_fn()`. If `evaluate_context = "session"` and `fire: false`, the policy is skipped (column/table stays visible). If `evaluate_context = "query"`, the visibility effect is skipped entirely (deferred to query time — column/table stays visible in schema, enforcement happens at query time via `PolicyEffects::collect`). Error handling uses `on_error`: `"deny"` → apply, `"skip"` → skip.

**Startup recompilation**: After migrations run, `recompile_decision_functions()` in `main.rs` queries for any decision functions with `decision_fn IS NOT NULL AND decision_wasm IS NULL` and recompiles them in dynamic mode. This ensures all functions are ready after deployment without manual re-save.

## User Attributes (ABAC)

Custom key-value attributes on users, governed by a schema-first attribute definition system.

**Storage**: JSON column `attributes TEXT DEFAULT '{}'` on `proxy_user`. Loads for free with the user model — zero extra queries on the hot path.

**Attribute definitions**: `attribute_definition` table defines allowed keys with types. One row per `(key, entity_type)` pair. `UNIQUE(key, entity_type)` index. For now only `entity_type = "user"` is wired up; `"table"` and `"column"` are accepted but not used in policy evaluation yet. Reserved keys for users: `username`, `id`, `user_id`, `roles` — rejected at the API.

**Value types**: `"string"` (→ Utf8 literal), `"integer"` (→ Int64), `"boolean"` (→ Boolean), `"list"` (→ list of strings, max 100 elements). Type-safe substitution in both template variables and decision function context. List attributes expand into multiple comma-separated string literals in `mangle_vars()` for use with `IN` clauses: `department IN ({user.departments})`. Empty lists expand to a NULL sentinel (effectively false). `parse_attributes()` returns `HashMap<String, serde_json::Value>` (not `HashMap<String, String>`).

**Namespace design**: Attributes are nested under `attributes` in the API (`PUT /users/{id}` payload and response) but flat in expressions (`{user.KEY}`) and decision context (`ctx.session.user.KEY`). This is intentional — API nesting separates user-defined from built-in fields; expression flattening keeps policy authoring concise. Reserved key validation prevents collisions. See `docs/permission-system.md` § "Namespace design" for the full rationale.

**Template variables**: `{user.KEY}` in filter/mask expressions. Built-in fields (`{user.username}`, `{user.id}`) take priority via `match` arms in `UserVars::get()`, preventing attribute override attacks. Custom attributes (including `tenant`) fall through to the resolved attribute map (user values + definition defaults via `resolve_user_attribute_defaults()`).

**Missing attribute resolution**: When a user lacks an attribute referenced by a policy, the proxy resolves it from the attribute definition's `default_value`. If a non-NULL default is set, it is used as a typed literal. If the default is NULL (the default), SQL `NULL` is substituted (comparisons with NULL evaluate to NULL → treated as false in WHERE → zero rows). If no definition exists, the query errors. This is handled centrally by `resolve_user_attribute_defaults()` in `hooks/policy.rs`, used in all three paths: template variables, query-level decision context, and visibility-level decision context (`build_typed_json_attributes` in `engine/mod.rs`).

**Decision function context**: Custom attributes are flattened as first-class fields on `ctx.session.user` (e.g., `ctx.session.user.region`, `ctx.session.user.tenant`) with typed JSON values. Missing attributes with a `default_value` appear as their typed default; missing attributes whose default is NULL appear as `null` (not `undefined`). Built-in fields (`id`, `username`, `roles`) always take priority. `ctx.session.time.now` is an ISO 8601 / RFC 3339 timestamp of the evaluation time (not session start).

**Save-time expression validation**: `validate_expression()` in `hooks/policy.rs` dry-run parses filter/mask expressions at policy create/update time and returns 422 if the syntax is unsupported. Called from `validate_definition()` in `dto.rs`.

**API endpoints**: `GET/POST /attribute-definitions`, `GET/PUT/DELETE /attribute-definitions/{id}`. User attributes are set via `PUT /users/{id}` with an `attributes` field (full-replace semantics, validated against definitions). DELETE supports `?force=true` for cascade cleanup via database-specific JSON operations (SQLite `json_remove()`, PostgreSQL `jsonb -`).

**Cache invalidation**: attribute changes trigger `policy_hook.invalidate_user()` + `proxy_handler.rebuild_contexts_for_user()`. This fires on attribute changes and is_active changes in `update_user`. Attribute definition `value_type` or `default_value` changes also trigger cache invalidation for all users with that attribute.

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
- **`AuditedTxn::commit()` errors if no audit entries were queued** — prevents accidentally unaudited transactions.
- **`audit_log()` is `pub(crate)`** — used internally by `AuditedTxn::commit()`. Handlers should not call it directly.
- Convention: log on the owning entity (role membership → role, policy assignment → policy).
- **Cache invalidation after commit**: always invalidate caches *after* `txn.commit()`, not before or inside the transaction. Collect affected user IDs before the transaction if needed (e.g., `remove_parent` collects members before removing the inheritance edge).

### Audit Changes JSON Conventions

All mutation handlers must follow these standardized conventions for the `changes` JSON in `txn.audit(...)`:

| Action | JSON shape | What to capture |
|---|---|---|
| **Create** | `{"after": {all fields}}` | Full snapshot of the new entity (excluding secrets) |
| **Update** | `{"before": {changed}, "after": {changed}}` | Only fields that changed, using `serde_json::Map` — matches `update_role` pattern |
| **Delete** | `{"before": {all fields}}` | Full snapshot of the entity being deleted (excluding secrets) |
| **Assign/Unassign** | `{relationship fields}` | Flat JSON with identifiers (assignment_id, datasource_id, scope, user_id, role_id) |

**Secrets rule**: Never log `config`, `secure_config`, `password_hash`, or `decision_fn` source code in audit entries. For update audits where these change, use boolean flags like `"config_changed": true` or `"field": "password"` instead of logging the actual values.

**Role handler cache invalidation pattern**:
- Member add/remove → `invalidate_user(affected_user_id)` + `rebuild_contexts_for_user(user_id)`
- Inheritance add/remove → `resolve_all_role_members` on child subtree, invalidate each
- Role deactivate/reactivate/delete → same as inheritance (all affected members)

## Testing

Data security and robustness are core product requirements. Every feature must ship with comprehensive unit and integration tests covering happy paths, edge cases, and security boundaries. Aim for best-in-class coverage — not just "it works", but "it cannot be bypassed".

### Test the real code path (non-optional)
Tests must exercise the actual code path that runs in production. If the real behavior flows through an HTTP handler, the test must make an HTTP request through the router — not bypass it with direct DB writes. Direct DB manipulation in tests is only appropriate for setting up preconditions (e.g., inserting seed data), never for testing the behavior itself. This ensures middleware, extractors, audit logging, cache invalidation, and error handling are all covered.

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
- TC-prefixed tests map to security vector numbers in `docs/security-vectors.md`.
- Template vars in `filter_expression` must not be quoted: `tenant = {user.tenant}` (correct), `tenant = '{user.tenant}'` (wrong).

### Documentation requirements (non-optional)
After completing any feature or adding tests, always update:
- **`docs/security-vectors.md`** — add a new vector entry for any new attack surface or bypass that was tested. Follow the schema defined at the top of that file: `**Vector**` → `**Attacks**` → `**Defense**` → `**Previously**` *(optional)* → `**Status**` *(optional)* → `**Tests**`. Every attack variant must back-reference a test (`— attack N`) or be explicitly marked under `**Status**`.
- **`docs/permission-system.md`** — keep the conceptual model, policy type descriptions, and examples in sync with the current implementation.

## Bug Fix Protocol
Use TDD: write the failing test(s) first to reproduce the bug, then fix the code until they pass. Never fix first and test after.

1. Write unit and integration tests that reproduce the bug and fail on the current code. Cover all relevant edge cases — add as many tests as needed, not just one of each.
2. Fix the code until the test passes.
3. Security-related bugs (policy bypass, access control, injection) MUST also be documented in `docs/security-vectors.md`. If the fix strengthens an existing vector's defense, add or extend the `**Previously**` section in past tense describing what was wrong — do not use the word "bug" in the doc. If the fix introduces a new threat class, add a new numbered vector following the schema at the top of that file.

## Known cross-cutting concerns

### Rename fragility (label-based identifiers)
BetweenRows uses user-facing labels — `datasource.name`, schema aliases — as identifiers in SQL, policy targets, and decision function context (`ctx.query.tables[*].datasource` / `.schema`). This is deliberate for UX (nobody wants to hand-write UUIDs in policy targets) but makes admin renames breaking changes for:
- SQL queries using the old name
- Decision function JS that hardcoded the old label
- Stored queries, dashboards, and audit log entries tagged with the old name
- `ctx.session.datasource.name` and `ctx.query.tables[*]` matches

Policy **enforcement** continues to work across renames: policies are assigned by `datasource.id` (UUID) and `matches_table` resolves schema aliases to upstream names via `df_to_upstream` at session build time. Only user-typed identifiers are affected. See `docs/permission-system.md` → "Rename fragility and label-based identifiers" for the full impact matrix and admin guidance. A future rename-warning UX is tracked via `// TODO(rename-warning)` comments in the admin-ui edit handlers.

## Known Issues
- **regclass / regproc not supported** — `datafusion-table-providers` drops these columns. Catalog stores `arrow_type = NULL`; `build_arrow_schema` skips them.
- **json/jsonb wire type** — json/jsonb columns are announced as `TEXT` (arrow-pg maps `Utf8` → `Type::TEXT`) in the pgwire RowDescription. Data is correct; some GUI tools won't show a JSON-specific editor.
- **`->>` / `->` operator precedence** — sqlparser 0.59 gives `->>` lower precedence than `=`, so `col->>'key' = 'val'` is misparsed as `col ->> ('key' = 'val')`. In practice this is masked because the filter is pushed down to upstream PostgreSQL before DataFusion evaluates it. Visible in `EXPLAIN` output as a planning error. Workaround: add explicit parens `(col->>'key') = 'val'`. Will be fixed when DataFusion upgrades to sqlparser 0.60+.

## JSON / JSONB Support
- Both `json` and `jsonb` columns map to Arrow `Utf8` via `UnsupportedTypeAction::String` on the pool (set in both `discovery/postgres.rs` and `engine/mod.rs`).
- `datafusion-functions-json` is registered on every `SessionContext` via `register_all()` — provides `->`, `->>`, `?` operators and all JSON UDFs.
- `BetweenRowsPostgresDialect` in `engine/mod.rs` unparses JSON UDFs back to native PG operators for filter pushdown. Wire type is still `VARCHAR`.
- **Pushdown coverage**: `json_as_text`, `json_get_str`, `json_get`, `json_get_json`, `json_contains` are pushed down. Other UDFs (e.g. `json_length`, `json_keys`) are not — DataFusion evaluates them in-process after fetching the rows.
