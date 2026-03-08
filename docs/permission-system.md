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
`row_filter` obligations from **different permit policies** are **OR**ed together — any permit match grants access.

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

## Wildcards

`"schema": "*"` matches all schemas. `"table": "*"` matches all tables. You can combine them:

```json
{ "schema": "*", "table": "*", "filter_expression": "1=1" }
```

This would apply to every table in every schema in the datasource. Useful for full-access policies assigned to admins.

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
| Multiple permit policies, same table | Row filters are OR'd |
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

- **open** (default) — queries with no matching policies execute without restriction. Useful for development datasources.
- **policy_required** — tables with no matching permit policy return empty results. Use this in production to ensure no data is accessible without an explicit policy.

## Management vs. data permissions

`is_admin` grants access to the admin API and UI only. It does **not** grant any data access through the proxy. An admin who needs to query data must have an explicit policy assigned, just like any other user.

This is intentional: separating management and data permissions prevents accidental data exposure through admin accounts.

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

### Policy-required lockdown (CC-01)
Set `access_mode: "policy_required"` on the datasource. Without an assigned permit policy, all tables return empty results.
