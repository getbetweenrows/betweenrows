# Permission System — Security Tests

This document lists attack vectors and corresponding integration tests for the permission system.

## Test environment

Integration tests live in `proxy/tests/policy_enforcement.rs`. They use `testcontainers` to spin up a real Postgres container automatically — no manual database setup or environment variables required. Run with `cargo test --test policy_enforcement`.

---

## Attack vectors

### 1. SQL injection via filter_expression

**Vector**: Admin creates a policy with a malicious filter expression.

```json
{ "filter_expression": "1=1 OR 1=1" }
```

**Defense**: Filter expressions are parsed as DataFusion SQL expressions (not raw SQL strings). They are injected as `Filter` nodes in the logical plan, not string-concatenated into SQL.

**Test**: Verify that `SELECT * FROM orders WHERE organization_id = '1' OR 1=1` does not bypass the tenant filter — the rewritten plan must still include the original filter.

---

### 2. Template variable injection

**Vector**: User registers with a tenant string containing SQL syntax:

```
tenant = "acme' OR '1'='1"
```

**Defense**: Template variable substitution uses `Expr::Literal(ScalarValue::Utf8(...))` — the value is inserted as a typed literal, never parsed as SQL. The user's tenant value cannot escape the string literal context.

**Test**: Create a user with `tenant = "x' OR '1'='1"`. Run `SELECT * FROM orders`. Verify the rewritten filter is `organization_id = 'x'' OR ''1''=''1'` (escaped) and returns 0 rows (not all rows).

---

### 3. Policy bypass via table aliases

**Vector**: User runs `SELECT * FROM orders AS o` hoping the alias bypasses the orders row filter.

**Defense**: DataFusion's `TableScan` node always contains the real table name regardless of alias. Row filter matching uses the `TableScan`'s `table_name`, not any user-supplied alias.

**Test**: Run `SELECT * FROM orders AS o WHERE 1=1`. Verify results are tenant-filtered.

---

### 4. Policy bypass via CTEs

**Vector**: User wraps the table in a CTE:

```sql
WITH data AS (SELECT * FROM orders) SELECT * FROM data
```

**Defense**: DataFusion inlines CTEs into the logical plan. The `TableScan` for `orders` remains in the plan tree and receives the row filter injection via `transform_up`.

**Test**: Verify the CTE query returns only tenant-scoped rows.

---

### 5. Column mask bypass via expressions

**Vector**: User writes `SELECT ssn || '' FROM customers` to bypass masking of `ssn`.

**Defense**: Column masking is enforced at the `TableScan` level — `apply_column_mask_at_scan` injects a `Projection` above each scan that replaces the masked column with the mask expression. For direct `SELECT ssn`, the mask is applied before any downstream node sees the raw value. However, if the user writes `SELECT ssn || '' FROM customers`, the `ssn` column reference in the compound expression resolves to the already-masked value from the scan-level Projection, so the concatenation operates on masked data.

**Note**: This is a known limitation for P0 — scan-level masking replaces the column at the source, but compound expressions that reference the column in the user's `SELECT` list operate on the masked value, not the original. The result is masked (not raw), but the transformation may produce unexpected output (e.g., `***-**-6789` concatenated with empty string). This is a P1 enhancement.

**Test**: Document the limitation. Verify `SELECT ssn FROM customers` returns masked value. Verify `SELECT ssn || '' FROM customers` is treated as a limitation/known gap.

---

### 6. Star expansion and column access/masking

**Vector**: User runs `SELECT * FROM customers` when `ssn` and `credit_card` are denied.

**Defense**: DataFusion expands `*` into explicit column references during plan building (via the catalog schema). The proxy intercepts the `Projection` node after expansion and removes denied columns.

**Test**: Run `SELECT * FROM customers`. Verify `ssn` and `credit_card` are absent from results.

---

### 7. Cross-table info leak via JOINs

**Vector**: User runs `SELECT c.ssn FROM orders o JOIN customers c ON o.customer_id = c.id` to bypass per-table filters.

**Defense**: Row filters are applied to each `TableScan` independently. The filter on `customers` is injected below the `customers` TableScan; the filter on `orders` below `orders`. Both apply in the joined plan.

**Test**: Run a JOIN query. Verify both tables are independently filtered.

---

### 8. Row filter bypass via subqueries

**Vector**: User runs `SELECT * FROM (SELECT * FROM orders) sub`.

**Defense**: DataFusion's logical planner inlines subqueries. The `TableScan` for `orders` is present in the plan and receives the row filter.

**Test**: Verify subquery results are tenant-filtered.

---

### 9. access_mode bypass

**Vector**: With `access_mode = "policy_required"`, user queries a table with no assigned permit policy.

**Defense**: The proxy injects `Filter(lit(false))` for such tables, producing empty results without round-tripping to the upstream database.

**Test**: Configure a datasource as `policy_required` with no policies. Run `SELECT * FROM orders`. Verify 0 rows returned. Verify no upstream query was executed (check upstream query log).

---

### 10. Optimistic concurrency bypass

**Vector**: Two admins simultaneously update the same policy; second write silently overwrites the first.

