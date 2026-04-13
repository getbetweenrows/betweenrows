---
title: Policy Types
description: Quick reference for the five policy types — row_filter, column_mask, column_allow, column_deny, table_deny — and their structural fields.
---

# Policy Types

Reference version of the policy type tables. For the full mental model and composition rules, see [Policy Model](/concepts/policy-model).

## The five types

| `policy_type` | Intent | What it does |
|---|---|---|
| `row_filter` | permit | Injects a `WHERE` clause into queries on matched tables |
| `column_mask` | permit | Replaces a column's value with a masked expression |
| `column_allow` | permit | Allowlists specific columns; all others are hidden |
| `column_deny` | deny | Removes specific columns from results |
| `table_deny` | deny | Blocks access to an entire table; returns a "not found" error |

Deny types are evaluated before permit types. There is no separate `effect` field.

## Policy responsibility matrix

| `policy_type` | Grants table access? | Grants column access? | Modifies data? |
|---|---|---|---|
| `row_filter` | **No** | No | Yes (filters rows) |
| `column_mask` | **No** | No | Yes (transforms value) |
| `column_allow` | **Yes** | Yes (named columns only) | No |
| `column_deny` | No | Removes named columns | No |
| `table_deny` | Removes table from catalog | Removes all | No |

## Structural fields

Every policy has the same top-level shape:

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

## Wildcards and glob patterns

| Pattern | Matches | Does not match |
|---|---|---|
| `"*"` | everything | — |
| `"public"` | `public` only | `public2`, `private` |
| `"raw_*"` | `raw_orders`, `raw_events` | `orders_raw`, `orders` |
| `"analytics_*"` | `analytics_dev`, `analytics_prod` | `public`, `raw_analytics` |

Both prefix globs (`col_*`) and suffix globs (`*_col`) are supported on the `columns` field. Patterns are case-sensitive.

## Priority and conflict resolution

| Situation | Resolution |
|---|---|
| Multiple `row_filter` policies, same table | Filters are AND'd (intersection) |
| Multiple `column_mask` policies, same column | Lowest priority number wins |
| `column_deny` from any enabled policy | Column is always removed (deny wins) |
| `table_deny` from any enabled policy | Table is always removed (deny wins) |
| Equal priority, user-specific vs wildcard | User-specific assignment wins |

## Assignment scopes

| Scope | Target field | Meaning |
|---|---|---|
| `user` | `user_id` | Applies to one specific user |
| `role` | `role_id` | Applies to all members of a role (including inherited) |
| `all` | neither | Applies to all users on the data source |

## Validation

The API validates policies at create/update time:

- **`row_filter`** — `filter_expression` must be parseable as a DataFusion expression. Unsupported syntax returns 422.
- **`column_mask`** — `mask_expression` must be parseable and must not reference columns outside the target table.
- **`column_allow` / `column_deny`** — `columns` array must be non-empty in every target entry.
- **`column_mask`** — target entries must specify exactly one column per entry.
- **`column_deny` / `table_deny` / `column_allow`** — the `definition` field must be absent.
- **`policy_type`** — must be one of the five enum values.
- **Version conflicts** — `PUT /policies/{id}` requires the current `version`; mismatch returns 409.

## See also

- **[Policy Model](/concepts/policy-model)** — concept page with worked examples
- **[Template Expressions](/reference/template-expressions)** — what you can use in `filter_expression` and `mask_expression`
- **[Admin REST API](/reference/admin-rest-api)** — the endpoints for managing policies
