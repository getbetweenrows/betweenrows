---
title: Troubleshooting
description: Common BetweenRows issues and how to diagnose them — connection failures, policy not matching, client compatibility, WASM errors.
---

# Troubleshooting

A field guide for the most common issues. If your problem isn't here, check the [Query Audit log](/guides/audit-debugging) first — it usually explains what's happening.

## Connection issues

### "Cannot connect to the proxy from psql"

```
psql: error: connection to server at "127.0.0.1", port 5434 failed: Connection refused
```

Walk the stack:

1. **Is the proxy process running?**

   ```sh
   docker ps --filter name=betweenrows
   ```

   Should show a running container. If not:

   ```sh
   docker logs betweenrows
   ```

   Look for startup errors — missing env vars (`BR_ADMIN_PASSWORD`), migration failures, port conflicts.

2. **Is the data plane port published?** Your `docker run` needs `-p 5434:5434`. If you forgot, the proxy is running but not reachable from outside the container.

3. **Is anything else on port 5434?**

   ```sh
   lsof -iTCP:5434 -sTCP:LISTEN
   ```

4. **Firewall / security group?** In cloud deployments, check that the data plane port is open between your client and the proxy. Remember: port 5434 should NOT be publicly reachable — connect through a VPN, bastion, or private network.

### "Authentication failed"

```
FATAL: password authentication failed for user "alice"
```

Check in order:

1. **Does `alice` exist?** Go to **Users** in the admin UI. If not, create her.
2. **Is the password correct?** Reset it via **Users → alice → Change Password** in the admin UI.
3. **Is `alice.is_active = true`?** Deactivated users cannot connect.
4. **Does `alice` have data source access?** She needs at least one matching entry in `data_source_access` — user-scoped, role-scoped (via an active role), or all-scoped. Check the data source's **User Access** and **Role Access** tabs.
5. **Is the data source name in the connection string correct?** The database field in the psql URL must exactly match the data source name you created in the admin UI (`postgresql://alice:secret@host:5434/my-datasource`).

### "SSL/TLS error" on connect

BetweenRows' data plane does not currently terminate TLS. If your client is trying `sslmode=require` or higher, it will fail. Use `sslmode=disable` on the direct connection, and terminate TLS upstream of the proxy (load balancer, service mesh, or Cloudflare Tunnel).

```sh
psql 'postgresql://alice:secret@127.0.0.1:5434/my-datasource?sslmode=disable'
```

See [Security Overview](/concepts/security-overview) for why TLS termination is deployment-time responsibility.

### "My SQL client connects, but schema browsing is empty"

Some SQL clients (DBeaver, DataGrip, some Metabase configurations) send metadata queries against `pg_catalog` or `information_schema` that BetweenRows may not fully support yet. Symptoms:

- The client connects without error.
- The sidebar shows no tables.
- Simple `SELECT * FROM mytable` works fine.

Workarounds:

