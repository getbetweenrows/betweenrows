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
- **`decision_function_id`** — optional FK to a `decision_function` entity; when set, the decision function gates whether the policy fires for each query

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

Filter and mask expressions can reference the authenticated user's identity and custom attributes:

### Built-in variables

| Placeholder | Value | Type |
|---|---|---|
| `{user.username}` | The user's username | string |
| `{user.id}` | The user's UUID | string |

### Custom attribute variables

Any attribute defined via Attribute Definitions (see below) can be referenced as `{user.KEY}`. The type of the produced literal matches the attribute definition's `value_type`:

| Attribute `value_type` | Placeholder example | Produced literal |
|---|---|---|
| `string` | `{user.region}` | `'us-east'` (Utf8) |
| `integer` | `{user.clearance}` | `3` (Int64) |
| `boolean` | `{user.is_vip}` | `true` (Boolean) |
| `list` | `{user.departments}` | `'eng', 'sec'` (multiple Utf8 — use with `IN`) |

Built-in variables (`username`, `id`) always take priority over custom attributes with the same name. This prevents attribute-based override of identity fields.

### Parse-then-substitute pattern

The proxy uses a **parse-then-substitute** pattern: the expression is parsed into a DataFusion expression tree first, then placeholder identifiers are replaced with typed literal values. The user's values never pass through the SQL parser, making this immune to SQL injection even if the value contains SQL syntax.

Example:
```
organization_id = {user.tenant}
```
becomes (at query time, for a user with a `tenant` attribute value of `acme`):
```
organization_id = 'acme'
```

Integer attribute example:
```
sensitivity_level <= {user.clearance}
```
becomes (for a user with `clearance = 3`):
```
sensitivity_level <= 3
```

List attribute example:
```
department IN ({user.departments})
```
becomes (for a user with `departments = ["engineering", "security"]`):
```
department IN ('engineering', 'security')
```
An empty list produces `department IN (NULL)`, which evaluates to false (no rows returned).

### Save-time validation

Filter and mask expressions are validated at policy create/update time. If the expression contains unsupported SQL syntax, the API returns a 422 error immediately — the policy is not saved. This prevents silent failures at query time.

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

Result: only `id` and `name` columns, filtered to the user's tenant attribute value.

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

## User Attributes (ABAC)

User attributes are custom key-value properties on users that drive policy evaluation. They extend the built-in identity fields (`username`, `id`) with arbitrary, admin-defined metadata like `region`, `department`, `tenant`, or `clearance_level`.

### Attribute definitions (schema-first)

Before setting attributes on users, admins must define the allowed keys via Attribute Definitions. Each definition specifies:

- **`key`** — the attribute name (e.g., `region`). Must match `^[a-zA-Z][a-zA-Z0-9_]*$`, 1-64 chars.
- **`entity_type`** — which entity this attribute applies to. Currently only `"user"` is wired up; `"table"` and `"column"` are reserved for future resource-level attributes.
- **`display_name`** — human-readable label shown in the admin UI (e.g., "AWS Region").
- **`value_type`** — one of `"string"`, `"integer"`, `"boolean"`, `"list"`. Determines the type of the literal produced in template variable substitution and in the decision function context. `"list"` stores an array of strings (max 100 elements); use with `IN ({user.KEY})` in filter expressions.
- **`allowed_values`** — optional enum constraint. If set, only these values are accepted.
- **`default_value`** — optional default (must pass type and enum validation).
- **`description`** — optional help text shown in the admin UI.

The same key can exist for different entity types with different constraints (e.g., user `region` as enum vs table `region` as free-text).

Reserved keys for `entity_type = "user"`: `username`, `id`, `user_id`, `roles` — these are rejected to prevent overriding built-in identity fields.

### Value types in detail

