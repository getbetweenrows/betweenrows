---
title: Audit Log Fields
description: Field reference for the query audit log and admin audit log — every column, its source, type, and semantics.
---

# Audit Log Fields

BetweenRows maintains two separate audit logs: one for data-plane queries and one for admin-plane mutations. Both are append-only — there are no UPDATE or DELETE endpoints.

## Query audit log

A row is written for every query that reaches the proxy, including denied queries and failed queries. Audit entries are written asynchronously after the result reaches the client, so query latency does not include audit I/O.

**API:** `GET /api/v1/audit/queries` (filterable by `user_id`, `datasource_id`, `status`, `from`, `to`, `page`, `page_size`)

| Field | Type | Description |
|---|---|---|
| `id` | UUID | Unique audit entry ID |
| `user_id` | UUID | The authenticated user who ran the query |
| `username` | string | Denormalized username (survives user deletion) |
| `data_source_id` | UUID | The datasource the query targeted |
| `datasource_name` | string | Denormalized datasource name (survives rename) |
| `original_query` | string | The SQL statement as sent by the client |
| `rewritten_query` | string (nullable) | The SQL actually executed against the upstream database, with all row filters and column masks applied. This is the key debugging field — compare it with `original_query` to see what BetweenRows changed. NULL if the query was denied before rewriting. |
| `policies_applied` | JSON string | Array of `{policy_id, version, name}` objects — a snapshot of which policies fired for this query, including decision function results. Use this to answer "which policies affected this query?" |
| `execution_time_ms` | integer (nullable) | Wall-clock time for the upstream query execution, in milliseconds. NULL for denied queries. |
| `client_ip` | string (nullable) | Client IP address from the pgwire connection |
| `client_info` | string (nullable) | Application name from pgwire startup parameters (e.g. `psql`, `DBeaver`, your app's connection string) |
| `status` | string | One of: `success` (query completed), `error` (query failed), `denied` (query blocked by policy or read-only enforcement) |
| `error_message` | string (nullable) | Error details when `status` is `error` or `denied`. For denied queries, does **not** reveal which policy caused the denial (404-not-403 principle). |
| `created_at` | datetime | When the audit entry was written |

### Key behaviors

- **Denied writes are audited.** If a client sends `DELETE FROM orders`, the proxy rejects it (read-only enforcement), but a row is still written with `status = "denied"`. You can see every attempted write.
- **The `rewritten_query` shows the real SQL.** Row filters appear as injected `WHERE` clauses; column masks appear as transformed expressions in the `SELECT` list. This is the single best debugging tool for "why did I get these rows?"
- **`policies_applied` is a snapshot.** It captures the policy name and version at query time, so even if the policy is later edited or deleted, the audit record shows what was in effect.

## Admin audit log

A row is written for every mutation to the admin-plane state: users, roles, policies, datasources, attribute definitions, policy assignments, role memberships, and role inheritance. Mutations and their audit entries are written atomically in the same database transaction — if the mutation commits, the audit entry exists; if it rolls back, neither is persisted.

**API:** `GET /api/v1/audit/admin` (filterable by `resource_type`, `resource_id`, `actor_id`, `from`, `to`, `page`, `page_size`)

| Field | Type | Description |
|---|---|---|
| `id` | UUID | Unique audit entry ID |
| `resource_type` | string | The entity type that was changed: `user`, `role`, `policy`, `datasource`, `attribute_definition`, `policy_assignment`, `role_member`, `role_inheritance`, `data_source_access` |
| `resource_id` | UUID | The ID of the entity that was changed |
| `action` | string | What happened: `create`, `update`, `delete`, `assign`, `unassign`, `add_member`, `remove_member`, `add_parent`, `remove_parent`, `grant_access`, `revoke_access` |
| `actor_id` | UUID | The admin user who performed the action |
| `changes` | JSON string (nullable) | A JSON object describing what changed. Shape depends on the action — see below. |
| `created_at` | datetime | When the mutation occurred |

### Changes JSON shape

| Action | JSON shape | Contents |
|---|---|---|
| `create` | `{"after": {...}}` | Full snapshot of the new entity (secrets excluded) |
| `update` | `{"before": {...}, "after": {...}}` | Only the fields that changed |
| `delete` | `{"before": {...}}` | Full snapshot of the deleted entity |
| `assign` / `unassign` | `{assignment_id, datasource_id, scope, ...}` | Flat JSON with relationship identifiers |
| `add_member` / `remove_member` | `{user_id, role_id}` | Who was added/removed |

::: warning Secrets are never logged
`config`, `password_hash`, and `decision_fn` source code are excluded from audit entries. When these fields change, the audit entry records a boolean flag like `"config_changed": true` instead of the actual value.
:::

## See also

- **[Audit & Debugging](/guides/audit-debugging)** — how to use the audit logs to debug policy behavior
- **[Troubleshooting](/operations/troubleshooting)** — diagnostic scenarios that reference audit log fields
