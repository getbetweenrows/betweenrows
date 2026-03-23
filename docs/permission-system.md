# Permission System

BetweenRows has a policy-based permission system that controls what data users can see when querying through the proxy.

## Mental model

Permissions are defined as **policies**. A policy is a named, reusable unit with a single **policy type** that determines what it does to a query. Policies are **assigned** to a datasource, scoped to all users, a specific user, or a role.

When a user runs a query:
1. The proxy loads all **enabled** policies assigned to the datasource for that user.
2. `table_deny` and `column_deny` policies are evaluated first. A `table_deny` match rejects the query with an error. `column_deny` strips specific columns from results.
3. `row_filter`, `column_mask`, and `column_allow` policies rewrite the query in-flight.
4. The rewritten query executes against the upstream database.

## Policy types

Each policy has exactly one `policy_type`, which encodes both the **intent** (permit or deny) and the **mechanism**:

| policy_type | Intent | What it does |
|---|---|---|
| `row_filter` | permit | Injects a `WHERE` clause into queries on matched tables |
| `column_mask` | permit | Replaces a column's value with a masked expression |
| `column_allow` | permit | Allowlists specific columns; all others are hidden |
| `column_deny` | deny | Removes specific columns from results |
| `table_deny` | deny | Blocks access to an entire table; returns an error |

Deny types (`column_deny`, `table_deny`) are evaluated before permit types. There is no separate `effect` field — the type name encodes the intent.

> **`is_enabled` flag**: only enabled policies are enforced. Disabling (or enabling) a policy removes (or adds) all its effects immediately — both for query-time enforcement and for schema visibility — without requiring a reconnect.
>
> Because a policy can be assigned to **multiple datasources**, disabling it drops its effects on **all** of them at once. If you only want to stop a policy from applying to one specific datasource, the correct action is to **remove the policy assignment** for that datasource rather than disabling the policy.

## Policy structure

Every policy has:

- **`name`** — unique, human-readable identifier
- **`policy_type`** — one of the five types above
- **`targets`** — JSON array of target entries specifying which schemas/tables/columns the policy applies to
- **`definition`** — nullable JSON with type-specific logic (only for `row_filter` and `column_mask`)
- **`is_enabled`** — whether the policy is currently active
- **`version`** — incremented on each update (used for optimistic concurrency)

## Targets

The `targets` array specifies where a policy applies. Each entry has:

```json
{
  "schemas": ["public", "reporting"],
  "tables": ["customers", "orders"],
  "columns": ["ssn", "credit_card"]
}
```

- **`schemas`** — array of schema name patterns (supports `"*"` and prefix globs like `"raw_*"`)
- **`tables`** — array of table name patterns (supports `"*"` and prefix globs)
- **`columns`** — array of column name patterns — **required for `column_mask`, `column_allow`, `column_deny`; absent for `row_filter` and `table_deny`**

Multiple target entries in the same policy form a union — the policy applies to any table matched by any entry.

## Policy types in detail

### row_filter

Injects a `WHERE` clause into queries that touch matched tables. The `definition` field must contain a `filter_expression`:

```json
{
  "filter_expression": "organization_id = {user.tenant}"
}
```

Use `"schemas": ["*"]` and/or `"tables": ["*"]` to match all schemas or tables.

`row_filter` policies from **different policies** are **AND**ed together — each policy adds a restriction, and users see the intersection of all matching policies.

### column_mask

Replaces a column's value with a masked expression. The `definition` field must contain a `mask_expression`. Each target entry must specify exactly one column.

```json
{
  "mask_expression": "'***-**-' || RIGHT(ssn, 4)"
}
```

When multiple `column_mask` policies target the same column, the one with the **lowest priority number** (highest precedence) wins.

When a `row_filter` and `column_mask` target the same column, the row filter always evaluates against the **raw** (unmasked) value. Masking is applied after filtering, so filter predicates are never affected by mask expressions.

### column_allow

Acts as a **column allowlist**: only the listed columns are visible in schema metadata and query results. All other columns are hidden. This is the only policy type that makes a table accessible in `policy_required` mode.

No `definition` field — the `targets[].columns` array specifies which columns are permitted.

### column_deny

Acts as a **column denylist**: the listed columns are removed from schema metadata and query results.

No `definition` field — the `targets[].columns` array specifies which columns to remove.

Denied columns from all enabled `column_deny` policies are unioned — if any policy removes a column, it is absent from results regardless of other policies.

