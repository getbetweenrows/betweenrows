---
title: Audit & Debugging
description: Use the query and admin audit logs to debug policy behavior, trace rewritten SQL, and investigate access decisions.
---

# Audit & Debugging

BetweenRows logs every query and every admin mutation. The audit logs are your primary debugging tool — they show exactly what the proxy did, which policies fired, and what SQL actually ran against the upstream database.

## Purpose and when to use

Use the audit logs when:
- A policy isn't behaving as expected (wrong rows, missing columns, unexpected access)
- You need to investigate who changed a policy, role, or user
- You need a compliance trail of all data access and admin actions
- You're debugging a "zero rows returned" or "table not found" issue

## The two audit logs

| Log | What it records | API endpoint |
|---|---|---|
| **Query audit** | Every query through the proxy — original SQL, rewritten SQL, policies applied, status, timing | `GET /api/v1/audit/queries` |
| **Admin audit** | Every admin mutation — user/role/policy/datasource changes, who did it, what changed | `GET /api/v1/audit/admin` |

Both are **append-only** — there are no UPDATE or DELETE endpoints. Once written, an audit entry cannot be modified.

→ Field-level reference: [Audit Log Fields](/reference/audit-log-fields)

## Step-by-step: debug a policy issue

### Scenario 1: row filter not applied

**Symptom:** Alice sees rows from all tenants, not just `acme`.

1. Open **Query Audit** in the admin UI.
2. Find Alice's query. Check the **Status** field — it should be `success`.
3. Check **Policies applied** — is `tenant-isolation` listed?
   - **Not listed:** the policy is not reaching Alice. Check: policy `is_enabled`, assigned to Alice's data source, target schemas/tables match the queried table, Alice has data source access.
   - **Listed:** the policy fired but the filter may not be effective.
4. Check **Rewritten query** — does it contain `WHERE org = 'acme'`?
   - **No WHERE clause:** the template variable may have resolved to NULL. Check Alice's `tenant` attribute value.
   - **Wrong value:** check which user attribute value Alice has set.

![Query audit detail view showing rewritten SQL and applied policies](/screenshots/audit-debugging-query-detail-v0.15.png)

### Scenario 2: zero rows returned

**Symptom:** Alice sees no rows when she should see some.

1. In the audit log, find the query and check the **Rewritten query**.
2. Look for `WHERE ... AND false` or `Filter: Boolean(false)` — this means a policy injected an always-false filter, which happens when:
   - The user's attribute resolved to NULL (no attribute set, no default value)
   - An empty list attribute expanded to `IN (NULL)`
   - In `policy_required` mode, no `column_allow` policy matched the table
3. Check **Policies applied** — are multiple row filters AND-combining to an impossible condition?
4. Check the user's attributes in the admin UI — is the expected attribute set?

### Scenario 3: column mask not visible in results

**Symptom:** You created a `column_mask` but the column shows raw values.

1. In the audit log, check **Rewritten query** — look for the mask expression in the SELECT list (e.g., `'***-**-' || RIGHT(ssn, 4) AS ssn`).
   - **Mask expression present:** the policy is applied but something downstream may be wrong. Check if you're querying the right data source.
   - **Mask expression absent:** the policy isn't firing. Check: `is_enabled`, target schema/table/column match exactly (case-sensitive), assigned to the data source, column exists in the catalog.
2. Check **Policies applied** — is the mask policy listed?
3. If multiple masks target the same column, check priority numbers — only the lowest-priority mask applies.

### Scenario 4: query denied

**Symptom:** Query returns an error instead of results.

1. In the audit log, check **Status** — should show `denied` or `error`.
2. Check **Error message** — note that BetweenRows deliberately does **not** reveal which policy caused the denial (404-not-403 principle). The error says "table not found" or "column does not exist," not "blocked by policy X."
3. Common causes:
   - `table_deny` hiding the table → "table not found"
   - `column_deny` removing all selected columns → SQLSTATE 42501
   - `policy_required` mode with no `column_allow` → table invisible
   - Write statement (INSERT/UPDATE/DELETE) → read-only enforcement

