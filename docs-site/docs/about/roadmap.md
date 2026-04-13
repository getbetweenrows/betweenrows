---
title: Roadmap
description: What's shipped, what's in progress, and what's planned for BetweenRows. Not a commitment — a window into where the project is heading.
---

# Roadmap

::: info
This roadmap is a reflection of current thinking, not a contract. Items can be reordered, reshaped, or dropped based on user feedback. File an issue on GitHub if you care about a specific item — the signal genuinely shapes priority.
:::

## Shipped

- **Row filters** — `row_filter` policies with template variables (`{user.*}`) and typed literal substitution.
- **Column masks** — `column_mask` policies with arbitrary SQL mask expressions.
- **Column allow / column deny** — explicit allowlist and denylist policy types.
- **Table deny** — hide tables from the user's virtual catalog with the 404-not-403 principle.
- **RBAC** — role hierarchies (DAG), cycle detection, depth cap, soft delete, inherited assignments.
- **ABAC** — schema-first user attribute definitions with `string`, `integer`, `boolean`, and `list` value types.
- **Decision functions** — JavaScript functions compiled to WASM via Javy, evaluated at query time via wasmtime with fuel limits and per-call isolation.
- **Query audit log** — every query logged with original SQL, rewritten SQL, and policies applied (success, denied, error, write-rejected).
- **Admin audit log** — append-only record of every management-plane mutation, written atomically with the mutation.
- **YAML policy-as-code** — export and import all policies as YAML via the REST API, with dry-run support.
- **Attribute definitions with `allowed_values` and `default_value`** — enum-constrained attributes with sensible missing-value defaults.
- **Catalog discovery and sync** — allowlist-based catalog with drift detection on re-sync.
- **Two-plane architecture** — data plane (5434) and management plane (5435) on separate ports with independent authentication.

## Next up (actively being worked or scheduled soon)

- **Shadow mode** — per-policy dry-run state. A shadow policy logs what it *would* have done without actually blocking or masking. Removes the "fear of breaking prod" adoption blocker. Each policy gets an `action_status: enforce | shadow` field.
- **Module cache for decision functions** — pre-compile WASM modules per `(decision_function_id, version)` and cache them in the policy hook instead of recompiling from bytes on every evaluation. Reduces per-query WASM overhead from milliseconds to microseconds.
- **WASM linear memory limit** — configurable per-function memory cap to complement the existing fuel limit.
- **Decision function integration tests** — end-to-end tests that exercise decision functions through the full proxy stack (real WASM evaluation via pgwire), in addition to the existing unit tests.
- **Password reset flow** — UI-driven forgot/reset password. Currently the only recovery path is the `proxy user create --admin` CLI rescue.
- **First-class PostgreSQL admin backend** — make the admin plane's Postgres support a tested, supported option so teams can run HA deployments with an external admin DB. Today SQLite is the only tested backend.
- **Audit log retention configuration** — operator-configurable TTL and/or row cap for `query_audit_log` and `admin_audit_log`, with optional export-before-prune. Today operators run their own cron scripts against the admin DB.

## Governance workflows

A per-data-source `governance_workflow` setting controls how governance state (schemas, policies, decision functions, assignments) is authored, reviewed, and deployed. Three tiers:

- **None** (default, today's behavior) — edit in UI, changes take effect immediately.
- **Draft** — edits go to a staged sandbox and must be deployed to go live. Review/preview before production.
- **Code** — governance lives as YAML in a git repo, reviewed via PRs, deployed via CI/CD. Admin UI is read-only for governance on this data source.

Natural progression: start with none, graduate to draft as a team grows, move to code when mature.

**No hybrid.** Each data source picks one workflow. Supporting UI and code simultaneously creates a two-source-of-truth sync nightmare.

## Tag-Based Access Control (TBAC)

- **Policy templates** — separate transformation logic from policy definitions. Update logic once, many policies benefit.
- **Metadata tagging layer** — apply tags (`pii`, `financial`, `deprecated`) to data sources, schemas, tables, and columns. Tags flow down from parent to child unless overridden.
- **Tag-based policy matching** — target policies via `"tag:pii"` instead of names.
- **Auto-classification** — pattern matchers (regex, Luhn, NLP) in the discovery job that automatically tag sensitive data.

## Advanced features on the horizon

### Security & governance

- **Security lineage / sticky security** — ensure security rules stick to data as it moves through CTEs, subqueries, and views (the enforcement is already scan-level, but this is about provenance tracking in the audit log).
- **Data domains** — group data sources into domains (Finance, HR) for delegated administration.
- **Delegated security** — per-domain admin permissions that do not grant query access.
- **Stealth mode** — hide injected filter expressions from `EXPLAIN` and audit logs. Addresses the `EXPLAIN` leakage gap in the current release.
- **JIT (just-in-time) access** — temporary, windowed policy assignments (e.g., 2-hour elevation) triggered by approval workflows.
- **Impact analysis engine** — "what-if" simulation of a policy change against historical query logs to identify breaking changes before enforcement.
- **Policy impersonation (sudo mode)** — "Run as user X" tool to verify policy enforcement in real time.

### Identity integration

- **User attribute sync from IDP** — pull ABAC attributes from identity claims at connect time instead of maintaining them in BetweenRows' own database.
- **Purpose-based access control (PBAC)** — require a validated claim (e.g., a ticket ID from a ticketing system) to unlock specific data lenses. Move beyond roles to purposes.

### Data operations

- **Clean room joins** — blind joins on a sensitive key where the key cannot be leaked in results or filters.
- **Canary rollout** — test new policies against a subset of users (e.g., 10% traffic) before enforcing broadly.

## How to influence the roadmap

1. **File an issue on [GitHub](https://github.com/getbetweenrows/betweenrows/issues)** describing your use case and why the current behavior doesn't work.
2. **Upvote existing issues** that match your needs.
3. **Propose a design** in a GitHub Discussion for non-trivial features. We'd rather talk through the shape before a PR lands.
4. **Send a PR** for well-scoped improvements — see [Install from source](/installation/from-source) and `CONTRIBUTING.md`.

Feedback from early alpha users has an outsized effect on prioritization right now. The project is small; the signal is loud.
