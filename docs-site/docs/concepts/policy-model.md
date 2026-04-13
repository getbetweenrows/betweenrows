---
title: Policy Model
description: The philosophy behind BetweenRows — zero-trust defaults, deny-wins, visibility-follows-access, and how policies compose.
---

# Policy Model

This page explains *why* BetweenRows enforces policies the way it does — the invariants that make the security model tractable and the design decisions behind them. For the *how* (which type to use, tutorials, field reference), see the [Policies guide](/guides/policies/).

## The three invariants

Every design decision in BetweenRows flows from three invariants. They're non-negotiable — no configuration, policy, or role assignment can violate them.

### 1. Zero-trust defaults

In `policy_required` mode (the recommended production setting), tables start invisible. A table with no matching `column_allow` policy returns empty results and is absent from `information_schema`. Access must be explicitly granted — there is no "allow all, then restrict."

This means forgetting a policy is safe: the user sees nothing, not everything. The blast radius of a misconfiguration is "too little access" (noticeable, fixable) rather than "too much access" (a data breach).

`open` mode relaxes this for development — tables are visible by default, and policies narrow the view. It's a convenience, not a security posture.

→ Deployment guidance: [Data Sources → Access modes](/guides/data-sources#access-modes)

### 2. Deny always wins

If any enabled policy denies access — from any role, any scope, any priority — the deny is enforced. A `column_deny` on `salary` overrides a `column_allow` that includes `salary`. A `table_deny` hides the table even if row filters exist for it.

This holds across:
- Multiple role memberships (user in role A and role B — if either denies, it's denied)
- Different assignment scopes (user-specific allow + all-scoped deny → denied)
- Priority levels (a low-priority deny still overrides a high-priority allow)

The consequence: you can layer permit-policies freely and reach for a deny as the final word. Adding a deny never requires auditing whether some other permit policy might override it.

### 3. Visibility follows access

Schema metadata matches data access exactly. If a column is denied, it disappears from `information_schema.columns` — the user cannot discover it exists. If a table is denied, `\dt` doesn't list it and queries return "table not found" (not "access denied").

This is the **404-not-403 principle**: denied resources look identical to nonexistent ones. An attacker cannot distinguish "this column exists but I can't see it" from "this column doesn't exist" — which means schema probing reveals nothing useful.

Policy changes update both query enforcement and schema visibility immediately, without requiring a reconnect.

## How it works: per-user virtual schema

When a user connects through the proxy, BetweenRows builds a **virtual schema** tailored to their access:

1. Start with the data source's saved catalog (schemas, tables, columns).
2. Apply `table_deny` — remove denied tables entirely.
3. Apply `column_deny` — remove denied columns.
4. In `policy_required` mode, apply `column_allow` — only columns with a matching allow policy survive.
5. The result is the user's virtual schema — what they see in `information_schema`, `\dt`, and `\d`.

At query time, the virtual schema is further narrowed by `row_filter` (injecting WHERE clauses) and `column_mask` (replacing column values in the SELECT projection). These happen in the logical plan, not as string manipulation — which makes them immune to bypass via aliases, CTEs, subqueries, JOINs, or UNIONs.

→ Architecture detail: [Architecture](/concepts/architecture)

## How policies compose

### The five types

| Type | Intent | Effect | Guide |
|---|---|---|---|
| `row_filter` | permit | Injects a WHERE clause | [Row Filters](/guides/policies/row-filters) |
| `column_mask` | permit | Replaces a column's value | [Column Masks](/guides/policies/column-masks) |
| `column_allow` | permit | Allowlists visible columns | [Column Allow & Deny](/guides/policies/column-allow-deny) |
| `column_deny` | deny | Removes columns from schema + results | [Column Allow & Deny](/guides/policies/column-allow-deny) |
| `table_deny` | deny | Removes table from catalog | [Table Deny](/guides/policies/table-deny) |

→ Structural reference: [Policy Types](/reference/policy-types)

### Composition rules

| Situation | Resolution |
|---|---|
| Multiple `row_filter` on the same table | **AND-combined** — narrowing, never expanding |
| Multiple `column_mask` on the same column | Lowest priority number wins |
| Multiple `column_deny` | Union — if any denies, it's denied |
| Multiple `column_allow` | Union — visible columns are the union of all allows |
| `column_deny` vs `column_allow` | **Deny wins** |
| `table_deny` vs any permit | **Deny wins** |

### Assignment and priority

Policies are assigned to a data source with a scope (all users, a specific role, or a specific user) and a priority number (lower = higher precedence, default 100). When the same policy reaches a user through multiple paths, BetweenRows deduplicates and keeps the lowest priority.

At equal priority: user-specific beats role-scoped beats all-scoped.

→ Full detail: [Policies guide → Priority and assignment](/guides/policies/#priority-and-assignment)

## Injection safety: parse-then-substitute

Template variables (`{user.tenant}`, `{user.clearance}`) are substituted as **typed SQL literals** after the expression is parsed into a DataFusion expression tree. The user's attribute value never passes through the SQL parser.

A tenant attribute containing `'; DROP TABLE users; --` produces a single escaped string literal — not an injection. This is safe by construction, not by escaping.

→ Full reference: [Template Expressions](/reference/template-expressions)

## When to mask vs. when to deny

The most common policy-design question, and it matters because the two have different security properties:

**Use `column_mask` when:**
- The column should remain queryable (JOINs, WHERE, GROUP BY work against the masked value)
- The column's *existence* is not sensitive
- You want partial visibility (last-4 of SSN, email domain only)

**Use `column_deny` when:**
- Even the column's existence is sensitive — it should be absent from `information_schema`
- You need to block **predicate probing** (`WHERE ssn = '123-45-6789'`) — masks don't block WHERE predicates; they see raw values
- You need to block **aggregate inference** (`AVG(salary)`, `COUNT(DISTINCT ssn)`) — aggregates can leak statistical properties even through masks

**Rule of thumb:** when in doubt, start with `column_deny`. You can always relax to `column_mask` later. Going the other direction (mask → deny) never causes access regressions.

→ Detailed caveats: [Known Limitations](/operations/known-limitations)

## Decision functions: the escape hatch

For policy gating logic too complex for a SQL expression — time-based access, multi-attribute business rules, query-shape inspection — attach a [decision function](/guides/decision-functions). The function runs in a WASM sandbox and returns `{ fire: true/false }` to control whether the policy applies.

Decision functions have access to a richer context than template variables: user roles, session time, data source metadata, and (in query mode) the tables, columns, and structure of the current query.

→ Full guide: [Decision Functions](/guides/decision-functions)
→ Comparison: [Glossary → Template expressions vs. decision functions](/reference/glossary#template-expressions-vs-decision-functions)

## See also

- [Policies guide](/guides/policies/) — practical: which type, tutorials, field reference
- [Policy Types reference](/reference/policy-types) — structural constraints, JSON shapes, validation
- [Template Expressions](/reference/template-expressions) — expression syntax and variable types
- [Architecture](/concepts/architecture) — the two-plane design and request lifecycle
- [Threat Model](/concepts/threat-model) — the full attack-vector catalog with defenses
- [Glossary](/reference/glossary) — standardized terminology
