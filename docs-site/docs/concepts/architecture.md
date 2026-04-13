---
title: Architecture
description: How BetweenRows is structured — the two-plane design, request lifecycle, and trust boundaries between the admin and data planes.
---

# Architecture

BetweenRows ships as a **single binary with two planes**. Understanding the separation between the two planes is the most important mental model for operating BetweenRows safely.

## Two planes, two ports

**Data plane** (port `5434`) — a PostgreSQL wire protocol proxy. SQL clients (`psql`, TablePlus, DBeaver, BI tools, your application) connect here exactly as they would to a real PostgreSQL database. Every query is authenticated, authorized against the user's data source grants, rewritten according to applicable policies, then executed against the upstream database.

**Management plane** (port `5435`) — the admin UI and REST API. Only users with `is_admin = true` can log in. Admins manage users, data sources, policies, roles, attribute definitions, decision functions, and audit logs.

The two planes are **independent**. Being an admin does **not** grant data access. All data access must be explicitly granted via data source assignments and policies. An admin with no policy assignments sees **zero data** through the proxy — this is by design.

```
psql / app
    ↓  PostgreSQL wire protocol (port 5434)
BetweenRows Data Plane
    ├─ Authenticate user (password)
    ├─ Check data source access (user/role/all scopes)
    ├─ Build per-user virtual schema (applies table_deny, column_deny, column_allow)
    ├─ On each query:
    │    ├─ Apply column_mask at scan level
    │    ├─ Apply row_filter below mask
    │    ├─ Apply final Projection (column allow/deny)
    │    └─ Execute via DataFusion
    ├─ Audit the query (original SQL + rewritten SQL + policies applied)
    └─ Stream results back to client
    ↓
Upstream PostgreSQL
```

The management plane runs alongside but has no path to the data plane's query execution:

```
Admin UI / REST API (port 5435)
    ├─ JWT-authenticated requests only
    ├─ CRUD on users, data sources, policies, roles, attributes
    ├─ Discovery jobs (catalog introspection)
    ├─ Query audit log (read-only)
    └─ Admin audit log (append-only)
```

## Trust boundaries

- **Admin access ≠ data access.** The ports are separate, the authentication is separate (admin uses JWT in browser; data plane uses password in the pgwire startup message), and the authorization lives in different tables (`proxy_user.is_admin` for admin, `data_source_access` for data).
- **The proxy cannot be bypassed from inside the network.** If an attacker reaches the upstream PostgreSQL port directly, BetweenRows provides zero protection — it must be the only network path to the database. Use firewall rules, security groups, or private networks to enforce this.
- **The upstream database is trusted.** BetweenRows reduces the blast radius of a compromised application credential or a misconfigured BI tool; it does not defend against a compromised database server.
- **The admin credential is a root credential.** Anyone with the admin password can rewrite every policy. Treat it accordingly: strong password, limited distribution, rotate on staff changes. Use the CLI to create additional admin accounts rather than sharing one.

## Request lifecycle

A single query through the proxy goes through these stages in order:

1. **Authentication (startup).** The client sends a PostgreSQL startup message with `user=alice password=... database=my-datasource`. BetweenRows looks up `alice` in the admin DB, verifies the Argon2id password hash, and checks `data_source_access` for at least one matching entry (user-scoped, role-scoped via inheritance, or all-scoped). No access → connection refused.

2. **Per-user virtual schema build.** On connect, BetweenRows computes the user's visible schema:
   - Start with the cached baseline catalog for the data source (from the last discovery job).
   - Remove tables matched by any `table_deny` policy assigned to this user (via scope `user`, `role` hierarchy, or `all`).
   - Remove columns matched by any `column_deny` policy.
   - In `policy_required` mode, remove tables that have no matching `column_allow` policy.
   - The result is a filtered `SessionContext` used for all queries on this connection.

3. **Query parsing and planning.** The query is parsed with `sqlparser`, rewritten for pg_catalog compatibility, and planned by DataFusion into a logical plan tree (`TableScan` → `Filter` → `Projection` → etc.).

4. **Policy enforcement on the logical plan.** The `PolicyHook` runs in this order:
   - **Column masks are applied at scan level** (a `Projection` injected above each `TableScan` that replaces the masked column). This ensures aliases, CTEs, and subqueries cannot bypass the mask.
   - **Row filters are applied below the mask.** Each `row_filter` policy becomes a `Filter` node between the scan and the mask projection, so the filter always evaluates against raw (unmasked) values. Multiple row filters are AND-combined.
   - **A top-level `Projection`** is then applied to enforce column allow/deny lists — defense-in-depth for any scan that the scan-level pass missed. If all selected columns are stripped, the query returns SQLSTATE `42501` (insufficient privilege).

5. **Execution and streaming.** The rewritten plan is executed via DataFusion, which pushes down filters to the upstream PostgreSQL where possible, then streams Arrow record batches back and encodes them as PostgreSQL wire protocol rows for the client.

6. **Audit logging.** Every auditable query — success, denied, error, write-rejected — writes a row to `query_audit_log` asynchronously. The audit entry captures the original SQL, the serialized rewritten SQL (produced by `datafusion::unparser`), the list of policies that fired with their versions, client IP, application name, and execution time.

## Why this split matters

Routing security through query planning (rather than string rewriting or view-based approaches) is the reason BetweenRows can enforce policies uniformly across aliases, CTEs, subqueries, JOINs, and nested projections. The filter or mask is attached to the `TableScan` node — the one place in the plan tree that cannot be aliased away. When a user writes `WITH t AS (SELECT * FROM orders) SELECT * FROM t AS o`, DataFusion inlines the CTE and the alias, but the underlying `TableScan(orders)` is still there, still carrying its injected filter.

See the [Policy Model](/concepts/policy-model) for the details of the five policy types and how they compose.

## What lives where

| Component | Location | Purpose |
|---|---|---|
| Data plane | `proxy` binary, port 5434 | pgwire, DataFusion, policy enforcement |
| Management plane | `proxy` binary, port 5435 | REST API (axum) + static admin UI |
| Admin UI | React SPA, served from `5435` | Built with Vite, TanStack Query, Tailwind |
| Admin database | `/data/proxy_admin.db` (SQLite) | Users, policies, datasources, audit logs, attribute definitions |
| Upstream databases | External | Your actual data; BetweenRows reads from them |

::: tip
The single-binary design means one Docker image, one process to monitor, one port matrix to open. It is not a microservices deployment.
:::
