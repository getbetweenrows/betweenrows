# Permission System

BetweenRows has a policy-based permission system that controls what data users can see when querying through the proxy.

## Mental model

Permissions are defined as **policies**. A policy is a named, reusable unit that contains one or more **obligations** — rules that specify what transformation to apply to a query. Policies are then **assigned** to a datasource (optionally scoped to a specific user).

When a user runs a query:
1. The proxy loads all **enabled** policies assigned to the datasource for that user.
2. `deny` policies are evaluated first. A `row_filter` match on a deny policy rejects the query with an error. `column_access deny` obligations on deny policies are collected alongside those from permit policies (step 3).
3. `permit` policies are applied — their obligations rewrite the query in-flight (row filters, column masks, column access controls). Column denies from both permit and deny policies are combined.
4. The rewritten query executes against the upstream database.

## Policy effects

- **permit** — applies obligations to the query (row filtering, masking, etc.)
- **deny** — primarily used to block access with an error. Can also carry `column_access deny` obligations to strip specific columns from results.

Deny policies are evaluated before permit policies. If a deny policy has a matching `row_filter` obligation, the query is rejected immediately with an error. `column_access deny` obligations on deny policies strip columns from results just like they do on permit policies — they do not cause an error.

> **`is_enabled` flag**: only enabled policies are enforced. Disabling (or enabling) a policy removes (or adds) all its effects immediately — both for query-time enforcement and for schema visibility — without requiring a reconnect.
>
> Because a policy can be assigned to **multiple datasources**, disabling it drops its effects on **all** of them at once. If you only want to stop a policy from applying to one specific datasource, the correct action is to **remove the policy assignment** for that datasource rather than disabling the policy — removing an assignment leaves the policy intact and active on any other datasources it is assigned to.

## Obligation types

### row_filter

Injects a `WHERE` clause into queries that touch the specified table.

```json
{
  "schema": "public",
  "table": "orders",
  "filter_expression": "organization_id = {user.tenant}"
}
```

Use `"schema": "*"` and/or `"table": "*"` to match all schemas or tables.

Multiple `row_filter` obligations on the **same policy** targeting the same table are **AND**ed together.
`row_filter` obligations from **different permit policies** are also **AND**ed together — each policy adds a restriction, and users see the intersection of all matching policies.

### column_mask

Replaces a column's value with a masked expression in query results.

```json
{
  "schema": "public",
  "table": "customers",
  "column": "ssn",
  "mask_expression": "'***-**-' || RIGHT(ssn, 4)"
}
```

When multiple mask policies target the same column, the one with the **lowest priority number** (highest precedence) wins.

### column_access (deny)

Removes the specified columns from query results entirely.

```json
{
  "schema": "public",
  "table": "customers",
  "columns": ["ssn", "credit_card"],
  "action": "deny"
}
```

Denied columns are unioned across all matching **enabled** policies, regardless of effect — if any enabled policy (permit or deny) denies a column, it is removed from the result. The column is also hidden from schema metadata (`information_schema.columns`) on the user's connection.

If the query selects **only** denied columns (e.g. `SELECT ssn FROM customers`), the proxy returns SQLSTATE `42501` (insufficient privilege) with a message identifying the restricted columns rather than returning empty rows.

### object_access (deny)

Hides entire schemas or individual tables from a user's virtual catalog — they become invisible in `information_schema.schemata`/`information_schema.tables`, SQL client sidebars, and query execution.

**Schema-level deny** — omit `table` or set it to `"*"`:

```json
{
  "schema": "analytics",
  "action": "deny"
}
```

**Table-level deny** — specify both `schema` and `table`:

```json
{
  "schema": "public",
  "table": "payments",
  "action": "deny"
}
```

`object_access` deny obligations are enforced in **both** `open` and `policy_required` modes. They are applied at connect time when building the user's virtual `SessionContext`, and take effect immediately (without reconnect) when a policy is mutated via the admin API.

Unlike `column_access deny` (which strips columns from results), `object_access deny` hides the schema or table entirely — queries against a denied schema/table return a "not found" error as if the object does not exist.