### Scenario 5: investigating an admin change

**Symptom:** "Who changed this policy?" or "When was this user deactivated?"

1. Open **Admin Audit** in the admin UI.
2. Filter by `resource_type` (e.g., `policy`, `user`, `role`) and optionally by `resource_id`.
3. Each entry shows:
   - **Actor** — which admin made the change
   - **Action** — `create`, `update`, `delete`, `assign`, `unassign`, etc.
   - **Changes** — JSON diff of what changed (before/after for updates, full snapshot for create/delete)

![Admin audit entry showing actor, action, and change diff](/screenshots/audit-debugging-admin-detail-v0.15.png)

## Patterns and recipes

### Filter the query audit

The API supports these query parameters:

| Parameter | Type | Purpose |
|---|---|---|
| `user_id` | UUID | Show only queries from this user |
| `datasource_id` | UUID | Show only queries on this data source |
| `status` | string | `success`, `error`, or `denied` |
| `since` | datetime | Entries after this timestamp |
| `until` | datetime | Entries before this timestamp |
| `limit` | integer | Max entries to return |

Example: "Show me all denied queries on `production_db` in the last hour."

### Correlate query and admin audit

When debugging a "policy stopped working," check both logs:
1. **Admin audit** — was the policy disabled, unassigned, or edited recently?
2. **Query audit** — did the policy appear in `policies_applied` before the issue started?

The timestamps correlate — find the admin change, then find the first query after it.

### Denied writes

BetweenRows is read-only. If a client sends `DELETE FROM orders`, the proxy rejects it — but the attempt is still audited with `status = "denied"`. Check the query audit for write attempts from users who shouldn't be sending them.

## Composition with other features

- **Policy changes take effect immediately.** After editing a policy, the next query from any connected user reflects the change. The audit log shows exactly when the change took effect.
- **Decision function results are included** in `policies_applied` — you can see whether the function returned `fire: true` or `fire: false` and what error (if any) occurred.
- **Rename safety:** the audit log denormalizes `datasource_name` and `username` at write time, so entries survive entity renames. Historical entries show the name at the time of the query, not the current name.

## Limitations and catches

- **Error messages do not reveal policy details.** "Table not found" means a `table_deny` or missing `column_allow` blocked access, but the error doesn't say which policy. This is intentional (prevents probing). Use the audit log to see which policies fired.
- **Audit entries are written asynchronously.** The query result reaches the client before the audit row is committed. In rare crash scenarios, the last few entries may be lost.
- **Admin audit records secrets as boolean flags.** Password changes log `"field": "password"`, not the actual hash. Config changes log `"config_changed": true`, not the connection details.
- **No retention policy yet.** Audit entries accumulate indefinitely. Monitor database size and plan manual cleanup if needed.

→ Full field reference: [Audit Log Fields](/reference/audit-log-fields)

## Troubleshooting

- **Audit log is empty** — check: user connected through the proxy (not directly to the upstream), the admin database is writable, `RUST_LOG` is at `info` or above.
- **Missing `rewritten_query`** — the query was denied before rewriting. Check `status` field.
- **Unexpected `policies_applied`** — a policy you didn't expect is firing. Check all assignments: user-scoped, role-scoped, and all-scoped. Remember that role inheritance can bring in policies from parent roles.

→ Full diagnostics: [Troubleshooting](/operations/troubleshooting)

## See also

- [Audit Log Fields](/reference/audit-log-fields) — every field in both logs
- [Policies overview](/guides/policies/) — understanding what fires and why
- [Troubleshooting](/operations/troubleshooting) — connection and policy diagnostic trees

<!-- screenshots: [audit-debugging-query-detail-v0.15.png, audit-debugging-admin-detail-v0.15.png, audit-debugging-filter-panel-v0.15.png] -->
