---
title: Security Overview
description: For security and compliance reviewers — what BetweenRows is designed to protect against, what it is not, trust boundaries, and the deployment checklist.
---

# Security Overview

This page is a curation layer for the security and compliance audience. It does not make new guarantees — it frames how BetweenRows thinks about its threat model and links to the pages where the specific behaviors are documented. If you are evaluating BetweenRows for a security review, read this page first, then follow the links.

## What BetweenRows is designed to protect against

- **Unauthorized row access.** A user scoped to a subset of rows (by tenant, region, clearance) cannot query rows outside that subset — even if they are a savvy SQL author. Row filters are injected into the DataFusion logical plan at the `TableScan` level, so aliases, CTEs, subqueries, and JOINs cannot bypass them. See [Policy Model](/concepts/policy-model) → *row_filter*.

- **Sensitive column exposure.** Columns flagged as `column_deny` are removed from schema metadata and query results. Columns flagged as `column_mask` are replaced with a transformed value. See [Policy Model](/concepts/policy-model) → *column_deny*, *column_mask*, and the *when to mask vs when to deny* guidance.

- **Unauthorized table access.** Tables flagged as `table_deny` become invisible in `information_schema.tables` and return a "not found" error on query — indistinguishable from a nonexistent table (the *404-not-403 principle*). See [Policy Model](/concepts/policy-model) → *table_deny*.

- **SQL injection via user attributes.** Template variables like `{user.tenant}` substitute typed literal values into the parsed expression tree — the user's attribute value never passes through a SQL parser. A tenant attribute containing `' OR '1'='1` produces `org = 'x'' OR ''1''=''1'` (a single escaped literal), not an injection. See [Template variables reference](/reference/template-expressions).

- **Policy bypass via role tampering.** Role hierarchies are protected against cycle creation, excessive depth (max 10 levels), and time-of-check-time-of-use races. Deactivating a role or removing a user from a role immediately invalidates that user's session contexts on active connections — no reconnect required. See [Users & Roles](/guides/users-roles).

- **Privilege separation between admin and data access.** The two planes are structurally separate: different ports (5434 vs 5435), different authentication mechanisms (password vs JWT), different authorization tables. An admin with no `data_source_access` entries and no policy assignments sees zero data through the proxy. See [Architecture](/concepts/architecture) → *Two planes, two ports*.

- **Audit trail integrity.** Every query that reaches the policy layer is audited — success, denied, error, and write-rejected. Every admin mutation (user/role/policy/datasource/attribute create/update/delete) is written inside the same database transaction as the mutation, so there is no window where a mutation can commit without its audit entry.

## What BetweenRows is NOT designed to protect against

::: warning
Be honest with your security reviewers about these. Misrepresenting the threat model is the fastest way to lose trust.
:::

- **Network-level attacks on the proxy host.** BetweenRows does not terminate TLS on the data plane (pgwire is plaintext in the current release). Deploy the proxy on a private network, behind a TLS-terminating load balancer, or inside a zero-trust mesh. Do not expose port 5434 directly to the internet.

- **Compromised admin credentials.** Anyone with the admin password can rewrite every policy. Treat the admin credential as root: strong password, limited distribution, rotate on staff changes, use the CLI to provision additional admin accounts rather than sharing one.

- **A compromised upstream database.** BetweenRows reduces the blast radius of a compromised *application* credential or a misconfigured BI tool. It does not protect the data at rest, and it provides no defense if the upstream PostgreSQL server itself is compromised.

- **A direct path to the upstream database.** If an attacker can bypass the proxy and connect to the upstream database directly, BetweenRows offers zero protection. The proxy must be the **only** network path to the database. Enforce this with firewall rules, security groups, or private networks.

- **Statistical inference on masked columns.** `column_mask` applies to projection output only. An attacker running `COUNT(DISTINCT ssn)`, `MIN(salary)`/`MAX(salary)`, `STRING_AGG(ssn, ',')`, or `WHERE ssn = '123-45-6789'` can still infer statistical properties of the raw values or test for specific values, because the mask does not affect predicates or aggregates over the raw column. **Use `column_deny` for columns where even statistical inference is unacceptable.** See [Known Limitations](/operations/known-limitations) for the specifics.