**Defense**: `PUT /policies/{id}` requires the current `version` in the payload. If `version` doesn't match, the server returns `409 Conflict`. The client must reload and retry.

**Test**: Fetch policy at version 1. Submit update with `version: 1`. Concurrently submit another update with `version: 1`. Verify one returns `409` and the other succeeds.

---

### 11. `column_deny` policy type strips columns at query time

**Vector**: Admin creates a `column_deny` policy on `ssn`, expecting the column to be stripped from query results immediately — without requiring the user to reconnect.

**Defense**: `column_deny` is a first-class policy type. `PolicyHook::handle_query` processes all deny-type policies in the session, matches `column_deny` policies against the queried tables using `TargetEntry` pattern matching, and adds matched columns to `column_denies`. The column is stripped from the `Projection` node before execution. Unlike `table_deny`, `column_deny` does NOT short-circuit the query — all non-denied columns are still returned.

**Test**: Create a `column_deny` policy on `ssn`. Assign to datasource. Without reconnecting, run `SELECT ssn FROM employees`. Verify `ssn` column is absent from the result set.

---

### 12. Disabled policies still enforced in visibility layer

**Vector**: Admin disables a policy with `column_access deny`, expecting the column to reappear in `information_schema.columns` on next reconnect.

**Bug**: `compute_user_visibility()` loaded policies for ALL assigned policy IDs, including those belonging to disabled policies. The `column_access deny` block didn't check if the parent policy was enabled, so disabling a policy had no effect on schema visibility.

**Defense**: `compute_user_visibility()` now loads policies only for *enabled* policy IDs (those from the `is_enabled = true` filtered query). Disabled policies contribute neither to `visible_tables` nor `denied_columns`.

**Test**:
- Unit: `engine::tests::test_disabled_policy_column_deny_not_applied` — disabled policy → `denied_columns` is empty.
- Unit: `engine::tests::test_enabled_policy_column_deny_applied` — enabled policy → `denied_columns` contains `ssn`.
- Manual: Disable a policy with `column_access deny`. Without reconnecting, verify `ssn` reappears in `information_schema.columns` on the next query (policy changes trigger an immediate `SessionContext` rebuild for all active connections).

---

### 13. Column mask had no effect — original values returned

**Vector**: Admin creates a `column_mask` policy expecting `ssn` values to be masked (e.g. `'***-**-' || RIGHT(ssn, 4)`). Data is queried and original SSN values are returned as-is.

**Bug**: `parse_mask_expr` built a standalone SQL plan (`SELECT {mask} AS {col} FROM {schema}.{table}`) via `ctx.sql()`, then extracted the first `Projection` expression. Two problems:
1. **Double alias**: the extracted expression was already `Alias(inner, "ssn")` from the `AS ssn` clause; `apply_projection` then wrapped it again with `.alias(col_name)` producing `Alias(Alias(...))`, which DataFusion silently resolved by dropping the inner alias — causing column not found or type mismatches at execution time.
2. **Qualified column references**: the inner expression carried table-qualified references (e.g. `public.customers.ssn`) bound to the standalone plan's `TableScan`. These did not resolve against the actual query plan, so the mask evaluated to NULL or errored.

**Defense**: `parse_mask_expr` is now sync and uses `sql_ast_to_df_expr(..., Some(ctx))` — the same sqlparser → DataFusion AST converter used for row filter expressions, extended with `FunctionRegistry` lookup. No standalone plan is built. Column references are unqualified (`col("ssn")`), resolving correctly against the real query plan. No alias wrapping occurs — `apply_projection` provides the alias.

**Test**:
- Unit: `hooks::policy::tests::test_exec_permit_column_mask` — literal mask `'REDACTED'` applied; all SSN values in result equal `"REDACTED"`.
- Unit: `hooks::policy::tests::test_exec_column_mask_with_row_filter` — row filter (3 rows) + mask combined; 3 rows returned with `ssn = "***"`.
- Unit: `hooks::policy::tests::test_deny_overrides_mask` — column denied and masked; deny takes priority, column absent from result.

---

### 14. Two permit policies with row_filter produced a union (OR) instead of intersection (AND)

**Vector**: Two permit policies both have `row_filter` policies on the same table with different conditions (e.g. Policy A: `org_id = 'acme'`, Policy B: `status = 'active'`). A user assigned both policies can see ALL rows matching either condition — including rows from other tenants or inactive records that neither policy alone intended to expose.

**Bug**: In `PolicyEffects::collect()`, cross-policy row filters were combined with OR semantics (seed `lit(false)`, combinator `.or()`). The intent was "any permit match grants access", but this allows a user assigned multiple narrow policies to see the union of all their allowed sets — potentially broader than any single policy intended.

**Defense**: Cross-policy row filters are now combined with AND semantics (seed `lit(true)`, combinator `.and()`). Each permit policy adds a restriction; users see the intersection. Within a single policy, multiple `row_filter` policies are still AND'd (unchanged). Deny policies are unaffected — the deny short-circuit on first match is equivalent to OR across denies.