> **`column_mask` on deny policies is invalid.** The API returns `422 Unprocessable Entity` if you attempt to create or update a `deny`-effect policy with a `column_mask` obligation. The UI hides `column_mask` from the available obligation types when `deny` is selected. Only `column_access` and `object_access` obligations are supported on deny policies.

## Template variables

Filter and mask expressions can reference the authenticated user's attributes:

| Placeholder | Value |
|---|---|
| `{user.tenant}` | The user's tenant string |
| `{user.username}` | The user's username |
| `{user.id}` | The user's UUID |

The proxy uses a **parse-then-substitute** pattern: the expression is parsed into a DataFusion expression tree first, then placeholder identifiers are replaced with typed literal values. The user's tenant/username never passes through the SQL parser, making this immune to SQL injection even if the tenant string contains SQL syntax.

Example:
```
organization_id = {user.tenant}
```
becomes (at query time, for a user with tenant `acme`):
```
organization_id = 'acme'
```

## Wildcards and glob patterns

`"schema": "*"` matches all schemas. `"table": "*"` matches all tables. You can combine them:

```json
{ "schema": "*", "table": "*", "filter_expression": "1=1" }
```

This would apply to every table in every schema in the datasource. Useful for full-access policies assigned to admins.

**Prefix glob patterns** (`prefix*`) are also supported for both `schema` and `table` fields. A trailing `*` matches any value that starts with the given prefix:

```json
{ "schema": "raw_*", "table": "*", "filter_expression": "1=1" }
```

This matches `raw_events`, `raw_orders`, `raw_customers`, etc. Useful for naming-convention-based policies without listing every table individually.

| Pattern | Matches | Does not match |
|---------|---------|----------------|
| `"*"` | everything | — |
| `"public"` | `public` only | `public2`, `private` |
| `"raw_*"` | `raw_orders`, `raw_events` | `orders_raw`, `orders` |
| `"analytics_*"` | `analytics_dev`, `analytics_prod` | `public`, `raw_analytics` |

Glob support applies to all obligation types: `row_filter`, `column_mask`, `column_access`, and `object_access`.

### Column glob patterns (`columns` field)

The `columns` field of `column_access` obligations also supports glob patterns:

| Column pattern | Denies | Keeps |
|----------------|--------|-------|
| `["*"]` | all columns in the matched table | — |
| `["secret_*"]` | `secret_key`, `secret_token` | `email`, `id`, `ssn` |
| `["*_name"]` | `first_name`, `last_name` | `email`, `id`, `created_at` |
| `["ssn"]` | `ssn` only (exact match) | all others |
| `["*_at", "secret_*"]` | `created_at`, `secret_key`, `secret_token` | `email`, `id`, `ssn` |

Both prefix globs (`col_*`) and suffix globs (`*_col`) are supported for column names. Patterns are **case-sensitive** — this matches PostgreSQL's default behaviour of folding identifiers to lowercase. Glob matching for columns is applied at schema-metadata build time (connect) and at query-time projection (execute), so denied columns are hidden from both `information_schema.columns` and `SELECT` results.

## Known limitations

### Column deny is not table-qualified at query time

`column_access deny` obligations identify denied columns by **name only**, not by `schema.table.column`. In a query that JOINs two tables both containing a column named `id`, a deny on `id` in `table_a` will also strip `id` from `table_b` in the same result set.

The column is correctly hidden from schema metadata (`information_schema.columns`) on a per-table basis at connect time. Query-time stripping in `SELECT *` or explicit projections is name-based across the full projection.

*Workaround:* use more specific column names to avoid collisions (e.g. `orders_id` instead of `id`), or restrict access at the table level with `object_access deny` when full table hiding is needed.

### `object_access deny` uses the upstream (source) schema name, not the alias

If a schema has been aliased in the datasource configuration, the `object_access deny` obligation must use the original upstream schema name — not the display alias. Using the alias will silently fail to deny access.

## Join-based row filters

For tables that don't directly contain a tenant column, use `join_through` to filter via a parent table:

```json
{
  "schema": "public",
  "table": "order_items",
  "filter_expression": "organization_id = {user.tenant}",
  "join_through": {
    "schema": "public",
    "table": "orders",
    "local_key": "order_id",
    "foreign_key": "id"
  }
}
```

