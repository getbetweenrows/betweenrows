---
title: Policies
description: The policy system — five types, how they compose, structural shape, validation rules, and when to use which.
---

# Policies

Policies are the core enforcement mechanism in BetweenRows. They determine which rows a user sees, which columns are visible or masked, and which tables exist from the user's perspective. This page is the landing for the whole policy cluster — which type to reach for, the JSON shape every policy shares, how they compose, and the deny-wins invariant. Each type has its own detailed guide linked below.

→ For the *philosophy* behind these design decisions, see [Policy Model](/concepts/policy-model).

## Which policy type do I need?

| I want to... | Use | Guide |
|---|---|---|
| **Filter rows** by user identity (tenant, department, region) | `row_filter` | [Row Filters](./row-filters) |
| **Redact a column value** (SSN → `***-**-1234`, email → `***@domain.com`) | `column_mask` | [Column Masks](./column-masks) |
| **Allowlist specific columns** (only these columns are visible) | `column_allow` | [Column Allow & Deny](./column-allow-deny) |
| **Remove specific columns** from results | `column_deny` | [Column Allow & Deny](./column-allow-deny) |
| **Hide an entire table** from a user or role | `table_deny` | [Table Deny](./table-deny) |

### When to mask vs. when to deny

- **Mask** when the column must remain queryable (JOINs, WHERE, GROUP BY work against the masked value) but the raw value should not be visible. Example: SSN masked to last-4, email domain preserved.
- **Deny** when the column should not exist at all from the user's perspective — not in query results, not in `information_schema`, not usable in expressions. Example: `credit_card` column removed entirely.

::: tip Rule of thumb
If the user needs to *reference* the column (even with redacted values), mask it. If the user should not know the column exists, deny it.
:::

## The five types at a glance

| Type | Intent | Grants access? | Modifies data? |
|---|---|---|---|
| `row_filter` | permit | No | Yes (filters rows) |
| `column_mask` | permit | No | Yes (transforms value) |
| `column_allow` | permit | **Yes** (named columns only) | No |
| `column_deny` | deny | Removes named columns | No |
| `table_deny` | deny | Removes table from catalog | No |

Deny types are evaluated before permit types. There is no separate `effect` field — the type implies the effect.