**Test**:
- Unit: `hooks::policy::tests::test_exec_two_permits_row_filter_and` — two disjoint filters (`acme` / `globex`) → AND → 0 rows.
- Unit: `hooks::policy::tests::test_exec_two_permits_row_filter_and_overlapping` — overlapping filters (`org_id = 'acme'` ∩ `name != 'Charlie'`) → 2 rows (Alice + Bob only).
- Unit: `hooks::policy::tests::test_row_filters_and_across_policies` — plan structure shows AND expression containing both filter values.

---

### 15. `SELECT <denied-column>` returns silent empty rows instead of an error

**Vector**: User runs `SELECT ssn FROM customers` where `ssn` is denied. They receive many rows with empty/null values and incorrectly conclude the column is empty in the database.

**Bug**: When all selected columns were stripped by `column_access deny`, `new_exprs` became empty. `LogicalPlanBuilder::project([])` produced a zero-column projection that DataFusion executed successfully — returning N rows with no column data. Clients rendered this as empty rows.

**Defense**: `PolicyHook` now checks for an empty `new_exprs` after column stripping and returns SQLSTATE `42501` (insufficient_privilege) listing the denied columns, before attempting to build the projection.

**Test**: Create a policy with `column_access deny` on `ssn`. Run `SELECT ssn FROM customers`. Verify the response is an error with SQLSTATE `42501` and not an empty result set.

---

### 16. Deny semantics and `column_mask` are mutually exclusive by type system construction

**Vector**: In a prior design (effect + obligation_type), an admin could create a `deny`-effect policy with a `column_mask` obligation. `PolicyHook` only applied `column_mask` from permit policies, so the mask silently had no effect — the column's real value was returned.

**Defense**: The flat `policy_type` enum eliminates this class of misconfiguration entirely. There is no `effect` field. `column_mask` is a specific policy type (always permit semantics). `column_deny` is a different policy type (always deny semantics). It is structurally impossible to express "deny + column_mask" — the policy has exactly one type. `validate_definition()` in `dto.rs` enforces that `column_mask` policies must have a `mask_expression`, while `column_deny` and `table_deny` must have no `definition` at all.

**Test**:
- `create_policy_column_mask_missing_mask_expression_422` — `column_mask` without `mask_expression` → `422`.
- `create_policy_column_deny_with_definition_422` — `column_deny` with a `definition` object → `422`.
- `create_policy_table_deny_with_definition_422` — `table_deny` with a `definition` object → `422`.

---

### 17. `table_deny` with `tables: ["*"]` — schema blocked at query time

**Vector**: Admin creates a `table_deny` policy targeting schema `analytics` with `tables: ["*"]`, expecting all tables in that schema to be blocked for the assigned user. Without the implementation, the user can still query `analytics.*` tables.

**Defense**: `compute_user_visibility()` in `engine/mod.rs` processes `table_deny` policies and populates `denied_tables` with all matching `(df_alias, table)` pairs. `build_user_context()` skips tables in `denied_tables` when building the user's filtered `SessionContext`. Because `tables: ["*"]` matches every table in the schema, the entire schema becomes inaccessible. This applies in both `open` and `policy_required` modes. At query time, `PolicyHook` also short-circuits on the first `table_deny` match with a descriptive error.

**Test**:
- Integration: Create a `table_deny` policy with `targets: [{ schemas: ["analytics"], tables: ["*"] }]`. Assign to datasource for a test user. Connect as that user and run `SELECT * FROM analytics.reports`. Verify a "table not found" or policy-denied error (not data rows).

---

### 18. `table_deny` — specific table blocked at query time

**Vector**: Admin creates a `table_deny` policy on table `public.payments`, expecting that table to be blocked for the assigned user while the rest of `public` remains accessible.

**Defense**: `compute_user_visibility()` processes `table_deny` policies and populates `denied_tables` with matching `(df_alias, table_name)` pairs. `build_user_context()` skips tables in `denied_tables` when building the user's filtered `SessionContext`, leaving all other tables in the schema visible. At query time, `PolicyHook` short-circuits on the first `table_deny` match with a descriptive error before plan execution.

**Test**:
- Integration: Create a `table_deny` policy with `targets: [{ schemas: ["public"], tables: ["payments"] }]`. Assign to datasource. Connect as that user and run `SELECT * FROM public.payments`. Verify a policy-denied error. Run `SELECT * FROM public.orders`. Verify normal results (other tables unaffected).

---

### 19. Glob pattern matching bypassed with unexpected table name

**Vector**: Admin creates a `row_filter` with `table: "raw_*"` intending to filter all tables whose names start with `raw_`. If matching is exact only, tables like `raw_events` and `raw_orders` are not filtered, leaking rows.

**Defense**: `matches_pattern()` in `policy_match.rs` supports prefix glob: if the pattern ends with `*`, it uses `starts_with(prefix)` matching. `matches_schema_table()` delegates to `matches_pattern()` for both schema and table fields. The same function is used by both `PolicyHook` (query-time) and `compute_user_visibility()` (connect-time), ensuring consistent semantics.

**Test**: 14 unit tests in `proxy/src/policy_match.rs` cover exact match, `*` wildcard, prefix glob on table, prefix glob on schema, combined globs, alias resolution, and non-matching cases (`orders_raw` does not match `raw_*`).

---

### 20. Policy type encodes grant vs. strip — no ambiguous `action` field