The proxy injects a semi-join: `order_items` is filtered to rows where the related `orders.organization_id` matches the tenant. Only `order_items` columns are returned.

## Priority and conflict resolution

Each policy assignment has a `priority` (integer, lower = higher precedence, default 100).

| Situation | Resolution |
|---|---|
| Multiple permit policies, same table | Row filters are AND'd (intersection) |
| Multiple column masks, same column | Lowest priority number wins |
| column_access deny from any enabled policy (permit or deny) | Column is always removed |
| Equal priority, user-specific vs wildcard | User-specific assignment wins |

## Policy design guidelines

### When to create a new policy

- The rules serve different **roles or use cases** (e.g., "admin access", "support read-only")
- You need to **mix/match** these rules across different datasources
- The rules have **different effects** (mixing permit and deny in one policy gets confusing)

### When to add obligations to existing policy

- The rules are **tightly related** to the same purpose
- They always need to **apply together**
- They're for the **same role or user type**

### Practical heuristics

| Scenario | Recommendation |
|----------|----------------|
| "Admins can see everything" | Single policy with multiple obligations |
| "Support can read, but mask SSN" | Two obligations (row_filter + column_mask) in one policy |
| "Finance can see costs, others can't" | Separate policy for cost visibility rules |
| Same rule needed on multiple datasources | Likely a separate policy to reuse |

### General advice

Favor **smaller, composable policies** over monolithic ones. Your system supports policy assignment with priority, so you can layer policies. This makes it easier to debug ("why can't user X see this?") when each policy has a clear, narrow purpose.

Start with simple policies and split them when they become hard to reason about.

## Access mode

Each datasource has an `access_mode`:

- **open** (default) — behaves as if an implicit "allow all" permit policy exists. Tables are accessible even without an explicit permit policy. However, deny policies are always enforced on top: a `row_filter` on a deny policy rejects the query; `column_access deny` strips columns. Think of it as "default allow, explicit deny." Useful for development datasources.
- **policy_required** — explicit grant only. Tables with no matching permit policy return empty results and are hidden from schema metadata. Deny policies apply on top. Think of it as "default deny, explicit grant." Use this in production to ensure no data is accessible without an intentional policy.

> **Note:** BetweenRows is an explicit-access-policy system. `open` mode is a convenience for lower environments (dev, staging) when you want to give users quick access without managing policies upfront — it does not disable the policy engine. Deny policies, column masks, and row filters from permit policies all still apply. For production, always use `policy_required` to ensure no data is accessible without an intentional policy.

## Visibility follows access

What a user can see in schema metadata mirrors exactly what they can query. This principle applies at two levels:

- **Table visibility** — in `policy_required` mode, tables without a matching permit policy are hidden from `information_schema.tables` and do not appear in schema introspection. A user cannot discover a table they cannot access.
- **Column visibility** — columns denied via `column_access deny` are hidden from `information_schema.columns` on the user's connection, not just stripped from query results. This prevents users from discovering the existence of sensitive columns.

This means schema metadata is never a leakage vector: if a user cannot query it, they cannot see it. Toggling `is_enabled` on a policy updates both query-time enforcement and schema visibility immediately — no reconnect required.

**Access mode impact on visibility:**
- `open`: all tables are visible in metadata; only `column_access deny` obligations affect column visibility
- `policy_required`: only tables referenced by a matching permit obligation appear; denied columns are also stripped

## Virtual schema architecture

The proxy uses a two-layer design to serve each user a schema that exactly matches their access rights.

```
Upstream DB → [discover] → Baseline Catalog (cached, shared)
                                    ↓
                          User connects + policies
                                    ↓
                          Per-user virtual schema (filtered)
                                    ↓
                          SessionContext (per-connection)
```

### 1. Baseline catalog

A cached, per-datasource snapshot of the upstream schema (tables, columns, Arrow types). Shared across all connections to the same datasource. Rebuilt on catalog re-discovery, not per-query. This keeps connect-time latency low — policy filtering operates on the cached catalog, not on live upstream queries.

