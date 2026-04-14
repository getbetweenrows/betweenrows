---
title: Column Allow & Deny
description: Use column_allow and column_deny policies to control which columns are visible, with glob patterns and access mode interaction.
---

# Column Allow & Deny

`column_allow` and `column_deny` control column visibility by name. Allow policies grant access to specific columns; deny policies remove them. Neither modifies data — they control what exists in the user's schema.

## Purpose and when to use

- **`column_allow`** — in `policy_required` mode, this is the **only** policy type that grants table access. Use it to define which columns each user/role can see. In `open` mode, allow policies are optional (all columns are visible by default).
- **`column_deny`** — removes specific columns from the user's view. The column disappears from query results AND from `information_schema`. Use it when a column should not exist from the user's perspective (e.g., `credit_card`, `cost_price`).

### Allow vs. deny vs. mask

| Goal | Use |
|---|---|
| Column must remain queryable but with redacted values | `column_mask` |
| Column should not exist at all for this user | `column_deny` |
| Only these specific columns should be visible (whitelist) | `column_allow` |

## Field reference

### column_allow

| Field | Value | Notes |
|---|---|---|
| `policy_type` | `column_allow` | |
| `targets.schemas` | Required | Supports globs |
| `targets.tables` | Required | Supports globs |
| `targets.columns` | Required | The columns to make visible. Supports globs. |
| `definition` | Not used | Must be absent — API returns 422 if present. |

### column_deny

| Field | Value | Notes |
|---|---|---|
| `policy_type` | `column_deny` | |
| `targets.schemas` | Required | Supports globs |
| `targets.tables` | Required | Supports globs |
| `targets.columns` | Required | The columns to remove. Supports globs. |
| `definition` | Not used | Must be absent. |

## Step-by-step tutorial

### Column deny: hide credit card numbers

1. **Create the policy:**
   - **Name:** `hide-credit-card`
   - **Type:** `column_deny`
   - **Targets:** schema `public`, table `customers`, column `credit_card`

2. **Assign** with scope: All users.

3. **Verify:**
   ```sh
   psql 'postgresql://alice:Demo1234!@127.0.0.1:5434/demo_ecommerce' \
     -c "SELECT credit_card FROM customers"
   # → ERROR: column "credit_card" does not exist
   ```

   The column is gone — not just empty, but absent from the schema entirely.

### Column allow: whitelist visible columns (policy_required mode)

1. **Set the data source to `policy_required` mode** (edit the data source).

2. **Create the policy:**
   - **Name:** `analyst-columns`
   - **Type:** `column_allow`
   - **Targets:** schema `public`, table `customers`, columns `id, first_name, last_name, email, org`

3. **Assign** to the `analyst` role.

4. **Verify:** analysts see only the allowed columns. `ssn`, `credit_card`, `phone` are invisible.

## Patterns and recipes

### Glob patterns

| Pattern | Matches | Does not match |
|---|---|---|
| `"*"` | All columns | — |
| `"name"` | `name` only | `first_name`, `last_name` |
| `"*_name"` | `first_name`, `last_name` | `name`, `email` |
| `"secret_*"` | `secret_key`, `secret_token` | `my_secret` |

Patterns are **case-sensitive**.

### Deny financial columns across all tables

```json
{
  "targets": [
    {
      "schemas": ["*"],
      "tables": ["*"],
      "columns": ["cost_price", "margin", "wholesale_price"]
    }
  ]
}
```

### Allow baseline + deny override

In `policy_required` mode, a common pattern is:

1. `column_allow` with `columns: ["*"]` → grants full column access (baseline)
2. `column_deny` on specific sensitive columns → overrides the allow

Deny always wins — the deny removes the column even though the allow includes it.

## Composition

### Deny always wins over allow

If a `column_allow` includes `salary` and a `column_deny` targets `salary`, the column is **denied**. This is the deny-wins invariant — it holds across all scopes and priorities.

### Per-table scoping in JOINs

Column policies are scoped per table. Denying `email` on `customers` does **not** affect `email` on `orders` in the same JOIN. You must create separate deny targets for each table.

### Multiple allow policies → union

If two `column_allow` policies target the same table, the visible columns are the **union** of both. User sees all columns that any allow policy grants.

### Multiple deny policies → union

If two `column_deny` policies target the same column, it's still denied (idempotent). If they target different columns, both are removed.

### Interaction with access modes

| Access mode | `column_allow` needed? | `column_deny` behavior |
|---|---|---|
| `policy_required` | **Yes** — without it, the table is invisible | Removes columns from the allow set |
| `open` | No — all columns visible by default | Removes columns from the default-visible set |

::: danger column_deny does not grant access
In `policy_required` mode, creating a `column_deny` without a `column_allow` leaves the table invisible. The deny has nothing to deny because the table was never granted. You need at least one `column_allow` to make the table visible first.
:::

## Limitations and catches

- **Denied columns disappear from `information_schema`.** Users cannot discover that the column exists — this is the visibility-follows-access invariant.
- **If all selected columns are denied, the query returns an error** (SQLSTATE 42501 — insufficient privilege), not an empty result.
- **`column_allow` with `columns: ["*"]` in `policy_required` mode** is equivalent to `open` mode for that table. Use it as a baseline, then layer denies.
- **Glob patterns are case-sensitive.** `"SSN"` does not match `"ssn"`.

→ Full list: [Known Limitations](/operations/known-limitations)

## Troubleshooting

- **Table invisible despite deny policy** — in `policy_required` mode, you need a `column_allow` first. Deny alone doesn't make the table exist.
- **Column still visible after deny** — check: policy `is_enabled`, target schema/table/column match exactly (case-sensitive), policy assigned to the user's data source.
- **Unexpected columns visible** — check for a `column_allow` with `["*"]` that grants everything, or `open` access mode.

→ Full diagnostics: [Audit & Debugging](/guides/audit-debugging) · [Troubleshooting](/operations/troubleshooting)

## See also

- [Policies overview](/guides/policies/) — when to deny vs. mask, structural shape, validation rules
- [Column Masks](./column-masks) — for redacting values instead of hiding columns
- [Table Deny](./table-deny) — for hiding entire tables
