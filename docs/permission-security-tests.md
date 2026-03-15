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

**Defense**: Column masking works by replacing `col("ssn")` in the `Projection` node. If `ssn || ''` is used, the `ssn` reference still passes through the `Projection` as a sub-expression. The proxy replaces the `ssn` column reference inside the expression with the masked value.

**Note**: This is a known limitation for P0 — column masking replaces direct `col(name)` references in the projection. Compound expressions that reference the column are not masked. This is a P1 enhancement.

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

### 11. `column_access deny` on deny-effect policies ignored at query time

**Vector**: Admin creates a **deny-effect** policy with a `column_access deny` obligation on `ssn`, expecting the column to be stripped from query results immediately.

**Bug**: `PolicyHook` only processed `column_access` obligations from *permit* policies (the loop at `handle_query` iterated over `session.permit_policies`). Deny-effect policies were only checked for table-level "Access denied" errors. So `column_access deny` on a deny policy had no query-time effect — the column appeared in results until the user reconnected (when `compute_user_visibility()` hid it from the schema).

**Defense**: `PolicyHook::handle_query` now runs a second loop over `session.deny_policies` after the permit loop, processing only `column_access` obligations and adding matched columns to `column_denies`.

**Test**: Create a deny-effect policy with `column_access deny` on `ssn`. Assign to datasource. Without reconnecting, run `SELECT ssn FROM employees`. Verify `ssn` column is absent from the result set.

---

### 12. Disabled policies still enforced in visibility layer

**Vector**: Admin disables a policy with `column_access deny`, expecting the column to reappear in `information_schema.columns` on next reconnect.

**Bug**: `compute_user_visibility()` loaded obligations for ALL assigned policy IDs, including those belonging to disabled policies. The `column_access deny` block didn't check if the parent policy was enabled, so disabling a policy had no effect on schema visibility.

**Defense**: `compute_user_visibility()` now loads obligations only for *enabled* policy IDs (those from the `is_enabled = true` filtered query). Disabled policies contribute neither to `visible_tables` nor `denied_columns`.

**Test**:
- Unit: `engine::tests::test_disabled_policy_column_deny_not_applied` — disabled policy → `denied_columns` is empty.
- Unit: `engine::tests::test_enabled_policy_column_deny_applied` — enabled policy → `denied_columns` contains `ssn`.
- Manual: Disable a policy with `column_access deny`. Without reconnecting, verify `ssn` reappears in `information_schema.columns` on the next query (policy changes trigger an immediate `SessionContext` rebuild for all active connections).

---

### 13. Column mask had no effect — original values returned

**Vector**: Admin creates a `column_mask` obligation expecting `ssn` values to be masked (e.g. `'***-**-' || RIGHT(ssn, 4)`). Data is queried and original SSN values are returned as-is.

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

**Vector**: Two permit policies both have `row_filter` obligations on the same table with different conditions (e.g. Policy A: `org_id = 'acme'`, Policy B: `status = 'active'`). A user assigned both policies can see ALL rows matching either condition — including rows from other tenants or inactive records that neither policy alone intended to expose.

**Bug**: In `ObligationEffects::collect()`, cross-policy row filters were combined with OR semantics (seed `lit(false)`, combinator `.or()`). The intent was "any permit match grants access", but this allows a user assigned multiple narrow policies to see the union of all their allowed sets — potentially broader than any single policy intended.

**Defense**: Cross-policy row filters are now combined with AND semantics (seed `lit(true)`, combinator `.and()`). Each permit policy adds a restriction; users see the intersection. Within a single policy, multiple `row_filter` obligations are still AND'd (unchanged). Deny policies are unaffected — the deny short-circuit on first match is equivalent to OR across denies.

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

### 16. `column_mask` obligation accepted on a `deny`-effect policy

**Vector**: Admin creates a `deny`-effect policy with a `column_mask` obligation, expecting the column to be masked. Because `PolicyHook` only applies `column_mask` from permit policies, the mask silently has no effect — the column's real value is returned.

**Defense**: `validate_no_deny_column_mask()` in `policy_handlers.rs` is called in both `create_policy` and `update_policy`. If `effect = "deny"` and any obligation has `obligation_type = "column_mask"`, the API returns HTTP `422 Unprocessable Entity` before the record is written. The admin UI hides `column_mask` from the obligation type picker when `deny` is selected and auto-removes any existing `column_mask` obligations when the effect is switched to `deny`.

**Test**:
- `create_deny_column_mask_rejected_422` — POST a deny policy with `column_mask` → `422`.
- `update_effect_to_deny_with_existing_column_mask_rejected_422` — create a permit policy with `column_mask`, then PATCH effect to `deny` → `422`.
- `update_obligations_column_mask_on_deny_policy_rejected_422` — create a deny policy, then PATCH obligations to add `column_mask` → `422`.

---

### 17. `object_access deny` — schema hidden at query time

**Vector**: Admin creates an `object_access deny` obligation on schema `analytics`, expecting all tables in that schema to be invisible to the assigned user. Without the implementation, the user can still see and query `analytics.*` tables.

**Defense**: `compute_user_visibility()` in `engine/mod.rs` parses `object_access` obligations and populates `denied_schemas`. `build_user_context()` skips entire schemas that appear in `denied_schemas` when building the user's filtered `SessionContext`. This applies in both `open` and `policy_required` modes.

**Test**:
- Unit: `engine::tests::test_disabled_policy_column_deny_not_applied` (existing) — verify disabled policy does not populate denied sets.
- Integration: Create a deny policy with `object_access { schema: "analytics", action: "deny" }`. Assign to datasource for a test user. Connect as that user and run `SELECT * FROM information_schema.schemata`. Verify `analytics` is absent. Run `SELECT * FROM analytics.reports`. Verify a "schema not found" error (not data rows).

---

### 18. `object_access deny` — table hidden at query time

**Vector**: Admin creates an `object_access deny` obligation on table `public.payments`, expecting that table to be invisible to the assigned user while the rest of `public` remains accessible.

**Defense**: `compute_user_visibility()` populates `denied_tables` with `(df_alias, table_name)` pairs. `build_user_context()` skips tables in `denied_tables` when building the virtual schema, leaving all other tables in the schema visible.

**Test**:
- Integration: Create a deny policy with `object_access { schema: "public", table: "payments", action: "deny" }`. Assign to datasource. Connect as that user and run `SELECT * FROM information_schema.tables WHERE table_schema = 'public'`. Verify `payments` is absent but `orders` and `customers` are present. Run `SELECT * FROM payments`. Verify a "table not found" error.

---

### 19. Glob pattern matching bypassed with unexpected table name

**Vector**: Admin creates a `row_filter` with `table: "raw_*"` intending to filter all tables whose names start with `raw_`. If matching is exact only, tables like `raw_events` and `raw_orders` are not filtered, leaking rows.

**Defense**: `matches_pattern()` in `policy_match.rs` supports prefix glob: if the pattern ends with `*`, it uses `starts_with(prefix)` matching. `matches_schema_table()` delegates to `matches_pattern()` for both schema and table fields. The same function is used by both `PolicyHook` (query-time) and `compute_user_visibility()` (connect-time), ensuring consistent semantics.

**Test**: 14 unit tests in `proxy/src/policy_match.rs` cover exact match, `*` wildcard, prefix glob on table, prefix glob on schema, combined globs, alias resolution, and non-matching cases (`orders_raw` does not match `raw_*`).
