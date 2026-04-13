---
title: Glossary
description: Standardized terminology for BetweenRows — user attributes, template variables, policy types, access modes, and other key terms.
---

# Glossary

This page defines the terms used throughout the BetweenRows documentation. When in doubt about what a term means, check here.

## Identity and access

### User
An identity that connects through the BetweenRows proxy. Each user has a username, password, optional admin flag, and a set of **user attributes**. Users authenticate on the data plane (pgwire) and optionally on the management plane (admin UI/API).

### Admin
A user with `is_admin = true`. Grants access to the admin UI and REST API. **Does not grant data plane access** — admin and data access are separate planes.

### Role
A named group of users. Roles are used for policy assignment — assign a policy to a role, and all members receive it. Roles support **inheritance** (a DAG with cycle detection and a depth cap of 10).

### Data source access
An explicit grant allowing a user or role to connect to a specific data source through the proxy. Every user starts with zero data access — grants must be added per data source.

## User attributes (ABAC)

### User attribute
Any property of a user that can be referenced in policy expressions or decision functions. User attributes are the foundation of **attribute-based access control (ABAC)**.

User attributes come in two kinds:

| Kind | Examples | Defined by | Always present? |
|---|---|---|---|
| **Built-in attributes** | `username`, `id` | System | Yes — every user has these |
| **Custom attributes** | `tenant`, `department`, `clearance`, `is_vip` | Admin (via Attribute Definitions) | No — only if the admin defines them and the user has a value set |

Both kinds are accessed the same way in expressions: `{user.username}`, `{user.tenant}`, `{user.clearance}`.

### Built-in attribute
A user attribute provided by the system: `username` (string) and `id` (UUID). Always available, cannot be overridden by custom attributes (reserved keys are rejected at the API).

### Custom attribute
A user attribute defined by the admin via **Attribute Definitions** (as opposed to built-in attributes which are system-managed). Has a key, value type (`string`, `integer`, `boolean`, `list`), optional default value, and optional allowed-values enum. Custom attributes must be defined (via Attribute Definitions) before they can be assigned to users.

This is the standard term — prefer "custom attributes" over "user-defined attributes" in all documentation.

### Attribute definition
The schema for a custom attribute — defines its key, value type, default value, and allowed values. Think of it as a "column definition" for user metadata. Created via the admin UI or API before assigning values to users.

### Default value
The value used when a user lacks a custom attribute. If set, the default is substituted as a typed literal. If not set (NULL), SQL NULL is used — which evaluates to false in WHERE clauses, meaning the user sees zero rows. This is fail-closed by design.

## Variables and expressions

### Template variable
A placeholder in a `row_filter` or `column_mask` expression that is replaced with a value at query time. Written as `{user.KEY}` in expressions.

Today, template variables expose **user attributes** (both built-in and custom). The design allows for future expansion to other namespaces (e.g., `{session.*}`, `{datasource.*}`), but only `{user.*}` is implemented.

Template variables are substituted as **typed SQL literals** after the expression is parsed — never as raw SQL strings. This makes them injection-safe by construction.

→ Full reference: [Template Expressions](/reference/template-expressions)

### Decision function context
The JSON object passed to a decision function's `evaluate(ctx, config)` call. It is a **superset** of what template variables expose — it includes user attributes plus additional context:

| Context path | Contents | Available when |
|---|---|---|
| `ctx.session.user.*` | All user attributes (built-in + custom) | Always |
| `ctx.session.user.roles` | Array of role names | Always |
| `ctx.session.time.now` | ISO 8601 timestamp | Always |
| `ctx.session.datasource.*` | Data source name and metadata | Always |
| `ctx.query.tables` | Array of `{datasource, schema, table}` objects | `evaluate_context = "query"` only |
| `ctx.query.columns` | Output column names | `evaluate_context = "query"` only |
| `ctx.query.join_count` | Number of JOINs | `evaluate_context = "query"` only |
| `ctx.query.has_aggregation` | Boolean | `evaluate_context = "query"` only |
| `ctx.query.statement_type` | `"SELECT"` | `evaluate_context = "query"` only |

→ Full reference: [Decision Functions](/guides/decision-functions)

### Filter expression
The SQL expression in a `row_filter` policy that determines which rows a user can see. Can reference table columns and template variables. Example: `org = {user.tenant}`.

### Mask expression
The SQL expression in a `column_mask` policy that replaces a column's value. Can reference the original column and template variables. Example: `'***-**-' || RIGHT(ssn, 4)`.

## Where attributes and variables are available

Attributes and variables surface in two different contexts — **template expressions** and **decision functions** — with different capabilities:

### Template expressions vs. decision functions

| | Template expressions | Decision functions |
|---|---|---|
| **Used in** | `row_filter` and `column_mask` definitions | Any policy (attached via `decision_function_id`) |
| **Syntax** | SQL with `{user.KEY}` placeholders | JavaScript: `evaluate(ctx, config)` |
| **Built-in attributes** | `{user.username}`, `{user.id}` | `ctx.session.user.username`, `ctx.session.user.id` |
| **Custom attributes** | `{user.tenant}`, `{user.clearance}`, etc. | `ctx.session.user.tenant`, `ctx.session.user.clearance`, etc. |
| **User's roles** | Not available | `ctx.session.user.roles` (array of role names) |
| **Session time** | Not available | `ctx.session.time.now` (ISO 8601) |
| **Data source info** | Not available | `ctx.session.datasource.name` |
| **Query metadata** | Not available | `ctx.query.tables`, `ctx.query.columns`, `ctx.query.join_count`, `ctx.query.has_aggregation`, `ctx.query.statement_type` (requires `evaluate_context = "query"`) |
| **Type safety** | Values substituted as typed SQL literals (Utf8, Int64, Boolean) | Values as typed JSON (string, number, boolean, array) |
| **Injection safety** | Safe by construction — literals in the parsed expression tree | N/A — JS runs in a WASM sandbox, no SQL access |
| **Complexity** | SQL expressions only (comparisons, CASE, string functions) | Full JavaScript logic (conditionals, loops, string manipulation) |
| **When evaluated** | At query time, per query | At connect time (`evaluate_context = "session"`) or per query (`evaluate_context = "query"`) |
| **Performance** | Negligible — literal substitution in the plan | ~1ms WASM execution per invocation |