`column_allow` is the only type that grants access. In `policy_required` mode, a table with no `column_allow` is invisible regardless of what else is configured. See [Access mode interaction](#access-mode-interaction).

## Structural shape

Every policy has the same top-level JSON shape:

```json
{
  "name": "string (unique)",
  "policy_type": "row_filter | column_mask | column_allow | column_deny | table_deny",
  "targets": [
    {
      "schemas": ["public", "raw_*"],
      "tables": ["orders"],
      "columns": ["ssn"]
    }
  ],
  "definition": { /* type-specific, see below */ },
  "is_enabled": true,
  "decision_function_id": null
}
```

### Target fields by policy type

| `policy_type` | `schemas` | `tables` | `columns` |
|---|---|---|---|
| `row_filter` | required | required | — (not used) |
| `column_mask` | required | required | **required** (exactly one) |
| `column_allow` | required | required | **required** |
| `column_deny` | required | required | **required** |
| `table_deny` | required | required | — (not used) |

### Definition by policy type

- **`row_filter`** — `definition` is required:

  ```json
  { "filter_expression": "org = {user.tenant}" }
  ```

- **`column_mask`** — `definition` is required:

  ```json
  { "mask_expression": "'***-**-' || RIGHT(ssn, 4)" }
  ```

- **`column_allow`**, **`column_deny`**, **`table_deny`** — no `definition` field; it must be absent.

The API rejects policies with the wrong shape (e.g., `column_deny` with a `definition` field → 422).

## How policies compose

### Multiple policies of the same type

| Situation | Resolution |
|---|---|
| Multiple `row_filter` on the same table | **AND-combined** — a row must pass all filters to be visible. Layering narrows results, never expands. |
| Multiple `column_mask` on the same column | **Lowest priority number wins** (highest precedence). Use distinct priorities to avoid undefined ordering. |
| Multiple `column_deny` on the same column | **Union** — if any deny policy matches, the column is removed. |
| Multiple `column_allow` on the same table | **Union** — visible columns are the union of all allow policies. |

### Deny always wins

If any enabled policy denies access — from any role, any scope, any source — the deny is enforced. A `column_deny` on `salary` overrides a `column_allow` that includes `salary`. A `table_deny` hides the table even if a `row_filter` exists for it.

This invariant means you can layer permit-policies freely and reach for a deny as the final word.

### Policy changes take effect immediately

When you create, edit, enable, disable, or reassign a policy, the change takes effect for all connected users on their next query — no reconnect needed. BetweenRows rebuilds each user's view of the schema in the background.

## Priority and assignment

### Priority numbers

Every policy assignment has a numeric priority (default: 100). Lower number = higher precedence. When the same policy could be assigned through multiple paths (user + role + all), BetweenRows deduplicates and keeps the **lowest priority number**.

| Priority | Use case |
|---|---|
| 0–49 | Override policies (e.g., admin bypass) |
| 50–99 | High-priority restrictions |
| 100 | Default |
| 101+ | Low-priority fallbacks |

### Assignment scopes

| Scope | Target | Meaning |
|---|---|---|
| `all` | — | Applies to every user on the data source |
| `role` | A specific role | Applies to all members (direct + inherited) |
| `user` | A specific user | Applies to that one user only |

At equal priority, **user-specific beats role-specific beats all**.

## Access mode interaction

The data source's `access_mode` changes what happens when no policy matches:

- **`policy_required`** (recommended for production): tables with no `column_allow` policy are **invisible**. `column_allow` is the only type that grants access. Without it, the table returns empty results and is hidden from `information_schema`.
- **`open`**: tables are visible by default. Row filters, masks, and denies narrow the view, but no `column_allow` is needed.

::: danger
`column_deny` does **not** grant table access. In `policy_required` mode, creating a deny-only policy without a `column_allow` leaves the table invisible — the deny has nothing to deny because the table was never granted.
:::

→ Full explanation: [Data Sources → Access modes](/guides/data-sources#access-modes)

## Template variables in expressions

`row_filter` and `column_mask` expressions can reference user attributes like `{user.tenant}`. Values are substituted as typed SQL literals — injection-safe by construction. → Full reference: [Template Expressions](/reference/template-expressions)

## Wildcard targets

Policy targets support glob patterns for schemas, tables, and columns:

| Pattern | Matches | Does not match |
|---|---|---|
| `"*"` | everything | — |
| `"public"` | `public` only | `public2`, `private` |
| `"raw_*"` | `raw_orders`, `raw_events` | `orders_raw`, `orders` |
| `"*_pii"` | `customers_pii`, `employees_pii` | `pii_customers`, `customers` |
| `"analytics_*"` | `analytics_dev`, `analytics_prod` | `public`, `raw_analytics` |

Both prefix globs (`col_*`) and suffix globs (`*_col`) are supported on the `columns` field. Patterns are **case-sensitive**.

## Validation

The API validates policies at create/update time:

- **`row_filter`** — `filter_expression` must be parseable as a DataFusion expression. Unsupported syntax returns 422.
- **`column_mask`** — `mask_expression` must be parseable and must not reference columns outside the target table. Target entries must specify exactly one column per entry.
- **`column_allow` / `column_deny`** — `columns` array must be non-empty in every target entry.
- **`column_deny` / `table_deny` / `column_allow`** — the `definition` field must be absent.
- **`policy_type`** — must be one of the five enum values.
- **Version conflicts** — `PUT /policies/{id}` requires the current `version`; mismatch returns 409.

## Detailed guides

- **[Row Filters](./row-filters)** — filter rows by user identity with template variables
- **[Column Masks](./column-masks)** — redact column values with SQL expressions
- **[Column Allow & Deny](./column-allow-deny)** — control column visibility by name
- **[Table Deny](./table-deny)** — hide entire tables
- **[Template Expressions](/reference/template-expressions)** — the syntax used inside `filter_expression` and `mask_expression`
- **[Decision Functions](/guides/decision-functions)** — conditionally gate any policy with JavaScript logic
- **[Multi-Tenant Isolation](/guides/recipes/multi-tenant-isolation)** — flagship recipe combining row filters with attributes at scale

## See also

- [Policy Model](/concepts/policy-model) — the philosophy: zero-trust, deny-wins, visibility-follows-access