**Vector**: In a prior design, `column_access` obligations had an `action` field (`"allow"` or `"deny"`) inside the definition JSON. With a `permit`-effect policy containing `"action": "deny"`, `compute_user_visibility()` checked `col_def.action == "allow"` to decide whether to grant table access. A mismatch silently denied access — the user saw "table not found" instead of data in `policy_required` mode.

**Defense**: The `action` field was removed entirely. Intent is now encoded directly in `policy_type`:
- `column_allow` — always an allowlist (grants table access in `policy_required` mode, specifies visible columns)
- `column_deny` — always a denylist (strips columns at query time, does not grant access)

There is no ambiguous per-definition `action` field. `compute_user_visibility()` branches on `policy_type` directly. `validate_targets()` in `dto.rs` enforces that both `column_allow` and `column_deny` require a non-empty `columns` array. The type system makes the wrong combination unrepresentable.

**Test**:
- Unit: `engine::tests::test_permit_column_allow_wildcard_grants_full_visibility_policy_required` — `column_allow` with `columns: ["*"]` in a `policy_required` datasource → table is visible, `visible_tables` non-empty.
- Unit: `hooks::policy::tests::test_column_deny_no_table_permit` — `column_deny` policy in `policy_required` mode → `lit(false)` (deny type alone does not grant table access).
- Unit: `admin::policy_handlers::tests::create_policy_column_allow_missing_columns_422` — `column_allow` without `columns` in targets → `422`.


---

### 21. Denied queries leave no audit trail (silent denial)

**Vector**: A user submits a query blocked by a deny policy. If the audit log is only written on the success path, there is no record of the denied access attempt — attackers can probe policy boundaries without leaving evidence.

**Bug**: The `tokio::spawn` audit write in `PolicyHook::handle_query` was placed after all `return Some(Err(...))` paths. Any failed or denied query short-circuited before the audit write.

**Defense**: `handle_query` now uses a labeled block (`'query: { ... }`) that returns `(result, status, error_message, rewritten_query)` on all paths. The audit write follows the block and runs unconditionally for every auditable query. Status values: `"success"`, `"error"`, `"denied"`.

**Test**:
- Integration: `tc_audit_01_success_audit_status` — successful query → `status: "success"`, `error_message: null`, `execution_time_ms ≥ 0`, `rewritten_query` contains actual SQL (not fake comment).
- Integration: `tc_audit_02_denied_audit_status` — deny-policy query → `status: "denied"`, `error_message` contains the policy name.
- Integration: `tc_audit_03_error_audit_status` — query for non-existent table → `status: "error"`, `error_message` populated.
- Integration: `tc_audit_04_status_filter` — `GET /audit/queries?status=denied` returns only denied entries.

---

### 22. Audit duration excludes encode phase (misleading timing)

**Vector**: `execution_time_ms` was captured after `execute_logical_plan` but before `encode_dataframe`. Since DataFusion returns a lazy `DataFrame`, the actual row-fetching happens during encoding. Timing was systematically under-reported.

**Defense**: `elapsed_ms` is now captured after the labeled block exits, covering planning + policy eval + execution + encoding (full user-perceived latency).

**Test**: Covered by `tc_audit_01_success_audit_status` — `execution_time_ms ≥ 0` (positive timing is asserted).

---

### 23. Rewritten query in audit log was fake (/* policy-rewritten */ comment only)

**Vector**: The audit log's `rewritten_query` field previously just prepended `/* policy-rewritten */` to the original query string. The actual row filters and column masks applied by policies were not visible, defeating the purpose of the audit trail.

**Defense**: DataFusion's `Unparser` with `BetweenRowsPostgresDialect` is used to serialize the final `LogicalPlan` (after all policy rewrites) back to SQL. If unparsing fails, the fallback is `/* plan-to-sql failed */ {original_query}`.

**Test**: `tc_audit_01_success_audit_status` — `rewritten_query` must not contain `/* policy-rewritten */` and must be non-empty when a row filter was applied.

---

### 24. Write statement rejected by ReadOnlyHook leaves no audit trail

**Vector**: A user submits a write statement (INSERT, UPDATE, DELETE, DROP, SET, etc.). `ReadOnlyHook` rejects it before `PolicyHook` runs, so no audit record is created. An attacker can probe write access without evidence.

**Bug**: Hook execution order was `[ReadOnlyHook, PolicyHook]`. `ReadOnlyHook` returned `Some(Err(...))` and short-circuited the chain, so `PolicyHook` never saw the statement.

**Defense**: Hook order is now `[PolicyHook, ReadOnlyHook]`. `PolicyHook` runs first: for non-`Query` statements that are not on the read-only passthrough list, it calls `audit_write_rejected()` (writes a `"denied"` audit entry with `error_message: "Only read-only queries are allowed"`) then returns `None`. `ReadOnlyHook` then runs and enforces the rejection. A shared `is_allowed_statement()` function in `read_only.rs` is the single source of truth for the allowlist — `PolicyHook` uses it to decide which statements to audit without duplicating the logic.

**Test**: `tc_audit_05_write_rejected_audit_status` — `INSERT` against the proxy → audit entry has `status: "denied"`, `error_message` contains `"read-only"`.

---

