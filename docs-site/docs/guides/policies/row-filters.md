---
title: Row Filters
description: Write row_filter policies to restrict which rows each user can see, with template variables and composition patterns.
---

# Row Filters

A `row_filter` policy injects a `WHERE` clause into every query on matched tables. The filter evaluates against raw (unmasked) data and is applied at the logical plan level — it cannot be bypassed by aliases, CTEs, subqueries, JOINs, or UNIONs.

## Purpose and when to use

Use row filters whenever different users should see different subsets of rows from the same table — by tenant, department, region, clearance level, or any attribute-driven dimension.

## Field reference

| Field | Value | Notes |
|---|---|---|
| `policy_type` | `row_filter` | |
| `targets.schemas` | Required | Which schemas to match (supports globs) |
| `targets.tables` | Required | Which tables to match (supports globs) |
| `targets.columns` | Not used | — |
| `definition.filter_expression` | Required | SQL expression using column references and `{user.KEY}` template variables |

## Step-by-step tutorial

This tutorial uses the [demo schema](/reference/demo-schema). The goal: each user sees only their tenant's rows.

### 1. Prerequisites

Ensure you have:
- A data source with catalog discovered (e.g., `demo_ecommerce`)
- A `tenant` attribute defined (value type: `string`)
- Users `alice` (tenant: `acme`) and `bob` (tenant: `globex`) with data source access

→ Setup: [Demo Schema](/reference/demo-schema) · [User Attributes](/guides/attributes)

### 2. Create the policy

Go to **Policies → Create**:

- **Name:** `tenant-isolation`
- **Type:** `row_filter`
- **Targets:** schemas `*`, tables `*` (applies to all tables with an `org` column)
- **Filter expression:** `org = {user.tenant}`

![Row filter policy editor with tenant isolation expression](/screenshots/row-filters-policy-editor-v0.15.png)

### 3. Assign the policy

On the data source page, assign `tenant-isolation` with **scope: All users**.

![Assigning a row filter policy to a data source with all-users scope](/screenshots/row-filters-assignment-v0.15.png)

### 4. Verify

Connect as alice and bob:

```sh
psql 'postgresql://alice:Demo1234!@127.0.0.1:5434/demo_ecommerce' \
  -c "SELECT DISTINCT org FROM orders"
# → acme

psql 'postgresql://bob:Demo1234!@127.0.0.1:5434/demo_ecommerce' \
  -c "SELECT DISTINCT org FROM orders"
# → globex
```

### 5. Check the audit log

Open **Query Audit** in the admin UI. Alice's query shows:

- **Original:** `SELECT DISTINCT org FROM orders`
- **Rewritten:** `SELECT DISTINCT org FROM orders WHERE org = 'acme'`
- **Policies applied:** `tenant-isolation (v1)`

![Query audit entry showing the injected WHERE clause from tenant isolation](/screenshots/row-filters-audit-v0.15.png)

## Patterns and recipes

### Per-tenant isolation (string)

```sql
org = {user.tenant}
```

The most common pattern. One policy covers all tables with an `org` column.

### Clearance-level filtering (integer)

```sql
sensitivity_level <= {user.clearance}
```

Users see rows up to their clearance level. A user with `clearance: 3` sees levels 1, 2, and 3.

### Department allowlist (list)

```sql
department IN ({user.departments})
```

A user with `departments: ["engineering", "security"]` sees rows in either department. An empty list expands to `NULL` → zero rows.

### Date-range restriction

```sql
created_at >= '2024-01-01'
```

Static filters work too — no template variable required.

### Combined filter (AND)

```sql
org = {user.tenant} AND status != 'deleted'
```

A single expression can combine attribute-based and static conditions.

## Composition

### Multiple row filters → AND

If two row filter policies match the same table, their expressions are AND-combined. A row must satisfy **both** filters to be visible.

Example: `tenant-isolation` (`org = {user.tenant}`) + `active-only` (`status = 'active'`) → user sees only active rows in their tenant.

### Row filters + column masks

Row filters evaluate **raw** values, even when a column is masked by another policy. If `salary` is masked to `0` but a row filter checks `salary > 50000`, the filter sees the real salary — the mask only affects what appears in query results.

This composition is safe by design: the filter runs against truthful data, and the mask runs against the projection.

### Row filters in JOINs

Each table in a JOIN is independently filtered by its own row filter policies. Filtering is per-table, not global. A filter on `orders` does not affect `customers` in the same JOIN — each table's filter applies to its own `TableScan`.

### Row filters + table deny

If a `table_deny` hides a table, row filters on that table are irrelevant — the table doesn't exist from the user's perspective.

## Limitations and catches

- **Bypass-immune by construction.** Filters are applied at the `TableScan` level in the logical plan. No query shape — aliases, CTEs, subqueries, JOINs, UNIONs — can escape them. This is a core security guarantee.
- **Multiple filters narrow, never expand.** AND-combination means adding a filter can only reduce visible rows. There is no OR-combination mode.
- **NULL attribute → zero rows.** If `{user.tenant}` resolves to NULL (user lacks the attribute, no default set), then `org = NULL` is never true. The user sees nothing. This is fail-closed by design. See [User Attributes → Missing attribute behavior](/guides/attributes#missing-attribute-behavior).
- **Empty list attribute → zero rows.** `department IN ({user.departments})` with an empty list becomes `department IN (NULL)` → zero rows.
- **Filter expressions are validated at save time.** Unsupported SQL syntax (e.g., correlated subqueries, window functions) returns 422 immediately. See [Template Expressions → Supported SQL syntax](/reference/template-expressions#supported-sql-syntax).

→ Full list: [Known Limitations](/operations/known-limitations)

## Troubleshooting

- **Filter not applied** — check: policy `is_enabled`, assigned to the user's data source, target schemas/tables match the queried table, user has data source access.
- **Zero rows when expecting data** — check: user has the required attribute set, attribute value matches the data, no conflicting row filter AND-combining to empty. Inspect the **rewritten query** in the audit log.
- **Filter applied to wrong tables** — check target patterns. `schemas: ["*"], tables: ["*"]` matches everything; a table without the referenced column (e.g., no `org` column) will error at query time.

→ Full diagnostics: [Audit & Debugging](/guides/audit-debugging) · [Troubleshooting](/operations/troubleshooting)

## See also

- [Policies overview](/guides/policies/) — which type to use when
- [Multi-Tenant Isolation](/guides/recipes/multi-tenant-isolation) — the flagship row filter use case at scale
- [Template Expressions](/reference/template-expressions) — full expression syntax and NULL semantics
- [User Attributes](/guides/attributes) — how to define and assign the attributes that drive filters

<!-- screenshots: [row-filters-policy-editor-v0.15.png, row-filters-assignment-v0.15.png, row-filters-audit-v0.15.png] -->