### 2. Per-user virtual schema

Derived at connect time by applying policies to the baseline catalog:

1. Load all policy assignments for this datasource + user
2. In `policy_required` mode: only tables referenced by a permit obligation are included
3. Denied columns from any enabled policy are stripped from table schemas
4. A filtered `SessionContext` is built with only the visible tables and columns

Each connection gets its own context; the baseline catalog and connection pool are shared.

### 3. Live updates

When a policy is mutated (create, update, delete, enable/disable) via the admin API:

- The PolicyHook's cached obligations are invalidated (query-time enforcement)
- All active connections on the affected datasource have their SessionContexts rebuilt in the background (schema visibility)
- Both layers update together — no reconnect required, no stale window
- Disabling a policy removes all its effects immediately; re-enabling restores them
- Rebuilds happen concurrently per-connection; failures log a warning but do not disconnect users

## Management vs. data permissions

`is_admin` controls admin API and UI access only — it is a management plane concern. Data plane access (querying through the proxy) requires explicit policy assignment to a specific datasource.

An admin with no policy assignments sees **zero data** through the proxy — this is by design. This prevents the common pitfall where admin accounts have implicit god-mode data access.

Even creating a datasource as an admin does not grant you query access to it — you must assign a policy. This separation ensures that management operations (configuring the proxy, managing users) are completely decoupled from data access decisions.

## YAML policy-as-code

Policies can be exported and imported as YAML for version control and reproducible deployments.

### Export

```bash
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:5435/api/v1/policies/export > policies.yaml
```

### Import

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: text/plain" \
  --data-binary @policies.yaml \
  http://localhost:5435/api/v1/policies/import
```

### Dry run (preview changes without applying)

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: text/plain" \
  --data-binary @policies.yaml \
  "http://localhost:5435/api/v1/policies/import?dry_run=true"
```

### YAML format

```yaml
version: 1
policies:
  - name: tenant-isolation
    effect: permit
    description: Filter rows by tenant
    is_enabled: true
    obligations:
      - type: row_filter
        schema: "*"
        table: "*"
        filter_expression: "organization_id = {user.tenant}"
    assignments:
      - datasource: production
        priority: 100
      - datasource: staging
        user: alice
        priority: 50
```

Assignments reference datasources and users by **name**. During import, names are resolved to IDs. If a datasource or user is not found, the policy import for that entry fails with an error.

## Example scenarios

### Tenant isolation (MT-01)
Users only see rows belonging to their organization:
```yaml
- name: tenant-isolation
  effect: permit
  obligations:
    - type: row_filter
      schema: "*"
      table: orders
      filter_expression: "organization_id = {user.tenant}"
```

### Column masking (DS-01)
Partially mask SSNs for support staff:
```yaml
- name: mask-ssn
  effect: permit
  obligations:
    - type: column_mask
      schema: public
      table: customers
      column: ssn
      mask_expression: "'***-**-' || RIGHT(ssn, 4)"
```

### Column access control (DS-10)
Remove sensitive columns entirely:
```yaml
- name: hide-pii
  effect: permit
  obligations:
    - type: column_access
      schema: public
      table: customers
      columns: [ssn, credit_card]
      action: deny
```

### Admin full access (MT-05)
Admin gets unrestricted access to all tables:
```yaml
- name: admin-full-access
  effect: permit
  obligations:
    - type: row_filter
      schema: "*"
      table: "*"
      filter_expression: "1=1"
```
Assign this policy to the datasource with `user: admin_username`.

### Hide a schema from a user (DS-14)
Hide the `analytics` schema entirely from external partners while leaving other schemas accessible:
```yaml
- name: hide-analytics-schema
  effect: deny
  obligations:
    - type: object_access
      schema: analytics
      action: deny
```

### Hide a table from a user (DS-15)
Hide the `payments` table from support agents who only need `orders` and `customers`:
```yaml
- name: hide-payments-table
  effect: deny
  obligations:
    - type: object_access
      schema: public
      table: payments
      action: deny
```

### Policy-required lockdown (CC-01)
Set `access_mode: "policy_required"` on the datasource. Without an assigned permit policy, all tables return empty results.