### 25. Row filter on aggregate with zero-column projection (DataFusion 52+ optimisation)

**Vector**: DataFusion 52+ optimises `SELECT COUNT(*) FROM t` to `TableScan(projection=Some([]))` — projecting zero columns. Our post-planning filter injection (`apply_row_filters`) adds a `Filter(tenant = 'acme')` node above this scan, but the scan's output schema has no columns, so the filter cannot resolve `tenant` → schema mismatch at execution time.

**Bug**: `apply_row_filters` injected the filter unconditionally without checking whether filter-referenced columns were present in the scan's projected schema.

**Defense**: Before wrapping the `TableScan` with a `Filter` node, extract column references from the filter expression (`Expr::column_refs()`). If `projection = Some(indices)`, merge any missing column indices into the projection and rebuild the `TableScan` via `TableScan::try_new(...)` with the expanded list. `lit(false)` and other zero-column-ref filters are a no-op (no expansion). Filter referencing a column absent from the full table schema returns a plan error.

**Test**: `aggregate_with_row_filter` — `SELECT COUNT(*)` and `SELECT SUM(amount)` with a tenant row filter → returns correct tenant-scoped counts. Unit tests: `test_row_filter_expands_narrow_projection`, `test_row_filter_no_expand_when_all_columns_present`, `test_row_filter_lit_false_no_expand`.

---

### 26. table_deny metadata leakage prevention (404-not-403 principle)

**Vector**: A `table_deny` policy that rejects a query with "access denied" reveals that the table exists. An attacker can probe for hidden tables by observing the difference between "table not found" and "access denied" responses.

**Defense**: `table_deny` tables are removed from the per-user catalog at connection time (`build_user_context` / `compute_user_visibility`). Queries against a denied table fail with "table not found" — indistinguishable from querying a non-existent table. The audit status is `"error"` (not `"denied"`), which matches any other query planning failure, providing no additional signal to the attacker.

**Test**: `deny_policy_row_filter_rejected` — error message must not contain the policy name. `tc_audit_02_denied_audit_status` — audit status is `"error"`, `error_message` does not contain the policy name. `tc_audit_04_status_filter` — `status=error` filter matches these entries.

---

### 27. Column deny scoping in multi-table JOINs

**Vector**: Three tables (`a`, `b`, `c`) share a column name (`name`). Denying `name` on `a` and `c` might accidentally also strip `b.name` if the deny logic uses unqualified matching.

**Defense**: Column deny is enforced at two levels: (1) visibility-level via `compute_user_visibility` / `build_user_context` — denied columns are removed from the per-user `SessionContext` schema at connect time, scoped per-table; (2) defense-in-depth via `apply_projection_qualified` — the top-level Projection uses DFSchema qualifiers to scope deny patterns to their source table.

**Test**: `tc_join_02_multi_table_join_shared_name` — JOIN 3 tables all with `name`. Deny `name` on `a` and `c`. `SELECT *` returns exactly one `name` column (from `b`), plus `id` from all three tables and `a_val`, `b_val`, `c_val`.

---

### 28. Table alias does not bypass column deny or column mask

**Vector**: User aliases a table (`SELECT * FROM customers AS c`) hoping the alias bypasses column-level policies. If the policy rewriter only checks the real table name, and the planner resolves columns under the alias qualifier, denied or masked columns might leak.

**Defense**: Column deny is enforced at visibility level — denied columns are removed from the schema before query planning, so they never appear in `SELECT *` regardless of alias. Column mask is enforced at the `TableScan` level via `apply_column_mask_at_scan` (injected `Projection` above each scan), which operates on the real `TableScan` table name before any alias is applied.

**Test**:
- `tc_join_03a_alias_column_deny` — deny `email` on `customers`. `SELECT * FROM customers AS c` returns only `id, name`. `SELECT c.email FROM customers AS c` errors (column not found).
- `tc_join_03b_alias_column_mask` — mask `email` on `customers`. `SELECT c.email FROM customers AS c` returns the masked value `***@example.com`, not the raw email.

---

### 29. row_filter alone does not grant visibility in policy_required mode

**Vector**: In `policy_required` mode, a `row_filter` policy is assigned to a table but no `column_allow` policy. If `row_filter` silently grants table visibility, the user can see the table in `information_schema` and query it, bypassing the zero-trust model.

**Defense**: `compute_user_visibility` only adds tables to `visible_tables` when a `column_allow` policy exists. `row_filter` and `column_mask` do not grant table access. Without a `column_allow` policy, the table is excluded from the per-user `SessionContext`, making it invisible in both `information_schema` queries and direct table references.

**Test**: `tc_zt_04_sidebar_sync_row_filter_only` — `policy_required` datasource with only a `row_filter` on `users`. `SELECT ... FROM information_schema.tables` returns 0 rows for the schema. Direct `SELECT * FROM users` errors (table not found). Catalog admin API still shows the table (admin view is unfiltered).

---

### 30. CTE wrapping does not bypass column deny, column mask, or column allow

**Vector**: User wraps a table in a CTE (`WITH t AS (SELECT * FROM users) SELECT ssn FROM t`) hoping that the CTE alias changes the column qualifier, causing deny/mask/allow patterns to miss.