### When to use which

- **Template expressions** are the default. Use them for straightforward attribute-based filtering and masking — `org = {user.tenant}`, `CASE WHEN {user.department} = 'hr' THEN ssn ELSE masked END`. No JavaScript needed.
- **Decision functions** are the escape hatch. Use them when the gating logic is too complex for a SQL expression — time-based access, multi-attribute business rules, query-shape inspection (e.g., "deny if the query touches more than 3 tables"), or when you need access to roles or session metadata that template variables don't expose.

### What's available today vs. planned

| Namespace | Template expressions | Decision functions | Status |
|---|---|---|---|
| `user.*` (built-in + custom attributes) | Yes | Yes | Shipped |
| `user.roles` | No | Yes | Shipped |
| `session.time.*` | No | Yes | Shipped |
| `session.datasource.*` | No | Yes | Shipped |
| `query.*` (tables, columns, aggregation) | No | Yes (requires `evaluate_context = "query"`) | Shipped |
| `datasource.*` in template expressions | No | N/A | Planned |
| `table.*` / `column.*` attributes | No | No | Planned |

Template variables today are scoped to `{user.*}`. The architecture supports future expansion to other namespaces without breaking existing expressions.

## Policies

### Policy
A named, versioned rule that controls data access. Every policy has a `policy_type`, a set of `targets` (which schemas/tables/columns it applies to), and optionally a `definition` (the expression logic).

### Policy type
One of five types, each with a different effect:

| Type | Intent | Effect |
|---|---|---|
| `row_filter` | permit | Adds a WHERE clause to filter rows |
| `column_mask` | permit | Replaces a column's value with a masked expression |
| `column_allow` | permit | Allowlists specific columns (only in `policy_required` mode) |
| `column_deny` | deny | Removes specific columns from results and schema |
| `table_deny` | deny | Removes an entire table from the user's view |

### Deny-wins invariant
If any enabled deny policy matches, the deny is enforced — regardless of any permit policies. This holds across all scopes, roles, and priorities. It is a core security guarantee.

### Policy assignment
The binding between a policy and a data source, with a scope (who it applies to) and a priority (which wins on conflict).

### Assignment scope
Who a policy assignment applies to:

| Scope | Meaning |
|---|---|
| `all` | Every user on the data source |
| `role` | All members of a specific role (direct + inherited) |
| `user` | One specific user |

### Priority
A numeric value on each policy assignment (default: 100). Lower number = higher precedence. At equal priority, user-specific beats role-specific beats all.

## Data sources and catalog

### Data source
BetweenRows' representation of an upstream PostgreSQL database. Stores connection details, access mode, and the discovered catalog.

### Catalog
The set of schemas, tables, and columns exposed through the proxy for a data source. Maintained as an allowlist — anything not in the catalog is invisible. Discovered from the upstream database and saved by the admin.

### Access mode
Determines what happens when no policy matches a table:

| Mode | Default behavior | Use case |
|---|---|---|
| `policy_required` | Tables are invisible without a `column_allow` policy | Production |
| `open` | Tables are visible to any user with data source access | Development |

### Catalog drift
When the upstream schema changes but the saved catalog hasn't been re-synced. New upstream tables/columns are not automatically exposed — an admin must explicitly select them via **Sync Catalog**.

## Architecture

### Data plane
The pgwire proxy (default port 5434). Handles user connections, query rewriting, policy enforcement, and audit logging.

### Management plane
The admin UI and REST API (default port 5435). Handles configuration: users, roles, policies, data sources, attribute definitions.

### Logical plan rewriting
How BetweenRows enforces policies. Queries are parsed into a DataFusion logical plan, then the plan is rewritten (row filters injected as Filter nodes, column masks as Projection nodes, columns/tables removed) before execution against the upstream database. This approach makes policies bypass-immune — no query shape (aliases, CTEs, subqueries, JOINs) can escape enforcement.

### Virtual schema
The per-user view of the database schema, built at connect time from the catalog + enabled policies. Each user sees a schema tailored to their access — denied columns/tables are absent, not just filtered.

### Visibility follows access
The principle that schema metadata matches data access. If a column is denied, it disappears from `information_schema.columns` — the user cannot discover it exists. If a table is denied, `\dt` doesn't list it and queries return "table not found" (not "access denied").

## Audit

### Query audit log
An append-only log of every query processed by the proxy. Records the original SQL, rewritten SQL, policies applied, execution time, client info, and status.

### Admin audit log
An append-only log of every mutation on the management plane — user/role/policy/datasource changes. Records the actor, action, resource, and a JSON diff of what changed.

### 404-not-403 principle
Denied resources return "not found" errors, not "access denied." This prevents users from discovering what exists but is restricted. Applied to table deny (table not found), column deny (column does not exist), and data source access (unknown database).