| `value_type` | Storage format | Validation | Template literal | Decision function context |
|---|---|---|---|---|
| `string` | Raw string | Max 1024 chars | `'value'` (Utf8) | `"value"` |
| `integer` | Numeric string | Must parse as i64 | `3` (Int64) | `3` |
| `boolean` | `"true"` / `"false"` | Exact match, case-sensitive | `true` (Boolean) | `true` |
| `list` | JSON array of strings | Max 100 elements, each max 1024 chars | Multiple Utf8 literals (see below) | `["a", "b"]` |

**List type details:**

- Always a list of strings — no nested lists, no mixed types. Stored as a JSON array in the user's `attributes` column: `{"departments": ["engineering", "security"]}`.
- Max 100 elements. Each element max 1024 characters.
- `allowed_values` on a list definition constrains the **individual elements**, not the array itself. For example, `allowed_values: ["engineering", "security", "finance"]` means each element in the list must be one of those values.
- `default_value` for a list is a JSON array string: `'["engineering"]'`.
- In template variables, `{user.departments}` expands to multiple comma-separated string literals: `'engineering', 'security'`. Use with `IN`: `department IN ({user.departments})`.
- An empty list `[]` expands to a single `NULL` literal: `department IN (NULL)` → evaluates to false (no rows match).
- In decision function context, list attributes appear as JSON arrays: `ctx.session.user.departments` → `["engineering", "security"]`.

### Namespace design: flat in expressions, nested in API

User attributes live at two different levels depending on the context:

| Surface | How attributes appear | Example |
|---|---|---|
| **API payloads** (create/update/response) | Nested under `attributes` | `{ "attributes": { "region": "us-east" } }` |
| **Template variables** (filter/mask expressions) | Flat under `user` | `{user.region}` |
| **Decision function context** | Flat under `user` | `ctx.session.user.region` |
| **CLI config (future YAML)** | Nested under `attributes` | `attributes: { region: us-east }` |

This is intentional:

- **API/storage nests** because attributes are a distinct concern from built-in fields (`username`, `is_admin`). They have different validation rules, full-replace semantics, and are governed by attribute definitions. Nesting makes the API self-documenting about what is user-defined vs. built-in.
- **Expressions/context flatten** because policy authors write these constantly and brevity matters. `{user.region}` and `ctx.session.user.region` are cleaner than `{user.attributes.region}`.
- **Reserved key validation** prevents collisions — attribute keys cannot shadow built-in fields, and built-in fields always take priority at runtime.

The rule is simple: **define attributes under `attributes`, reference them as `{user.KEY}`**.

### Setting attributes on users

User attributes are stored as a JSON column on the user record. They are set via `PUT /api/v1/users/{id}` with an `attributes` field:

```json
PUT /api/v1/users/{id}
{
  "attributes": {
    "region": "us-east",
    "clearance": "3",
    "is_internal": "true",
    "departments": ["engineering", "security"]
  }
}
```

- **Full replace semantics**: the entire attribute map is replaced. Absent `attributes` field = don't touch. Empty `{}` = clear all.
- **Validation at write time**: every key must match a defined attribute definition; every value must pass type and `allowed_values` checks. Invalid attributes are rejected with a 422 error.
- **No validation on read**: attributes are returned as-is from the JSON column.
- **String-typed values for scalar types**: `integer` and `boolean` attributes are set as strings (`"3"`, `"true"`), not native JSON numbers/booleans. List attributes use native JSON arrays (`["a", "b"]`).

### Creating attribute definitions

```bash
# String attribute with enum constraint
curl -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  http://localhost:5435/api/v1/attribute-definitions \
  -d '{"key": "region", "entity_type": "user", "display_name": "Region", "value_type": "string", "allowed_values": ["us-east", "us-west", "eu-west"], "default_value": "us-east", "description": "User geographic region"}'

# Integer attribute (no enum)
curl -X POST ... \
  -d '{"key": "clearance", "entity_type": "user", "display_name": "Clearance Level", "value_type": "integer", "description": "Security clearance level (1-10)"}'

# Boolean attribute
curl -X POST ... \
  -d '{"key": "is_internal", "entity_type": "user", "display_name": "Internal User", "value_type": "boolean", "default_value": "false"}'

# List attribute with constrained elements
curl -X POST ... \
  -d '{"key": "departments", "entity_type": "user", "display_name": "Departments", "value_type": "list", "allowed_values": ["engineering", "security", "finance", "hr", "analytics"], "description": "Departments the user belongs to"}'
```

