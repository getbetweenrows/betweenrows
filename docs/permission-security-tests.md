# Permission System — Security Tests

This document lists attack vectors and corresponding integration tests for the permission system.

## Test environment

Integration tests live in `proxy/tests/`. They require a PostgreSQL instance and a configured datasource. Set `TEST_DATABASE_URL` to a test Postgres database.

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

## Planned integration test file

`proxy/tests/policy_enforcement.rs`:

```rust
#[tokio::test]
async fn row_filter_tenant_isolation() { ... }

#[tokio::test]
async fn row_filter_template_variable_injection_safe() { ... }

#[tokio::test]
async fn row_filter_bypasses_table_alias() { ... }

#[tokio::test]
async fn row_filter_bypasses_cte() { ... }

#[tokio::test]
async fn column_access_deny_star_expansion() { ... }

#[tokio::test]
async fn column_mask_direct_column_reference() { ... }

#[tokio::test]
async fn join_both_tables_filtered() { ... }

#[tokio::test]
async fn policy_required_no_policy_returns_empty() { ... }
```

---

### 9. `column_access deny` on deny-effect policies ignored at query time

**Vector**: Admin creates a **deny-effect** policy with a `column_access deny` obligation on `ssn`, expecting the column to be stripped from query results immediately.

**Bug**: `PolicyHook` only processed `column_access` obligations from *permit* policies (the loop at `handle_query` iterated over `session.permit_policies`). Deny-effect policies were only checked for table-level "Access denied" errors. So `column_access deny` on a deny policy had no query-time effect — the column appeared in results until the user reconnected (when `compute_user_visibility()` hid it from the schema).

**Defense**: `PolicyHook::handle_query` now runs a second loop over `session.deny_policies` after the permit loop, processing only `column_access` obligations and adding matched columns to `column_denies`.

**Test**: Create a deny-effect policy with `column_access deny` on `ssn`. Assign to datasource. Without reconnecting, run `SELECT ssn FROM employees`. Verify `ssn` column is absent from the result set.

---

### 10. Disabled policies still enforced in visibility layer

**Vector**: Admin disables a policy with `column_access deny`, expecting the column to reappear in `information_schema.columns` on next reconnect.

**Bug**: `compute_user_visibility()` loaded obligations for ALL assigned policy IDs, including those belonging to disabled policies. The `column_access deny` block didn't check if the parent policy was enabled, so disabling a policy had no effect on schema visibility.

**Defense**: `compute_user_visibility()` now loads obligations only for *enabled* policy IDs (those from the `is_enabled = true` filtered query). Disabled policies contribute neither to `visible_tables` nor `denied_columns`.

**Test**:
- Unit: `engine::tests::test_disabled_policy_column_deny_not_applied` — disabled policy → `denied_columns` is empty.
- Unit: `engine::tests::test_enabled_policy_column_deny_applied` — enabled policy → `denied_columns` contains `ssn`.
- Manual: Disable a policy with `column_access deny`. Without reconnecting, verify `ssn` reappears in `information_schema.columns` on the next query (policy changes trigger an immediate `SessionContext` rebuild for all active connections).

---

### 11. `SELECT <denied-column>` returns silent empty rows instead of an error

**Vector**: User runs `SELECT ssn FROM customers` where `ssn` is denied. They receive many rows with empty/null values and incorrectly conclude the column is empty in the database.

**Bug**: When all selected columns were stripped by `column_access deny`, `new_exprs` became empty. `LogicalPlanBuilder::project([])` produced a zero-column projection that DataFusion executed successfully — returning N rows with no column data. Clients rendered this as empty rows.

**Defense**: `PolicyHook` now checks for an empty `new_exprs` after column stripping and returns SQLSTATE `42501` (insufficient_privilege) listing the denied columns, before attempting to build the projection.

**Test**: Create a policy with `column_access deny` on `ssn`. Run `SELECT ssn FROM customers`. Verify the response is an error with SQLSTATE `42501` and not an empty result set.