**Defense**: Column deny is enforced at visibility level — denied columns are excluded from the `SELECT *` inside the CTE, so they never appear in the CTE output schema. Column mask is enforced at `TableScan` level via `apply_column_mask_at_scan`, which injects a mask `Projection` above the scan before the CTE node is constructed. Column allow (in `policy_required` mode) restricts the schema to allowed columns only, so non-allowed columns are absent from the CTE output.

**Bug found**: Column mask was previously only applied at the top-level `Projection` via `apply_projection_qualified`. CTE nodes (`SubqueryAlias`) change the DFSchema qualifier from the real table name to the CTE alias, causing the top-level mask matching to miss. Raw values leaked through CTEs.

**Fix**: Added `apply_column_mask_at_scan` method in `PolicyEffects` — applies column masks at the `TableScan` level via `transform_up`, before CTE/subquery nodes can change the qualifier. Uses `alias_qualified` to preserve the table qualifier on the masked column. Masks cleared from `column_masks` after scan-level application to prevent double-masking.

**Test**:
- `tc_plan_01a_cte_column_deny` — deny `ssn`. CTE `SELECT *` excludes `ssn`. Explicit `SELECT ssn FROM t` errors.
- `tc_plan_01b_cte_column_mask` — mask `ssn`. CTE `SELECT ssn FROM t` returns masked value `***-**-6789`.
- `tc_plan_01c_cte_column_allow` — allow only `id, name`. CTE `SELECT ssn FROM t` errors (not in allow list).

---

### 31. Subquery-in-FROM wrapping does not bypass column deny, column mask, or column allow

**Vector**: User wraps a table in a subquery (`SELECT sub.ssn FROM (SELECT * FROM users) AS sub`) hoping that the `SubqueryAlias` changes the qualifier from `users` to `sub`, causing deny/mask/allow patterns to miss at the top-level Projection.

**Defense**: Same as CTE (vector 30). Column deny works at visibility level. Column mask works at `TableScan` level via `apply_column_mask_at_scan`. Column allow restricts the schema before the subquery is constructed.

**Bug found**: Same as CTE — column mask was bypassed by subquery aliasing. Fixed by scan-level mask enforcement.

**Test**:
- `tc_plan_02a_subquery_column_deny` — deny `ssn`. Subquery `SELECT *` excludes `ssn`. Explicit `SELECT sub.ssn` errors.
- `tc_plan_02b_subquery_column_mask` — mask `ssn`. Subquery `SELECT sub.ssn` returns masked value `***-**-6789`.
- `tc_plan_02c_subquery_column_allow` — allow only `id, name`. Subquery `SELECT sub.ssn` errors (not in allow list).

---

### 32. Row filter + column mask on the same column

**Vector**: A row filter and column mask target the same column (e.g. `ssn`). If masks are applied before filters in the plan tree, row filters evaluate against masked values instead of raw data, causing incorrect filtering. Example: filter `ssn != '000-00-0000'` passes on masked value `'***-**-0000'`, leaking a row that should be excluded.

**Bug found**: `apply_row_filters` ran before `apply_column_mask_at_scan`. Both use `transform_up` on `TableScan`, producing `Filter(row_filter) → Projection(mask) → TableScan`. Data flows bottom-up: scan → mask → filter, so the filter saw masked values.

**Defense**: Swap the call order so masks are applied first. With `apply_column_mask_at_scan` running before `apply_row_filters`, `transform_up` places the `Filter` between `TableScan` and the mask `Projection`: `Projection(mask) → Filter(row_filter) → TableScan`. Data flows: scan → filter (raw data) → mask. Row filters always evaluate against unmasked values.

**Test**: `row_filter_and_column_mask_same_column` — filter excludes `ssn = '000-00-0000'`, mask replaces ssn with `'***-**-XXXX'`. Verifies 2 rows returned (not 3) and values are masked.

---

## RBAC (Role-Based Access Control) — Vectors 33–45

### 33. Role-based datasource access grants connection

**Vector**: User has no direct `data_source_access` entry but is a member of a role that has role-scoped access.

**Defense**: `resolve_datasource_access()` checks all three scopes: user-direct, role-based (resolving full role hierarchy), and all-scoped.

**Test**: `rbac_02_role_based_access` — user with no direct access but member of role with datasource access can connect.

---

### 34. Inherited role datasource access

**Vector**: User is member of child role; parent role has datasource access. Child role has no direct access grant.

**Defense**: `resolve_user_roles()` BFS traverses the full inheritance DAG upward, so parent role access is found.

**Test**: `rbac_03_inherited_role_access` — user in child role connects via parent role's datasource access.

---

### 35. Cycle detection in role inheritance

**Vector**: Admin attempts to create a circular inheritance chain (A→B→C→A) to cause infinite loops in role resolution.

**Defense**: `detect_cycle()` runs BFS from proposed parent upward before insertion. SQLite single-writer prevents concurrent cycle creation.

**Test**: `rbac_15_cycle_detection` — creating A→B→C, then C→A, returns 422 error.

---

### 36. Self-referential role inheritance

**Vector**: Admin sets a role as its own parent to create a trivial cycle.

**Defense**: `detect_cycle()` short-circuits on `parent_id == child_id`.

