---
title: Policies
description: Overview of the five policy types, when to use each, how they compose, and the deny-wins invariant.
---

# Policies

Policies are the core enforcement mechanism in BetweenRows. They determine which rows a user sees, which columns are visible or masked, and which tables exist from the user's perspective. This page covers the mental model — which type to reach for, how policies compose, and the key invariants. Each type has its own detailed guide linked below.

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

| Type | Intent | Requires `definition`? | Requires `columns` in targets? |
|---|---|---|---|
| `row_filter` | permit | Yes (`filter_expression`) | No |
| `column_mask` | permit | Yes (`mask_expression`) | Yes (exactly 1 per target) |
| `column_allow` | permit | No | Yes |
| `column_deny` | deny | No | Yes |
| `table_deny` | deny | No | No |

→ Full structural reference: [Policy Types](/reference/policy-types)

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

`row_filter` and `column_mask` expressions can reference user attributes:

```sql
-- Row filter: tenant isolation
org = {user.tenant}

-- Column mask: conditional by department
CASE WHEN {user.department} = 'hr' THEN ssn ELSE '***-**-' || RIGHT(ssn, 4) END
```

Values are substituted as typed SQL literals — injection-safe by construction.

→ Full reference: [Template Expressions](/reference/template-expressions)

## Wildcard targets

Policy targets support glob patterns for schemas, tables, and columns:

| Pattern | Matches |
|---|---|
| `"*"` | Everything |
| `"public"` | Exact match only |
| `"raw_*"` | Prefix match: `raw_orders`, `raw_events` |
| `"*_pii"` | Suffix match: `customers_pii`, `employees_pii` |

Patterns are **case-sensitive**.

→ Full reference: [Policy Types → Wildcards](/reference/policy-types#wildcards-and-glob-patterns)

## Detailed guides

- **[Row Filters](./row-filters)** — filter rows by user identity with template variables
- **[Column Masks](./column-masks)** — redact column values with SQL expressions
- **[Column Allow & Deny](./column-allow-deny)** — control column visibility by name
- **[Table Deny](./table-deny)** — hide entire tables
- **[Multi-Tenant Isolation](/guides/recipes/multi-tenant-isolation)** — the flagship use case combining row filters with attributes at scale
- **[Decision Functions](/guides/decision-functions)** — conditionally gate any policy with JavaScript logic

## See also

- [Policy Model](/concepts/policy-model) — the philosophy: zero-trust, deny-wins, visibility-follows-access
- [Policy Types reference](/reference/policy-types) — structural constraints, JSON shapes, validation rules
- [Template Expressions](/reference/template-expressions) — what you can use in filter and mask expressions
