---
title: Table Deny
description: Use table_deny policies to make entire tables invisible to specific users or roles.
---

# Table Deny

A `table_deny` policy removes an entire table from the user's view. The table disappears from query results, `information_schema`, and `\dt` — the user cannot discover it exists. Queries referencing the table fail with "table not found" (not "access denied"), following the 404-not-403 principle.

## Purpose and when to use

Use table deny when a table should be completely invisible to certain users — e.g., hiding internal `audit_logs` from non-admin users, or restricting `salary_history` to HR.

If you only want to hide specific columns, use [`column_deny`](./column-allow-deny) instead.

## Field reference

| Field | Value | Notes |
|---|---|---|
| `policy_type` | `table_deny` | |
| `targets.schemas` | Required | Supports globs |
| `targets.tables` | Required | Supports globs |
| `targets.columns` | Not used | — |
| `definition` | Not used | Must be absent. |

## Step-by-step tutorial

### Hide an internal table

1. **Create the policy:**
   - **Name:** `hide-internal-metrics`
   - **Type:** `table_deny`
   - **Targets:** schema `public`, table `internal_metrics`

2. **Assign** with scope: All users (or a specific role).

3. **Verify:**
   ```sh
   psql 'postgresql://alice:Demo1234!@127.0.0.1:5434/demo_ecommerce' \
     -c "SELECT * FROM internal_metrics"
   # → ERROR: table "internal_metrics" does not exist
   ```

   The table is gone from `\dt` output too.

## Patterns and recipes

### Deny by glob pattern

```json
{
  "targets": [
    {
      "schemas": ["*"],
      "tables": ["internal_*"]
    }
  ]
}
```

Hides all tables starting with `internal_`.

### Conditional deny with decision functions

Attach a [decision function](/guides/decision-functions) to the `table_deny` policy to make it conditional — e.g., deny only for users outside business hours, or deny only for users who are not members of a specific admin role (`!ctx.session.user.roles.includes("platform-admin")`).

## Limitations and catches

- **Uses the upstream schema name, not any alias.** The target must match the schema name as it appears in the catalog. Using a different name silently fails — the table remains visible.
- **Deny always wins.** If any enabled `table_deny` matches, the table is hidden, regardless of other policies.
- **Row filters on denied tables are irrelevant.** If a table is denied, its row filters never execute — the table doesn't exist.
- **Error message does not reveal the policy.** The user sees "table not found", not "access denied" — this prevents leaking metadata about what tables exist but are restricted.

→ Full list: [Known Limitations](/operations/known-limitations)

## See also

- [Policies overview](/guides/policies/) — choosing between deny and other types
- [Column Allow & Deny](./column-allow-deny) — for column-level visibility control
- [Decision Functions](/guides/decision-functions) — for conditional table deny