### Using attributes in expressions

See [Template variables](#template-variables) above. `{user.KEY}` references produce typed literals based on the attribute definition's `value_type`.

**Missing attributes**: if a user does not have an attribute set (key absent from their `attributes` JSON), `{user.KEY}` produces an empty string literal (`''`). This means a filter like `region = {user.region}` becomes `region = ''` — which typically matches no rows. Design your policies to account for this, or ensure all users have required attributes set.

### Using attributes in decision functions

User attributes are available as first-class fields on `ctx.session.user` with correctly typed JSON values:

```json
{
  "session": {
    "user": {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "username": "alice",
      "roles": ["finance-analyst", "finance"],
      "tenant": "acme",
      "region": "us-east",
      "clearance": 3,
      "is_vip": true,
      "departments": ["engineering", "security"]
    },
    "time": {
      "now": "2026-03-28T14:30:00Z",
      "hour": 14,
      "day_of_week": "Saturday"
    },
    "datasource": {
      "name": "production",
      "access_mode": "policy_required"
    }
  }
}
```

Note: integer and boolean attributes appear as native JSON types in the decision function context (not strings). List attributes appear as JSON arrays of strings.

### Cache behavior

Attribute changes via the admin API trigger immediate cache invalidation (`invalidate_user` + `rebuild_contexts_for_user`). The PolicyHook session cache (60s TTL) is also cleared. Changes take effect without requiring user reconnect.

### Deleting attribute definitions

- Without `?force=true`: returns 409 with the count of affected users.
- With `?force=true`: deletes the definition and removes the key from all users' attribute JSON (database-specific: SQLite `json_remove()`, PostgreSQL `jsonb -`). Triggers cache invalidation.

## Decision Functions

Decision functions are optional, programmable gates that control whether a policy fires for a given query. They are standalone entities with their own lifecycle, separate from policies. A policy references a decision function via `decision_function_id`; one decision function can be reused across multiple policies.

### What they are

A decision function is a JavaScript function compiled to WebAssembly via Javy. At query time, the function receives a JSON context object and returns `{ fire: boolean }`. If `fire` is `false`, the policy is skipped for that query. If `fire` is `true` (or no decision function is attached), the policy applies normally.

This enables logic that is too complex or dynamic for static SQL — for example, role-based masking decisions, time-of-day access windows, or join-count limits.

### Entity lifecycle

Decision functions are managed independently of policies:

- `GET /decision-functions` — list all
- `POST /decision-functions` — create (with JS source; WASM compiled at save time via Javy CLI)
- `GET /decision-functions/{id}` — get single
- `PUT /decision-functions/{id}` — update (recompiles WASM, evicts module cache)
- `DELETE /decision-functions/{id}` — delete (rejected if any policy references it)

The `decision_fn` field holds the JS source. The `decision_wasm` field holds the compiled WASM binary (populated after successful save). Both are stored in the database.

### Function signature

The JS function must be named `evaluate` and return `{ fire: boolean }`:

```js
function evaluate(ctx, config) {
  // ctx: session and/or query context (see evaluate_context)
  // config: hardcoded parameters from the decision function's config field
  return { fire: ctx.session.user.roles.includes("analyst") };
}
```

### Context modes (`evaluate_context`)

| Value | `ctx` contains | When it can fire |
|-------|---------------|-----------------|
| `"session"` | `ctx.session` only: user id/username/roles + custom attributes (e.g., tenant), time (now, hour, day_of_week), datasource name/access_mode | Both at connect time (visibility) and query time |
| `"query"` | `ctx.session` + `ctx.query`: tables, columns, join_count, has_aggregation, has_subquery, has_where, statement_type | Query time only — visibility effect skipped at connect time (policy deferred to query time) |

`time.now` is an ISO 8601 / RFC 3339 timestamp representing the **evaluation time** — the moment the context is built. For visibility-level functions this is when the connection context is computed; for query-level functions it is when the query is processed. This enables time-windowed decision functions (e.g., break-glass temporary access).

Custom user attributes are flattened as first-class fields on the `user` object with correctly typed values (string/number/boolean/array) — e.g., `ctx.session.user.region`, not `ctx.session.user.attributes.region`. This matches the flat `{user.KEY}` namespace used in template variables. In the API, the same attributes are nested under `attributes` (see [Namespace design](#namespace-design-flat-in-expressions-nested-in-api)). List attributes appear as JSON arrays of strings. Built-in fields (`id`, `username`, `roles`) always take priority. See [User Attributes (ABAC)](#user-attributes-abac) for details.

**Visibility-level enforcement**: `column_deny`, `table_deny`, and `column_allow` policies are enforced at connect time (visibility level) by removing columns/tables from the per-user schema. Decision functions on these policy types are evaluated at visibility time when `evaluate_context = "session"`. If the decision function returns `fire: false`, the policy is skipped and the column/table remains visible. For `evaluate_context = "query"`, the policy's visibility effect is skipped entirely (deferred to query time), since query metadata is not available at connect time — the column/table stays visible in the schema and the decision function runs at query time as normal.

### Error handling (`on_error`)

| Value | Behavior when function fails |
|-------|------------------------------|
| `"deny"` | Policy fires (fail-secure) — treat errors as if `fire: true` |
| `"skip"` | Policy skipped (fail-open) — treat errors as if `fire: false` |

Errors include: WASM compilation failure, fuel exhaustion, runtime traps, invalid return shape.

### Logging (`log_level`)

| Value | Effect |
|-------|--------|
| `"off"` | No log capture |
| `"error"` | Capture stderr (exception messages) |
| `"info"` | Capture all output (`console.log` from stdout, exceptions from stderr) |

Captured logs are included in the `DecisionResult` returned by evaluation and appear in the `policies_applied` audit field. Note: Javy 8.x routes `console.log` to stdout by default. The runtime parses stdout robustly, extracting the JSON result and capturing any preceding `console.log` lines as logs.

### `is_enabled`

When `is_enabled = false` on a decision function, the gate is disabled and the policy always fires. This allows temporarily bypassing the gate without changing the policy assignment.

### Config (hardcoded vs parameterized)

The `decision_config` field on the decision function is a JSON object passed as `config` to the JS function. It allows parameterizing a single function for different policies:

```json
{ "max_joins": 3, "allowed_hours": [9, 17] }
```

The function receives this as its second argument: `function evaluate(ctx, config)`.

### Fuel limits

Evaluation is capped at **1,000,000 WASM instructions** per invocation. Exceeding the limit triggers a fuel exhaustion error, which is handled according to `on_error`. This prevents runaway scripts from blocking query processing.

### Audit integration

Decision function results are included in the `policies_applied` JSON field of each `query_audit_log` row. Each entry records `fire`, `fuel_consumed`, `time_us`, `error` (if any), and `logs` (if log_level is not `"off"`). This gives a complete trace of which policies fired and why.

### Scope filtering: targets vs decision functions

Targets and decision functions both control when a policy fires, but they answer different questions and operate at different layers:

| | Targets (declarative) | Decision Functions (programmatic) |
|---|---|---|
| **Question answered** | *Where* does this policy apply? | *When* does this policy fire? |
| **Defined by** | Schema/table/column name patterns | JavaScript logic over session + query context |
| **Evaluated at** | Plan time — the proxy checks if the query touches a matched table/column | Connect time (visibility) and/or query time (enforcement) |
| **Cost** | Zero runtime cost — pattern matching during plan rewrite | ~1ms WASM execution per evaluation |
| **Expressiveness** | Glob patterns on names only (`"public"`, `"raw_*"`, `"*"`) | Arbitrary logic: roles, time, query shape, config parameters |
| **Visibility impact** | Always — targets determine which tables/columns the policy can affect | Conditional — `fire: false` skips the policy, potentially leaving columns/tables visible |

**When to use targets alone (no decision function):**

- The policy applies to a fixed set of tables or columns by name — e.g., `column_deny` on `ssn`, `row_filter` on `public.orders`.
- The policy should always fire when the user queries the matched objects. No conditional logic needed.
- You want zero WASM overhead.

**When to add a decision function:**

- The policy should fire conditionally based on *who* is querying (roles, tenant, attributes), *when* (time of day, day of week), or *what* the query looks like (number of joins, aggregation, specific tables).
- You need conditional behavior on `column_deny`, `table_deny`, or `column_allow` — these types have no expression field, so `CASE WHEN` is not available. Decision functions are the only way to make them conditional.
- You need logic that targets can't express — e.g., "only mask SSN for users who are not in the `compliance` role" or "only apply this row filter during business hours."
- You want to reuse the same conditional logic across multiple policies (decision functions are standalone entities, shareable via `decision_function_id`).

See [Conditional policy examples](#conditional-policy-examples) below for concrete examples of each policy type gated by user attributes.

**How they compose:**

Targets are evaluated first. If the query does not touch any table/column matched by the policy's targets, the policy is skipped entirely — the decision function is never called. If targets match, the decision function (if attached) is evaluated next. Both must pass for the policy to fire.

```
Query arrives
  → Does this query touch a target? (declarative, pattern-based)
     No  → policy skipped (no WASM cost)
     Yes → Is a decision function attached?
            No  → policy fires
            Yes → Evaluate function: fire?
                   true  → policy fires
                   false → policy skipped
```

This layered design means targets act as a cheap pre-filter: they narrow the scope to the relevant tables/columns, and the decision function provides fine-grained conditional logic only when the pre-filter matches. You should always set targets as specifically as possible, even when a decision function is attached, to avoid unnecessary WASM evaluation.

**Example — business-hours masking:**

Goal: mask the `salary` column in `hr.employees` only outside business hours.

- **Targets**: `schemas: ["hr"], tables: ["employees"], columns: ["salary"]` — narrow scope to exactly the right column.
- **Decision function**: `function evaluate(ctx) { const h = ctx.session.time.hour; return { fire: h < 9 || h >= 17 }; }` — mask fires only outside 9-5.
- **Result**: Queries to other tables skip instantly (targets don't match). Queries to `hr.employees.salary` during business hours skip (decision function returns `fire: false`). Queries outside business hours fire the mask.

Without the decision function, the mask would apply 24/7. Without the targets, the decision function would run on every query to every table — wasting WASM cycles on irrelevant queries.

### Conditional policy examples

Decision functions are the primary mechanism for making any policy type conditional based on user attributes. The examples below show how to gate each policy type using user attributes — something that `row_filter` and `column_mask` can also achieve with `CASE WHEN {user.*}` expressions (see [ABAC expression patterns](#abac-expression-patterns)), but that `column_deny`, `table_deny`, and `column_allow` can only do via decision functions.

**Hide columns from non-privileged users (`column_deny`):**

Goal: deny access to the `content` column in `secret.files` only for users with clearance below 5.

Decision function (`evaluate_context: "session"`):
```js
function evaluate(ctx) {
  return { fire: ctx.session.user.clearance < 5 };
}
```

Policy:
```yaml
- name: hide-classified-content
  policy_type: column_deny
  targets:
    - schemas: [secret]
      tables: [files]
      columns: [content]
```

Users with `clearance >= 5` see the column normally. Users below see it removed from schema and query results.

**Hide tables from non-executive users (`table_deny`):**

Goal: block the entire `analytics` schema from users not on the executive team.

Decision function (`evaluate_context: "session"`):
```js
function evaluate(ctx) {
  return { fire: ctx.session.user.team !== 'executive' };
}
```

Policy:
```yaml
- name: hide-analytics-from-non-execs
  policy_type: table_deny
  targets:
    - schemas: [analytics]
      tables: ["*"]
```

Executives see the full `analytics` schema. Everyone else sees "table not found."

**Grant column access conditionally (`column_allow`):**

Goal: in `policy_required` mode, grant access to `hr.employees` only for users in the HR or finance departments.

Decision function (`evaluate_context: "session"`):
```js
function evaluate(ctx) {
  const dept = ctx.session.user.department;
  return { fire: dept === 'hr' || dept === 'finance' };
}
```

Policy:
```yaml
- name: hr-employee-access
  policy_type: column_allow
  targets:
    - schemas: [hr]
      tables: [employees]
      columns: [id, name, title, department, start_date]
```

HR and finance users see the allowed columns. Everyone else sees the table as nonexistent (no `column_allow` grant).

**Mask salary for non-finance users (`column_mask`):**

Goal: mask the `salary` column for users outside the finance department.

Decision function (`evaluate_context: "session"`):
```js
function evaluate(ctx) {
  return { fire: ctx.session.user.department !== 'finance' };
}
```

Policy:
```yaml
- name: mask-salary-non-finance
  policy_type: column_mask
  targets:
    - schemas: [hr]
      tables: [employees]
      columns: [salary]
  definition:
    mask_expression: "0"
```

Finance users see the real salary. Everyone else sees `0`. (This can also be achieved with a `CASE WHEN` in the mask expression — see [ABAC expression patterns](#abac-expression-patterns). Decision functions are useful when the same condition gates multiple policies.)

**Skip row filter for admins (`row_filter`):**

Goal: apply tenant isolation to all users except those with `role = 'admin'`.

Decision function (`evaluate_context: "session"`):
```js
function evaluate(ctx) {
  return { fire: ctx.session.user.role !== 'admin' };
}
```

Policy:
```yaml
- name: tenant-isolation
  policy_type: row_filter
  targets:
    - schemas: ["*"]
      tables: ["*"]
  definition:
    filter_expression: "organization_id = {user.tenant}"
```

Admin users skip the filter entirely (full table access). Non-admins see only their tenant's rows. (This can also be achieved with `CASE WHEN {user.role} = 'admin' THEN true ELSE organization_id = {user.tenant} END` — see [ABAC expression patterns](#abac-expression-patterns).)

**Reusable decision function across policies:**

A single decision function can gate multiple policies. For example, the "non-finance" check above could be shared:

```js
// Decision function: "non-finance-gate"
function evaluate(ctx) {
  return { fire: ctx.session.user.department !== 'finance' };
}
```

Attach to:
- `column_mask` on `hr.employees.salary` → mask salary
- `column_deny` on `hr.employees.bonus` → hide bonus entirely
- `row_filter` on `hr.payroll` → filter to own department

One function, three policies, consistent behavior. Updating the condition (e.g., adding `accounting` as an exception) only requires editing one decision function.

**Parameterized decision function with config:**

Instead of hardcoding department names, use the `config` parameter to make the function reusable across different teams:

```js
// Decision function: "department-gate"
function evaluate(ctx, config) {
  const allowed = config.allowed_departments || [];
  return { fire: !allowed.includes(ctx.session.user.department) };
}
```

Config for salary masking: `{ "allowed_departments": ["finance", "accounting"] }`
Config for executive tables: `{ "allowed_departments": ["executive"] }`

Same function, different configs per policy attachment.

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

Template variables (`{user.username}`, `{user.id}`, and custom attributes like `{user.tenant}`) always resolve from the connecting user's identity and attributes, not the role. A `row_filter` policy with `{user.tenant}` assigned to a role will filter by each member's individual tenant attribute value.

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

## ABAC expression patterns

User attributes (`{user.*}`) can be combined with `CASE WHEN` in `row_filter` and `column_mask` expressions to create conditional behavior without decision functions. The `{user.*}` variables are substituted as typed literals before the expression is parsed, so CASE branches with user attributes evaluate to constants — DataFusion optimizes away the dead branch at plan time.

### row_filter patterns

**Skip filter for admins** — admins see all rows, others are filtered by tenant:
```yaml
- name: tenant-isolation-except-admins
  policy_type: row_filter
  targets:
    - schemas: ["*"]
      tables: ["*"]
  definition:
    filter_expression: "CASE WHEN {user.role} = 'admin' THEN true ELSE organization_id = {user.tenant} END"
```
For an admin user, this becomes `WHERE true` (all rows). For a regular user with tenant `acme`, this becomes `WHERE organization_id = 'acme'`.

**Department-scoped access** — users only see rows from their own department, but the `analytics` team sees everything:
```yaml
- name: department-scoped-orders
  policy_type: row_filter
  targets:
    - schemas: [public]
      tables: [orders]
  definition:
    filter_expression: "CASE WHEN {user.department} = 'analytics' THEN true ELSE department = {user.department} END"
```

**Clearance-level filtering** — users see rows up to their clearance level:
```yaml
- name: clearance-filter
  policy_type: row_filter
  targets:
    - schemas: [classified]
      tables: ["*"]
  definition:
    filter_expression: "sensitivity_level <= {user.clearance}"
```
For a user with `clearance = 3` (integer attribute), this becomes `WHERE sensitivity_level <= 3`. No CASE needed — the integer comparison does the work directly.

**Region-based data isolation** — users only see data from their assigned region:
```yaml
- name: region-isolation
  policy_type: row_filter
  targets:
    - schemas: [public]
      tables: [customers, orders, transactions]
  definition:
    filter_expression: "region = {user.region}"
```

**Multi-department access with list attributes** — users in multiple departments see rows from any of them:
```yaml
- name: multi-department-access
  policy_type: row_filter
  targets:
    - schemas: [public]
      tables: [projects]
  definition:
    filter_expression: "department IN ({user.departments})"
```
For a user with `departments = ["engineering", "security"]`, this becomes `WHERE department IN ('engineering', 'security')`. For a user with an empty list, this becomes `WHERE department IN (NULL)` — effectively no rows.

**Combine clearance and department** — two filters AND together when assigned as separate policies:
```yaml
# Policy 1: row-level department filter
- name: department-filter
  policy_type: row_filter
  targets:
    - schemas: [public]
      tables: [projects]
  definition:
    filter_expression: "department IN ({user.departments})"

# Policy 2: row-level clearance filter
- name: project-clearance-filter
  policy_type: row_filter
  targets:
    - schemas: [public]
      tables: [projects]
  definition:
    filter_expression: "sensitivity_level <= {user.clearance}"
```
Both policies are AND-combined: `WHERE department IN ('engineering') AND sensitivity_level <= 3`. The user must satisfy both constraints.

**VIP flag as a bypass** — boolean attribute to grant unrestricted access:
```yaml
- name: vip-bypass-tenant-filter
  policy_type: row_filter
  targets:
    - schemas: [public]
      tables: [analytics_events]
  definition:
    filter_expression: "CASE WHEN {user.is_vip} THEN true ELSE tenant_id = {user.tenant} END"
```
For a user with `is_vip = true`, this becomes `WHERE true`. For `is_vip = false`, this becomes `WHERE tenant_id = 'acme'`.

### column_mask patterns

**Conditional masking by role** — mask SSN for non-HR users, show raw value for HR:
```yaml
- name: mask-ssn-except-hr
  policy_type: column_mask
  targets:
    - schemas: [public]
      tables: [employees]
      columns: [ssn]
  definition:
    mask_expression: "CASE WHEN {user.department} = 'hr' THEN ssn ELSE '***-**-' || RIGHT(ssn, 4) END"
```
HR users see `123-45-6789`. Everyone else sees `***-**-6789`.

**Tiered masking by clearance** — different mask levels based on user clearance:
```yaml
- name: tiered-salary-mask
  policy_type: column_mask
  targets:
    - schemas: [hr]
      tables: [employees]
      columns: [salary]
  definition:
    mask_expression: "CASE WHEN {user.clearance} >= 5 THEN salary WHEN {user.clearance} >= 3 THEN CAST(ROUND(salary / 1000) * 1000 AS INT) ELSE 0 END"
```
Clearance 5+: sees exact salary (`85432`). Clearance 3-4: sees rounded to nearest thousand (`85000`). Below 3: sees `0`.

**Mask email for external users** — show full email internally, mask for partners:
```yaml
- name: mask-email-external
  policy_type: column_mask
  targets:
    - schemas: [public]
      tables: [customers]
      columns: [email]
  definition:
    mask_expression: "CASE WHEN {user.is_internal} THEN email ELSE '***@' || SPLIT_PART(email, '@', 2) END"
```
Internal users see `alice@example.com`. External users see `***@example.com`.

**Redact unless same region** — show phone numbers only to users in the customer's region:
```yaml
- name: regional-phone-mask
  policy_type: column_mask
  targets:
    - schemas: [public]
      tables: [customers]
      columns: [phone]
  definition:
    mask_expression: "CASE WHEN region = {user.region} THEN phone ELSE '[REDACTED]' END"
```
This references both a **row column** (`region`) and a **user attribute** (`{user.region}`) in the same expression. The user sees raw phone numbers only for customers in their own region.

### Choosing: `CASE WHEN` expression vs decision function

For `row_filter` and `column_mask`, both approaches work. Use this to decide:

| | `CASE WHEN {user.*}` in expression | Decision function |
|---|---|---|
| **Best for** | Single policy, self-contained logic | Same condition shared across multiple policies |
| **Performance** | Zero overhead (constant folding at plan time) | ~1ms WASM per evaluation |
| **Expressiveness** | SQL only, user attributes + row data | Arbitrary JS, config params, time checks |
| **Visibility** | Logic visible in the expression itself | Logic in a separate entity |

Rule of thumb: if the condition is simple and used by one policy, use `CASE WHEN`. If the same condition gates 3+ policies, use a shared decision function to keep them in sync.

For `column_deny`, `table_deny`, and `column_allow`, there is no expression field — decision functions are the only option for conditional behavior.

### Conditional behavior for deny/allow types

`column_deny`, `table_deny`, and `column_allow` have no expression field — they are binary (the policy either applies or doesn't). For conditional behavior on these types, use a **decision function**:

```js
// Decision function: only fire for non-executive users
function evaluate(ctx) {
  return { fire: ctx.session.user.team !== 'executive' };
}
```

Attach this to a `table_deny` policy targeting `executive_comp` — executives see the table, everyone else gets "not found." See [Conditional policy examples](#conditional-policy-examples) in the Decision Functions section for more examples across all five policy types.

### Pattern summary

| Pattern | Mechanism | Example |
|---------|-----------|---------|
| Filter rows by user attribute | `row_filter` expression | `region = {user.region}` |
| Skip filter for privileged users | `CASE WHEN` in `row_filter` | `CASE WHEN {user.role} = 'admin' THEN true ELSE ... END` |
| Filter by numeric threshold | `row_filter` with integer attribute | `sensitivity_level <= {user.clearance}` |
| Filter by list membership | `row_filter` with list attribute | `department IN ({user.departments})` |
| Mask column for non-privileged users | `CASE WHEN` in `column_mask` | `CASE WHEN {user.department} = 'hr' THEN val ELSE '***' END` |
| Tiered masking by clearance | Nested `CASE WHEN` in `column_mask` | `CASE WHEN {user.clearance} >= 5 THEN val WHEN ... END` |
| Mask using row data + user attributes | `CASE WHEN` referencing both | `CASE WHEN region = {user.region} THEN val ELSE ... END` |
| Conditional deny/allow | Decision function | `{ fire: ctx.session.user.team !== 'x' }` |
| Combine multiple filters | Separate policies (AND-combined) | Two `row_filter` policies on same table |