**Test**: `rbac_16_self_referential` — returns 422 error.

---

### 37. Inheritance depth cap (max 10)

**Vector**: Admin creates a chain deeper than 10 levels to cause performance degradation or stack overflow in BFS.

**Defense**: `resolve_user_roles()` caps BFS at depth 10. `check_inheritance_depth()` rejects edges that would exceed the limit.

**Test**: `rbac_17_depth_cap` — chain of 10 accepted; chain of 11 rejected with 422.

---

### 38. Deny-wins across roles

**Vector**: User is member of Role A (which has `column_deny` on `ssn`) and Role B (which has `column_allow` including `ssn`). User expects to see `ssn`.

**Defense**: Deny policies always win regardless of source. `column_deny` from any assignment path removes the column.

**Test**: `rbac_14_deny_wins_across_roles` — column is denied despite allow from another role.

---

### 39. Scope mismatch: both user_id AND role_id

**Vector**: API caller sends both `user_id` and `role_id` in an assignment request, attempting to create an ambiguous assignment.

**Defense**: `assign_policy` validates scope constraints. `scope='user'` requires only `user_id`, `scope='role'` requires only `role_id`, `scope='all'` requires neither.

**Tests**: `rbac_70` through `rbac_73` — all combinations return 400.

---

### 40. Role deactivation immediately removes access

**Vector**: Admin deactivates a role. Users still in the role attempt to access data on their next query.

**Defense**: `resolve_user_roles()` skips inactive roles. Deactivation triggers cache invalidation + context rebuild for all affected users.

**Test**: `rbac_42_deactivate_loses_policies` — after deactivation, member's query returns no rows (in policy_required mode).

---

### 41. Deactivated role in middle of inheritance chain

**Vector**: Chain A→B→C where B is deactivated. Users in A expect to still get C's policies via B.

**Defense**: BFS stops at inactive roles — B being inactive means C (and everything above B) is unreachable from A.

**Test**: `rbac_43_deactivate_middle_breaks_chain` — policies from C no longer apply to A's members.

---

### 42. Template variables resolve from user, not role

**Vector**: `row_filter` with `{user.tenant}` assigned to a role. Attacker expects the filter to use the role's properties instead of the connecting user's tenant.

**Defense**: Template variable substitution happens in `PolicyHook` using the authenticated user's metadata, not role metadata. Roles have no tenant/username properties.

**Test**: `rbac_24_row_filter_via_role` — filter uses the connecting user's tenant, not any role property.

---

### 43. SQL injection via role name

**Vector**: Admin creates a role named `"; DROP TABLE role; --"`.

**Defense**: Role name validation restricts to `[a-zA-Z0-9_.-]`, 3-50 chars, must start with a letter.

**Test**: `rbac_34_invalid_characters` — returns 422 error.

---

### 44. Diamond inheritance deduplication

**Vector**: User is in role A which inherits from B and C, both of which inherit from D. Policy on D should apply once, not twice.

**Defense**: `resolve_effective_assignments()` deduplicates by `policy_id`, keeping the assignment with the lowest priority number.

**Test**: `rbac_18_diamond_dedup` — policy applied exactly once.

---

### 45a. Revoked role datasource access persists on active connections (H1)

**Vector**: Admin revokes a role's access to a datasource via `PUT /datasources/{id}/access/roles` (removing the role from the list). Users who are already connected via that role's access still have active `SessionContext` entries reflecting the old access.

**Bug**: `set_datasource_role_access` only invalidated members of newly-added roles, not members of removed roles. Users who lost access could continue querying until they disconnected.

**Defense**: Before deleting old role-scoped entries, capture `old_role_ids`. After commit, compute `all_affected = old_role_ids ∪ new_role_ids` and invalidate members of all affected roles. Also added audit log entry.

**Test**: Code review — `set_datasource_role_access` now invalidates `old_role_ids.union(&new_role_ids)`.

---

### 45b. Revoked user datasource access has no invalidation or audit (H3b)

**Vector**: Admin changes the user access list via `PUT /datasources/{id}/users`, removing a user. The removed user's active connections retain the old `SessionContext` and can continue querying. No audit trail is recorded.

**Bug**: `set_datasource_users` had no cache invalidation and no audit log call.

**Defense**: Before deleting old user-scoped entries, capture `old_user_ids`. After commit, invalidate `old_user_ids ∪ new_user_ids`. Added audit log entry with `resource_type: "datasource"`, `action: "update"`, changes showing before/after user IDs.

**Test**: Code review — `set_datasource_users` now invalidates all affected users and writes audit log.

---

### 45c. Silent rebuild failure leaves stale SessionContext (H2)

**Vector**: After a policy or role mutation, `rebuild_contexts_for_datasource` or `rebuild_contexts_for_user` fails for a specific connection (e.g., the upstream database is unreachable). The stale `SessionContext` remains, potentially missing new deny policies.

**Bug**: On rebuild failure, the error was logged but the connection entry was left in place with the old context.

**Defense**: On rebuild failure, the stale connection entry is removed from `connection_contexts`. The user's next query will receive a "Session context not found — please reconnect" error, forcing a fresh connection that re-evaluates `check_access` and `build_user_context`.