- **Refresh the schema** in your client (usually a right-click or F5).
- **Use `\dt` in psql** to confirm tables are visible through BetweenRows' own catalog introspection. If psql sees them, the data plane is fine — it's your specific client's metadata queries that are failing.
- **Report the issue** on [GitHub](https://github.com/getbetweenrows/betweenrows/issues) with the client name, version, and any error messages.

See [Known Limitations](/operations/known-limitations) → *metadata queries* for the broader picture.

## Data source issues

### "Test Connection fails when creating a data source"

1. **Is the upstream database reachable from inside the proxy container?** If the proxy is in Docker and the upstream is on the host, use `host.docker.internal` (Docker Desktop) instead of `127.0.0.1`. If both are in Docker, use the container network and the upstream container's name.
2. **Is the upstream user and password correct?** Test the same credentials with a separate psql from the proxy host.
3. **Does the upstream user have permission to read the catalog?** BetweenRows needs to query `information_schema.schemata`, `information_schema.tables`, and `information_schema.columns` during discovery. Granting `pg_read_all_data` (or read on the relevant schemas) is usually enough.
4. **Check the proxy logs** (`docker logs betweenrows`) for the specific error message — connection refused vs authentication failed vs permission denied all indicate different things.

### "Discovery shows no schemas"

The upstream user doesn't have visibility into any schemas with user-visible tables. Check:

1. Does the user have `USAGE` on at least one schema and `SELECT` on at least one table?
2. Are you looking at the right database? The data source `config.db` field must point at the correct upstream database.

### "Discovery shows tables but columns are missing"

Some column types are not supported by the underlying `datafusion-table-providers` crate (notably `regclass`, `regproc`). These are dropped during discovery — see [Known Limitations](/operations/known-limitations).

## Policy issues

### "My row filter isn't being applied"

See the full walkthrough in [Debug a policy with the audit log](/guides/audit-debugging). Short version:

1. Check the Query Audit entry for the test query.
2. Is your policy listed in `Policies applied`? If not, targets didn't match — check schema/table names.
3. If listed, does the `Rewritten query` contain the expected filter? If not, check the filter expression and the user's attribute values.

### "The policy returns zero rows when it shouldn't"

Common cause: an attribute used in the filter is missing on the user, and the attribute definition's `default_value` is NULL. `WHERE tenant = NULL` evaluates to NULL → false → zero rows.

Fix: set a default value on the attribute definition, or set the attribute on the user.

### "Column mask returns original values"

1. **Is the policy enabled?** Disabled policies are not enforced.
2. **Does the target `columns` array exactly match the catalog column name?** Case-sensitive. If the upstream column is `SSN` and your target is `ssn`, the policy doesn't fire.
3. **Is another `column_mask` policy with a lower priority number winning?** Only the highest-precedence mask applies per column.
4. **Does the expression validate?** Open the policy in the admin UI; invalid expressions are rejected at save time, but a recent bug could theoretically slip by — check the audit log's `Policies applied` JSON for any error field.

## Query / WASM issues

### "Decision function on_error='deny' is firing unexpectedly"

The WASM runtime raised an error during evaluation. Check:

1. **Log output from the decision function.** Set the policy's `log_level` to `info` and re-run the query. Captured `console.log` output appears in the `policies_applied` audit field.
2. **Fuel exhaustion.** Decision functions are capped at 1,000,000 WASM instructions. An infinite loop or very expensive computation triggers fuel exhaustion. Optimize the function or simplify it.
3. **Invalid return shape.** The function must return `{ fire: boolean }` — anything else is treated as an error. Check the JS source.
4. **Javy version mismatch.** If the function was compiled with an older Javy version, a proxy upgrade may have bumped the harness requirements. Re-save the function to trigger a recompile.

### "Query fails with SQLSTATE 42501"

BetweenRows returns this when all selected columns are stripped by `column_deny`. The error message lists the restricted columns. Either:

1. Remove the denied columns from your `SELECT` list.
2. Request access to those columns via a `column_allow` policy assigned to your user.

This is different from "table not found" — the table is visible, but the specific columns you asked for are denied.

## Performance issues

### "Queries feel slower than direct PostgreSQL"

BetweenRows adds query planning, policy evaluation, and Arrow → pgwire encoding overhead. For small queries this is typically under 10ms. For large result sets, most of the overhead is in encoding.

Check:

1. **The `execution_time_ms`** in the Query Audit entry. Is it close to what the upstream would take directly?
2. **Filter pushdown.** Run `EXPLAIN SELECT ...` and check if filters are pushed down to the upstream. Pushdown depends on the filter expression — simple equality and range predicates push, complex expressions may not.
3. **Decision functions on hot-path policies.** Each decision function evaluation is ~1ms. A policy with a decision function evaluated on every query adds up. Consider moving the condition into a `CASE WHEN` in the filter expression if possible.

### "Startup takes a long time"

Most startup time is migrations (fast) and catalog cache priming. If startup is over 10 seconds:

1. **Check the admin database size.** A very large `query_audit_log` can slow the migration phase. Consider truncating old audit entries.
2. **Check upstream reachability.** If the proxy tries to warm caches for upstream databases that are unreachable, it can take a while to time out.

## Getting help

- **[GitHub Issues](https://github.com/getbetweenrows/betweenrows/issues)** — bug reports and questions. When filing an issue, include:
  - The running BetweenRows version (visible in the admin UI footer, or via `GET /health` on the admin plane).
  - The relevant admin logs (`docker logs betweenrows`).
  - The Query Audit entry for the failing query, if the issue is policy-related.
- **[Known Limitations](/operations/known-limitations)** — check here first to see if your issue is a known behavior rather than a bug.
- **[Security Overview](/concepts/security-overview)** — for threat-model and deployment questions.
