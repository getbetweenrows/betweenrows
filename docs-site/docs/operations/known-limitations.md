---
title: Known Limitations
description: Current limitations, security trade-offs, and alpha caveats — what BetweenRows does not do, what it does oddly, and what to avoid in production.
---

# Known Limitations

An honest list of things BetweenRows does not do, does oddly, or has not tested thoroughly yet. Read this before production deployment.

## Security-adjacent limitations

These are the items security and compliance reviewers should know about. They are current behaviors, not bugs.

### `EXPLAIN` output may leak plan structure

A user who runs `EXPLAIN SELECT * FROM orders` against the proxy may see the injected row filter expressions and other plan structure in the output. This can reveal:

- The contents of a row filter (e.g., the name and value of a tenant attribute).
- The names of columns hidden by `column_deny` if they are referenced in plan annotations.
- The existence of tables hidden by `table_deny` in certain edge cases.

**Currently unmitigated.**

**Deployment guidance:** restrict `EXPLAIN` to trusted users upstream of the proxy, or block it entirely by deploying BetweenRows behind a query filter that rejects `EXPLAIN` for non-trusted users. Do not assume `EXPLAIN` is safe to expose to arbitrary users.

### `column_mask` does not block predicate probing

Column masks apply to the projection output only. A user can still run:

```sql
SELECT id FROM customers WHERE ssn = '123-45-6789';
```

and learn whether a specific SSN exists, even though `SELECT ssn FROM customers` would return masked values. The `WHERE` predicate evaluates against the raw column — by design, because rewriting predicates would break the `row_filter` + `column_mask` composition invariant.

Variants that work the same way:

- **`EXISTS` subqueries:** `SELECT 1 WHERE EXISTS (SELECT 1 FROM customers WHERE ssn = '...')`
- **`JOIN` with a VALUES clause:** `SELECT c.id FROM customers c JOIN (VALUES ('...')) v(probe) ON c.ssn = v.probe`
- **`IN` clauses:** `SELECT id FROM customers WHERE ssn IN ('...', '...')`

**Deployment guidance:** for columns where even existence-testing must be prevented, use **`column_deny`** instead of `column_mask`. Denied columns cannot be referenced in `WHERE` at all — the query fails with a column-not-found error (or SQLSTATE 42501 if the denied column is the only thing selected).

### `column_mask` does not block aggregate inference

Similarly, aggregates operate on raw values:

```sql
SELECT COUNT(DISTINCT ssn) FROM customers;
SELECT MIN(salary), MAX(salary) FROM employees;
SELECT STRING_AGG(ssn, ', ') FROM customers;
```

These return statistical properties or bulk collections of raw values. `column_mask` cannot prevent this because aggregation happens during query execution, not projection — the raw column is still readable at the scan level.

With a small `GROUP BY` group, aggregates can effectively deanonymize individuals:

```sql
SELECT department, COUNT(*), MIN(salary), MAX(salary)
  FROM employees GROUP BY department;
```

A department of one person means `MIN = MAX = that_person.salary`.

**Deployment guidance:** use `column_deny` for columns where cardinality, range, or bulk values are sensitive. If you need to allow projection but not aggregation, there is currently no mechanism — file an issue if this is a blocker for your use case.

### Compound column-mask expressions produce masked-then-concatenated output

This query:

```sql
SELECT ssn || ' (masked)' FROM customers;
```

returns:

```
***-**-6789 (masked)
```

The mask is applied at the scan level, so downstream expressions reference the already-masked value. The result is correct — raw SSN is not exposed — but the output is not what the author of the SELECT list might have intended. Users may see aesthetically odd output when they concatenate masked columns with other values.

Internally, this is a known behavior. A future improvement could preserve the original SELECT-list expression structure while still enforcing the mask, but that adds significant complexity to the query rewrite path. For now, the behavior is: the value is masked, but derived expressions that reference the masked column operate on the masked value.

## SQL client compatibility

### Metadata queries from some clients may fail

BetweenRows implements a subset of `pg_catalog` and `information_schema` — enough for `psql`, TablePlus, and DBeaver to show the schema tree and run queries. Some clients send additional metadata queries (introspection, autocompletion, schema browsing) that BetweenRows may not support yet. Symptoms:

- The client connects without error.
- Queries run fine.
- Schema browsing in the client's sidebar is empty or shows errors.