**Test**: Code review — both `rebuild_contexts_for_datasource` and `rebuild_contexts_for_user` now call `conn_store.connection_contexts.remove(&conn_id)` in the error branch.

---

### 45d. Inheritance depth check ignores child subtree (H4)

**Vector**: Role A has depth 2 above. Role B has depth 8 below (child chain). Adding A as parent of B: old check only looked at depth above A (2 + 1 = 3 < 10, accepted). But total chain depth is 2 + 1 + 8 = 11, exceeding the limit.

**Bug**: `add_parent` only called `check_inheritance_depth` on the parent (upward), ignoring the child's downward subtree.

**Defense**: Added `check_inheritance_depth_down` (BFS downward). The depth check now computes `total = depth_above_parent + 1 + depth_below_child` and rejects if > 10.

**Test**: `u16_depth_down_chain` — verifies downward depth calculation. `u17_total_depth_check` — verifies total depth accounting.

---

### 46. Effective members source annotation shows wrong role name

**Vector**: Admin views the Members tab for a parent role (e.g., `data-analysts`). A user who is a direct member of a child role (`data-architect`, which inherits from `data-analysts`) appears as "via role 'data-analysts'" instead of "via role 'data-architect'". This misleads admins about where the member relationship actually exists.

**Bug**: In `resolve_effective_members()`, the BFS source annotation for child roles used `all_roles.get(&role_id)` (the top-level role being viewed) and `all_roles.get(&current)` (the intermediate parent) instead of `all_roles.get(&child_id)` (the actual child role the member belongs to). The source label should indicate which role the user is a *direct member of*, not which role is being viewed.

**Defense**: Changed both BFS levels in `resolve_effective_members()` to use `child_id` for the role name lookup. The source label now correctly says "via role '<child_role_name>'" — identifying the child role the member actually belongs to.

**Test**: `u13_resolve_all_members` — verifies BFS downward member collection. Manual: create parent→child hierarchy, add member to child, view parent's effective members — should show "via role '<child_name>'".

---

### 45. Role deletion cascade integrity

**Vector**: Deleting a role that has members, inheritance edges, and policy assignments. Orphaned references could cause query failures.

**Defense**: All FK relationships use `ON DELETE CASCADE`. Members, inheritance edges, policy assignments, and data_source_access entries are automatically deleted.

**Test**: `rbac_19_delete_cascades` — after deletion, no orphaned rows exist.

---

### 47. Inactive role granted datasource access

**Vector**: Admin grants datasource access to an inactive (deactivated) role via `PUT /datasources/{id}/access/roles`. If accepted, the inactive role's access entry exists in `data_source_access` but has no effect — until someone reactivates the role, at which point members unexpectedly gain access without an explicit grant decision.

**Defense**: `set_datasource_role_access` now validates `is_active` on each role before inserting the access entry. Inactive roles are rejected with HTTP 400.

**Test**: `rbac_74_set_datasource_role_access_rejects_inactive_role` — create role, grant access (204), deactivate role, attempt grant again (400).

---

### 48. TOCTOU in role inheritance cycle detection

**Vector**: Two concurrent `add_parent` requests could each pass cycle detection independently but together create a cycle, because the detection and insert were not atomic.

**Defense**: `add_parent` now wraps `detect_cycle` + `check_inheritance_depth` + `insert` in a single database transaction. SQLite's single-writer serialization ensures that the second concurrent request sees the first's insert during its cycle detection.

**Test**: Covered by existing `rbac_15_cycle_detection` and `rbac_16_self_referential`. The TOCTOU fix is structural (transaction boundary), not behavioral.

---

### 49. Audit log outside transaction — all mutation handlers

**Vector**: Multiple handlers performed entity mutations and audit log insertion as separate operations, or called `audit_log` outside the transaction boundary. Specific cases:
- `create_role` / `update_role`: mutation on `&state.db`, audit on `&state.db` — two separate statements, not atomic.
- `set_datasource_users`: mutation inside `txn`, audit after `txn.commit()` on `&state.db` — audit completely outside the transaction.
- `delete_role`: did not audit cascaded policy assignment deletions.
- `remove_parent`: invalidated caches before `txn.commit()` — stale cache served during the window between invalidation and commit.

**Defense**: All mutation handlers now use `AuditedTxn` (from `admin_audit.rs`), a wrapper around `DatabaseTransaction` that queues audit entries via `txn.audit(...)` and writes them atomically on `commit()`. This makes the correct pattern (audit inside the transaction) the only pattern — `AuditedTxn::commit()` errors if no entries are queued, preventing unaudited commits. The old `audit_delete` / `audit_insert` helpers have been removed.

Additional fixes:
- `delete_role` now audits each cascaded policy assignment as `Unassign` before deleting the role.
- `remove_parent` collects affected user IDs before the transaction and invalidates caches after `txn.commit()`.
- `set_datasource_role_access` validates all roles before any mutations (no validate-after-delete).

**Test**: `audited_txn_commits_with_entries`, `audited_txn_rejects_empty_commit`, `audited_txn_rollback_on_drop`, `audited_txn_multiple_entries` (unit tests in `admin_audit.rs`). Structural enforcement via the type system.