- **`EXPLAIN` output leakage.** A user with the ability to run `EXPLAIN` against the proxy may see injected filter expressions and plan structure that would otherwise be hidden. **Currently unmitigated.** Restrict `EXPLAIN` to trusted users upstream of the proxy, or prevent it via a `table_deny`-equivalent mechanism until a dedicated mitigation ships. See [Known Limitations](/operations/known-limitations).

- **Side-channel attacks beyond the 404-not-403 guarantee.** BetweenRows ensures that denied tables return the same error shape as nonexistent tables and that error messages do not leak policy names. Fine-grained timing analysis is not part of the threat model. If timing leakage is a concern for your environment, layer additional mitigations (rate limiting, noise injection) at the network edge.

- **Physical or supply-chain attacks** on the proxy host, the upstream database host, the Docker registry, or the admin user's endpoint device. Standard operational security applies.

## Trust boundaries at a glance

See [Architecture](/concepts/architecture) for the diagram. The short version:

1. **The admin plane is trusted** — anyone who can reach port 5435 and authenticate as an admin can change any policy. Lock it down with network policy, not just passwords.
2. **The data plane is semi-trusted** — authenticated users can run any query, but the proxy rewrites it before execution. An authenticated user who discovers a bypass in the policy engine can escalate, which is why bypass prevention is tested at the `TableScan` level rather than in string rewriting.
3. **The upstream database is trusted** — BetweenRows assumes the upstream is not actively adversarial. If you do not trust the upstream database, BetweenRows is not the right tool.

## Deployment checklist for security reviewers

Use this as a pre-production gate:

- [ ] **Pin the Docker tag** to a specific version (`ghcr.io/getbetweenrows/betweenrows:0.15.0`). Never use `:latest` in production.
- [ ] **Set `BR_ENCRYPTION_KEY` explicitly** to a 64-character hex string (generate with `openssl rand -hex 32`). Do not rely on the auto-generated value.
- [ ] **Set `BR_ADMIN_JWT_SECRET` explicitly** to a strong random value (generate with `openssl rand -base64 32`). Do not rely on the auto-generated value.
- [ ] **Persist `/data` to a durable volume** that is backed up. The encryption key, JWT secret, and admin database all live here.
- [ ] **Place the proxy on a private network.** The data plane port (5434) must be reachable only by intended clients. The admin plane port (5435) must be reachable only by admin operators and CI/CD.
- [ ] **Terminate TLS upstream of the proxy** (load balancer, service mesh, or Cloudflare Tunnel). The current pgwire listener is plaintext.
- [ ] **Use `access_mode: policy_required`** on every production data source. `open` mode is a dev convenience.
- [ ] **Restrict `EXPLAIN`** to trusted users or block it upstream. See [Known Limitations](/operations/known-limitations).
- [ ] **Change the initial admin password** on first boot. Do not leave `BR_ADMIN_PASSWORD` set to a memorable value in your environment.
- [ ] **Monitor the query audit log** for `status = denied` and `status = error`. A spike in either can indicate policy misconfiguration or an attack.
- [ ] **Monitor the admin audit log** for unexpected mutations. Policy creation, role membership changes, and data source access grants are high-signal events.
- [ ] **Subscribe to the [GitHub releases feed](https://github.com/getbetweenrows/betweenrows/releases)** for security advisories and upgrade notices.

## Where to go next

- **[Threat Model](/concepts/threat-model)** — the full attack-vector catalog: every known bypass attempt, its defense, and the tests that verify it.
- **[Architecture](/concepts/architecture)** — two-plane design, request lifecycle, trust boundaries in detail.
- **[Policy Model](/concepts/policy-model)** — the philosophy: zero-trust defaults, deny-wins, visibility-follows-access, and how policies compose.
- **[Known Limitations](/operations/known-limitations)** — the full honesty page: predicate probing, EXPLAIN, aggregate inference, alpha caveats.
- **[Audit & Debugging](/guides/audit-debugging)** — how to read the audit trail when verifying that policies are enforced as intended.
- **[Multi-tenant isolation guide](/guides/recipes/multi-tenant-isolation)** — the flagship use case, end-to-end. A concrete demonstration that policies cannot be bypassed via aliases, CTEs, or subqueries.
- **[License & alpha status](/about/license)** — ELv2 license summary and the alpha disclaimer.