**If this happens with your client, please file a [GitHub issue](https://github.com/getbetweenrows/betweenrows/issues)** with the client name, version, and any error messages. We use these reports to prioritize which metadata queries to implement next.

### TLS on the data plane

The current pgwire listener does not terminate TLS. Clients configured with `sslmode=require` or higher will fail to connect. Use `sslmode=disable` and deploy BetweenRows behind a TLS-terminating load balancer, service mesh, or Cloudflare Tunnel.

## Column type limitations

### `regclass` and `regproc` columns are dropped during discovery

The `datafusion-table-providers` crate does not handle PostgreSQL's `regclass` / `regproc` types. BetweenRows skips these columns during catalog discovery — they are invisible to the proxy and cannot be queried through it.

**Workaround:** if you need to read these columns, cast them to `TEXT` upstream with a view. The view's `TEXT` column is then visible and queryable through the proxy.

### `json` and `jsonb` appear as `TEXT` on the wire

JSON and JSONB columns are announced to clients as PostgreSQL `TEXT` type (because DataFusion maps both to Arrow `Utf8`). Data is correct — values are readable and functionally complete — but some GUI tools won't show a JSON-specific editor or syntax highlighting.

JSON operators and functions (`->`, `->>`, `?`, `json_length`, `json_keys`) work normally because `datafusion-functions-json` is registered on every session. For filter pushdown, some operators push to the upstream and some are evaluated in-process by DataFusion.

### `->>` operator precedence issue

Due to a known issue in `sqlparser` 0.59 (the parser DataFusion uses), `col->>'key' = 'val'` parses as `col ->> ('key' = 'val')` — wrong associativity. In practice this is masked when the filter is pushed down to upstream PostgreSQL (which parses it correctly), but appears as a planning error in `EXPLAIN` output.

**Workaround:** add explicit parentheses:

```sql
-- Wrong (misparsed)
WHERE metadata->>'status' = 'active'

-- Correct
WHERE (metadata->>'status') = 'active'
```

Will be fixed when DataFusion upgrades to `sqlparser` 0.60+.

## Write support

### BetweenRows is read-only

The proxy rejects `INSERT`, `UPDATE`, `DELETE`, `DROP`, `TRUNCATE`, `CREATE`, `ALTER`, and all other write statements with SQLSTATE `25006` ("read-only transaction"). Rejected writes are audited as `status: denied` with `error_message: "Only read-only queries are allowed"`.

**This is intentional.** BetweenRows is a read-path proxy. For write access, connect to the upstream database directly (with whatever write-path security controls are appropriate) and use BetweenRows only for read queries that need row/column-level security.

Write support may come in a future major version, but it's a significantly larger problem than read rewriting and is not on the immediate roadmap.

### Some SQL clients send write statements on startup

Some clients issue `SET` statements or temporary-table creation during their startup sequence. BetweenRows allows a small allowlist of read-adjacent statements (`SET`, `SHOW`, `BEGIN`, `COMMIT` and similar) to pass through without rejection. If your client fails on startup with a "read-only" error, file an issue — we may need to extend the allowlist.

## Upgrade and migration

### Downgrades are not supported

SeaORM migrations are forward-only. If an upgrade goes wrong, the only supported recovery is restoring `/data` from a backup and running the older image version. See [Upgrading](/operations/upgrading) and [Backups](/operations/backups).

## Deployment

### Admin UI origin restrictions

If you host the admin UI on a different origin than the REST API (which most users don't), you must set `BR_CORS_ALLOWED_ORIGINS` to the list of allowed origins. Without it, browser requests fail with CORS errors.

### IPv6-only connectivity on Fly.io

The default Fly.io deployment exposes the pgwire port via IPv6. macOS users with IPv6 disabled may see timeouts. See [Install on Fly.io → IPv4-only environments](/installation/fly#ipv4-only-environments) for the WireGuard tunneling workaround.

## Things we are actively tracking (not yet decided)

- Predicate-probing mitigation for `column_deny` beyond projection-level stripping (does `WHERE denied_col = 'x'` error? — needs verification).
- `HAVING` clause behavior with masked columns (does `HAVING MAX(masked_col) > X` reference raw or masked values? — needs verification).
- `CASE WHEN denied_col IS NOT NULL` bypass potential (needs verification).
- Window function `ORDER BY masked_col` ranking leakage (needs verification).
- Full write-path audit support (currently limited to rejection logs).

File an issue on GitHub if any of these block your use case — we use that signal to prioritize.

## See also

- **[Security Overview](/concepts/security-overview)** — threat model and what BetweenRows protects against
- **[Troubleshooting](/operations/troubleshooting)** — for things that feel like limitations but might just be misconfigurations
- **[Roadmap](/about/roadmap)** — planned features, including some mitigations for the limitations above