If the query selects **only** denied columns (e.g. `SELECT ssn FROM customers`), the proxy returns SQLSTATE `42501` (insufficient privilege) with a message identifying the restricted columns rather than returning empty rows.

Glob patterns are supported in the `columns` field — see [Column glob patterns](#column-glob-patterns-columns-field) below.

### table_deny

Hides an entire table from a user's virtual catalog — it becomes invisible in `information_schema.tables`, SQL client sidebars, and query execution. Querying a denied table returns a "not found" error as if the table does not exist.

`table_deny` applies in **both** `open` and `policy_required` modes. It takes effect immediately when a policy is mutated via the admin API.

No `definition` field. The `targets` array specifies which schema/table combinations to deny. To deny an entire schema, use `"tables": ["*"]`:

```json
"targets": [{ "schemas": ["analytics"], "tables": ["*"] }]
```

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

`"schemas": ["*"]` matches all schemas. `"tables": ["*"]` matches all tables. You can combine them:

```json
{ "schemas": ["*"], "tables": ["*"], "filter_expression": "1=1" }
```

**Prefix glob patterns** (`prefix*`) are also supported for both `schemas` and `tables` entries. A trailing `*` matches any value that starts with the given prefix:

```json
{ "schemas": ["raw_*"], "tables": ["*"] }
```

This matches `raw_events`, `raw_orders`, `raw_customers`, etc.

| Pattern | Matches | Does not match |
|---------|---------|----------------|
| `"*"` | everything | — |
| `"public"` | `public` only | `public2`, `private` |
| `"raw_*"` | `raw_orders`, `raw_events` | `orders_raw`, `orders` |
| `"analytics_*"` | `analytics_dev`, `analytics_prod` | `public`, `raw_analytics` |

Glob support applies to all policy types.

### Column glob patterns (`columns` field)

The `columns` field also supports glob patterns:

| Column pattern | Denies | Keeps |
|----------------|--------|-------|
| `["*"]` | all columns in the matched table | — |
| `["secret_*"]` | `secret_key`, `secret_token` | `email`, `id`, `ssn` |
| `["*_name"]` | `first_name`, `last_name` | `email`, `id`, `created_at` |
| `["ssn"]` | `ssn` only (exact match) | all others |
| `["*_at", "secret_*"]` | `created_at`, `secret_key`, `secret_token` | `email`, `id`, `ssn` |

Both prefix globs (`col_*`) and suffix globs (`*_col`) are supported. Patterns are **case-sensitive**. Glob matching is applied at schema-metadata build time (connect) and at query-time projection (execute), so denied columns are hidden from both `information_schema.columns` and `SELECT` results.

## Zero-trust column model

Column visibility follows a **zero-trust** model in `policy_required` mode: a table is completely inaccessible until a `column_allow` policy explicitly grants access. `row_filter` and `column_mask` policies transform data but do **not** grant access by themselves.

### Policy type responsibility matrix

| policy_type | Grants table access? | Grants column access? | Modifies data? |
|---|---|---|---|
| `row_filter` | **No** | No | Yes (filters rows) |
| `column_mask` | **No** | No | Yes (transforms value) |
| `column_allow` | **Yes** | Yes (named columns only) | No |
| `column_deny` | **No** — does not unblock a table | Removes named columns | No |
| `table_deny` | Removes table from catalog | Removes all | No |

### `column_allow` — the access grant policy type

`column_allow` is the **only** policy type that makes a table visible in `policy_required` mode. It specifies which columns the user can see via the `targets[].columns` array:

```json
{
  "policy_type": "column_allow",
  "targets": [{ "schemas": ["public"], "tables": ["customers"], "columns": ["id", "name", "email"] }]
}
```

With only this policy, the user sees the `customers` table with exactly three columns. Any column not in the `columns` list is invisible in both schema metadata and query results.

### Composing access with row filters

`column_allow` and `row_filter` policies stack correctly — use two separate policies:

```json
[
  { "policy_type": "column_allow", "targets": [{ "schemas": ["public"], "tables": ["customers"], "columns": ["id", "name"] }] },
  { "policy_type": "row_filter",   "targets": [{ "schemas": ["public"], "tables": ["customers"] }], "definition": { "filter_expression": "organization_id = {user.tenant}" } }
]
```

Result: only `id` and `name` columns, filtered to the user's tenant rows.

### `column_deny` does not grant access

In `policy_required` mode, a `column_deny` policy does **not** unblock the table. The table remains blocked by `lit(false)`. Use `column_allow` first to grant access, then add `column_deny` policies to strip specific columns.

In `open` mode, `column_deny` removes the specified columns from results regardless.

### JOIN column scoping

Column allow/deny/mask policies are scoped to their source table. Denying `email` on `customers` does **not** affect `email` on `orders` in the same JOIN:

```sql
-- With column_deny on customers.email, this correctly returns:
-- customers.id, customers.name, orders.id, orders.email, orders.total
SELECT * FROM customers JOIN orders ON customers.id = orders.customer_id
```

Column qualifiers from DataFusion's query planner identify which table each output column originated from, ensuring column policies apply only to their intended table.

## Known limitations

### `table_deny` uses the upstream (source) schema name, not the alias

If a schema has been aliased in the datasource configuration, the `table_deny` target must use the original upstream schema name — not the display alias. Using the alias will silently fail to deny access.

## Priority and conflict resolution

Each policy assignment has a `priority` (integer, lower = higher precedence, default 100).

| Situation | Resolution |
|---|---|
| Multiple `row_filter` policies, same table | Filters are AND'd (intersection) |
| Multiple `column_mask` policies, same column | Lowest priority number wins |
| `column_deny` from any enabled policy | Column is always removed |
| Equal priority, user-specific vs wildcard | User-specific assignment wins |

## Policy design guidelines

### One policy, one type, one purpose

Each policy has exactly one `policy_type`. To apply different types of controls to the same user, assign multiple policies:

| Goal | Policies |
|---|---|
| Tenant isolation | One `row_filter` policy |
| Mask SSN for support staff | One `column_mask` policy |
| Tenant isolation + mask SSN | Two policies (one each) |
| Column allowlist + row filter | Two policies (`column_allow` + `row_filter`) |
| Hide the `analytics` schema | One `table_deny` policy |
| Remove sensitive columns | One `column_deny` policy |

### Practical heuristics

Favor **smaller, composable policies** over monolithic ones. Your system supports policy assignment with priority, so you can layer policies. This makes it easier to debug ("why can't user X see this?") when each policy has a clear, narrow purpose.

Start with simple policies and split them when they become hard to reason about. A policy assigned to multiple datasources has the same effect on all of them.

## Access mode

Each datasource has an `access_mode`:

- **open** (default) — behaves as if an implicit "allow all" policy exists. Tables are accessible even without an explicit `column_allow` policy. However, deny policies are always enforced: `table_deny` rejects queries, `column_deny` strips columns. Think of it as "default allow, explicit deny." Useful for development datasources.
- **policy_required** — explicit grant only. Tables with no matching `column_allow` policy return empty results and are hidden from schema metadata. Deny policies apply on top. Think of it as "default deny, explicit grant." Use this in production to ensure no data is accessible without an intentional policy.

> **Note:** BetweenRows is an explicit-access-policy system. `open` mode is a convenience for lower environments when you want to give users quick access without managing policies upfront — it does not disable the policy engine. For production, always use `policy_required`.

## Visibility follows access

What a user can see in schema metadata mirrors exactly what they can query. This principle applies at two levels:

- **Table visibility** — in `policy_required` mode, tables without a matching `column_allow` policy (or blocked by `table_deny`) are hidden from `information_schema.tables` and do not appear in schema introspection.
- **Column visibility** — columns denied via `column_deny` are hidden from `information_schema.columns` on the user's connection, not just stripped from query results. This prevents users from discovering the existence of sensitive columns.

Schema metadata is never a leakage vector: if a user cannot query it, they cannot see it. Toggling `is_enabled` on a policy updates both query-time enforcement and schema visibility immediately — no reconnect required.

**Access mode impact on visibility:**
- `open`: all tables are visible in metadata; only `column_deny` policies affect column visibility (and `table_deny` removes specific tables)
- `policy_required`: only tables referenced by a matching `column_allow` policy appear; denied columns are also stripped

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

A cached, per-datasource snapshot of the upstream schema (tables, columns, Arrow types). Shared across all connections to the same datasource. Rebuilt on catalog re-discovery, not per-query.

### 2. Per-user virtual schema

Derived at connect time by applying policies to the baseline catalog:

1. Load all policy assignments for this datasource + user
2. In `policy_required` mode: only tables referenced by a `column_allow` policy are included
3. Tables blocked by `table_deny` are excluded
4. Denied columns from any enabled `column_deny` policy are stripped from table schemas
5. A filtered `SessionContext` is built with only the visible tables and columns

### 3. Live updates

When a policy is mutated (create, update, delete, enable/disable) via the admin API:

- The PolicyHook's cached session data is invalidated (query-time enforcement)
- All active connections on the affected datasource have their SessionContexts rebuilt in the background (schema visibility)
- Both layers update together — no reconnect required, no stale window
- Rebuilds happen concurrently per-connection; failures log a warning but do not disconnect users

## Roles (RBAC)

Roles provide a named grouping layer for managing policy assignments and datasource access at scale. Instead of assigning policies to individual users, assign them to a role and add users to the role.

### Role basics

- **Global scope** — roles are not scoped to datasources. Scoping happens at the assignment level.
- **Soft delete** — roles have an `is_active` flag. Deactivated roles are excluded from policy/access resolution but remain visible in the admin API for reactivation.
- **Name validation** — 3-50 characters, starts with a letter, only `[a-zA-Z0-9_.-]`.

### Role hierarchy (DAG)

Roles support a directed acyclic graph (DAG) hierarchy via parent-child relationships:

- A child role inherits all policy assignments from its parent roles.
- Multiple parents are allowed (diamond inheritance).
- Maximum depth: 10 levels.
- Cycle detection, depth check, and insertion are wrapped in a single transaction — the API rejects any inheritance edge that would create a cycle or exceed the depth limit.
- SQLite's single-writer serialization provides additional protection against concurrent race-condition cycles.

Example:
```
finance-analyst ─── inherits from ─── finance
                └── inherits from ─── analyst
```
Users in `finance-analyst` get policies from both `finance` and `analyst`.

### Assignment scopes

Policy assignments now have an `assignment_scope` field:

| Scope | Target field | Meaning |
|-------|-------------|---------|
| `user` | `user_id` | Applies to a specific user |
| `role` | `role_id` | Applies to all members of a role (including inherited members) |
| `all` | neither | Applies to all users on the datasource |

The old convention of NULL `user_id` meaning "all users" is preserved via backfill migration.

### Datasource access

The `data_source_access` table (replacing `user_data_source`) supports the same three scopes:
- **User-scoped**: direct user access to a datasource (managed via User Access panel on datasource edit page)
- **Role-scoped**: all members of a role can connect to the datasource (managed via Role Access panel on datasource edit page). Only active roles can be granted access — inactive roles are rejected.
- **All-scoped**: everyone can connect

The role edit page shows a "Data Sources" tab listing all datasource access (both direct and inherited from parent roles).

### Connection-time access check

When a user connects to a datasource via the PostgreSQL wire protocol, the proxy runs `check_access(user_id, datasource_name)` during the startup handshake. This calls `resolve_datasource_access()` in `role_resolver.rs`, which checks the `data_source_access` table for any matching entry across all three scopes:

1. `scope = 'all'` AND `data_source_id` matches → access granted
2. `scope = 'user'` AND `user_id` matches → access granted
3. `scope = 'role'` AND `role_id` is in the user's resolved roles (via `resolve_user_roles()` BFS) → access granted

If no matching entry is found, the connection is rejected with a "not assigned to this data source" error. This check runs before `build_user_context()` — a user cannot connect at all without at least one matching `data_source_access` entry.

### Priority and deduplication

- **Unified priority**: the assignment's stated priority is used regardless of whether it comes from a direct assignment, role assignment, or inherited role.
- **Deduplication**: if the same policy is assigned via multiple paths (e.g., directly and via a role), only the lowest priority (highest precedence) assignment is used.

### Deny always wins

`column_deny` and `table_deny` policies cannot be overridden by `column_allow` from another role. If any path (direct or inherited) applies a deny policy, it takes effect regardless of allow policies from other sources.

### Template variables resolve from the user

Template variables (`{user.tenant}`, `{user.username}`, `{user.id}`) always resolve from the connecting user's attributes, not the role. A `row_filter` policy with `{user.tenant}` assigned to a role will filter by each member's individual tenant.

### Immediate effect

Role changes take effect immediately for active connections:
- **Member add/remove**: the affected user's session context is rebuilt in the background.
- **Inheritance add/remove**: all users in the child subtree have their contexts rebuilt.
- **Role deactivate/reactivate**: all direct and inherited members are affected.
- **Role delete**: all members lose role-granted policies immediately (cascade delete on FK).

### Effective members

The role detail endpoint and the Members tab show **effective members** — users who are direct members of the role plus users who are members of child roles (inherited via the role hierarchy). Each member is annotated with a source:
- `"direct"` — the user is a direct member of this role
- `"via role '<name>'"` — the user is a member of the named child role

Only direct members can be removed from the Members tab. Inherited members must be removed from their source role.

`GET /roles/{id}/effective-members` returns the full effective member list with source annotations.

### Effective policy preview

`GET /users/{id}/effective-policies?datasource_id=X` returns all policies that apply to a user on a given datasource, annotated with the source (direct, role name, or inherited role name).

### Admin audit log

All admin mutations (roles, users, policies, datasources) are recorded in the `admin_audit_log` table. This is an append-only table — no UPDATE or DELETE endpoints are exposed. Each entry records the resource type, resource ID, action, actor, and a JSON `changes` field with before/after snapshots.

**Atomicity:** All admin mutation handlers use `AuditedTxn` (from `admin_audit.rs`), a wrapper around `DatabaseTransaction` that queues audit entries and writes them atomically on `commit()`. This makes it structurally impossible to commit a mutation without its audit entry, or to write an audit entry outside the transaction boundary. `AuditedTxn::commit()` errors if no audit entries were queued, preventing accidentally unaudited transactions.

The admin UI provides two views into the audit log:
- **Centralized Admin Audit page** (`/admin-audit`) — filterable by resource type, actor ID, and date range. Shows all admin mutations across the system.
- **Per-entity Activity tabs/sections** — embedded on role, user, policy, and datasource edit pages using the `AuditTimeline` component. Shows audit entries scoped to that specific entity.

### Example: role-based access

```yaml
# Create roles
POST /roles { "name": "finance-analyst" }
POST /roles { "name": "finance" }

# Set up inheritance
POST /roles/{finance-analyst-id}/parents { "parent_role_id": "{finance-id}" }

# Grant datasource access to role
PUT /datasources/{ds-id}/access/roles { "role_ids": ["{finance-id}"] }

# Assign policy to role
POST /datasources/{ds-id}/policies {
  "policy_id": "{tenant-filter-id}",
  "role_id": "{finance-id}",
  "scope": "role",
  "priority": 100
}

# Add user to role
POST /roles/{finance-analyst-id}/members { "user_ids": ["{alice-id}"] }
```

Alice now has datasource access (via `finance` inheritance) and the tenant-filter policy.

## Management vs. data permissions

`is_admin` controls admin API and UI access only — it is a management plane concern. Data plane access (querying through the proxy) requires explicit policy assignment to a specific datasource.

An admin with no policy assignments sees **zero data** through the proxy — this is by design.

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
    policy_type: row_filter
    description: Filter rows by tenant
    is_enabled: true
    targets:
      - schemas: ["*"]
        tables: ["*"]
    definition:
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
  policy_type: row_filter
  targets:
    - schemas: ["*"]
      tables: [orders]
  definition:
    filter_expression: "organization_id = {user.tenant}"
```

### Column masking (DS-01)
Partially mask SSNs for support staff:
```yaml
- name: mask-ssn
  policy_type: column_mask
  targets:
    - schemas: [public]
      tables: [customers]
      columns: [ssn]
  definition:
    mask_expression: "'***-**-' || RIGHT(ssn, 4)"
```

### Column access control (DS-10)
Remove sensitive columns entirely:
```yaml
- name: hide-pii
  policy_type: column_deny
  targets:
    - schemas: [public]
      tables: [customers]
      columns: [ssn, credit_card]
```

### Column allowlist (DS-11)
Expose only specific columns to support agents:
```yaml
- name: support-read-customers
  policy_type: column_allow
  targets:
    - schemas: [public]
      tables: [customers]
      columns: [id, name, email, status]
```

### Hide a table from a user (DS-15)
Hide the `payments` table from users who only need `orders` and `customers`:
```yaml
- name: hide-payments-table
  policy_type: table_deny
  targets:
    - schemas: [public]
      tables: [payments]
```

### Hide a schema from a user (DS-14)
Hide the entire `analytics` schema from external partners:
```yaml
- name: hide-analytics-schema
  policy_type: table_deny
  targets:
    - schemas: [analytics]
      tables: ["*"]
```

### Admin full access (MT-05)
Admin gets unrestricted access to all tables (via `column_allow` for `policy_required` mode):
```yaml
- name: admin-full-access
  policy_type: column_allow
  targets:
    - schemas: ["*"]
      tables: ["*"]
      columns: ["*"]
```
Assign this policy to the datasource with `user: admin_username`.

### Policy-required lockdown (CC-01)
Set `access_mode: "policy_required"` on the datasource. Without an assigned `column_allow` policy, all tables return empty results.
