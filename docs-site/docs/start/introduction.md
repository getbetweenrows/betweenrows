---
title: Introduction
description: BetweenRows is a fully customizable data access governance layer — a SQL-aware proxy that enforces fine-grained access policies across your databases, warehouses, and lakehouses.
---

# Introduction

**BetweenRows** is a fully customizable data access governance layer. It sits between your users and your data sources as a SQL-aware proxy, enforcing fine-grained access policies — masking, filtering, and blocking — in real-time on every query. Define who can see what through an admin UI, and every connection through the proxy is automatically governed. Currently supports PostgreSQL data sources; warehouses and lakehouses are on the roadmap.

![BetweenRows admin console, showing the Users page with seeded demo accounts alice, bob, and charlie](/screenshots/introduction-dashboard-v0.14.png)

## Why BetweenRows

- **No application changes** — policies are enforced at the proxy layer, not in your app code.
- **Row-level filtering** — automatically filter rows based on user identity (role, department, tenant).
- **Column masking** — mask sensitive columns (SSN, email, salary) with expressions, not views.
- **Column & table deny** — hide columns or entire tables from specific users or roles.
- **Full audit trail** — every query is logged with the original SQL, rewritten SQL, and policies applied.
- **RBAC + ABAC** — assign policies via roles, user attributes, or programmable decision functions (JavaScript/WASM).

Built with **Rust**, **DataFusion**, **pgwire**, and **React**.

## The philosophy

BetweenRows is built on three invariants:

1. **Zero-trust defaults.** In `policy_required` mode, tables start invisible. Access must be explicitly granted — there is no "allow all, then restrict."
2. **Deny always wins.** If any policy denies access — from any role, any scope, any source — the deny is enforced. You can layer permit-policies freely and reach for a deny as the final word.
3. **Visibility follows access.** Denied columns don't just get filtered from query results — they disappear from `information_schema` entirely. Users cannot discover what they can't see.

These make the security model tractable to reason about: permitted access is an intersection of explicit grants, and denial is always authoritative.

→ Full explanation: [Policy Model](/concepts/policy-model)

## How it's different from application-layer RLS

Most row-level security is implemented inside the application (`WHERE tenant_id = :user_tenant` in every query) or inside the database (PostgreSQL `CREATE POLICY`). Both approaches couple security to the thing that should be protected.

BetweenRows decouples them. Your application connects to the proxy as if it were PostgreSQL. The proxy rewrites every query at the logical plan level before sending it upstream — row filters, column masks, and access controls are applied uniformly regardless of which tool, ORM, or BI client sent the query. The upstream database sees only policy-compliant queries; your application code is unchanged.

This means:

- Existing tools (DBeaver, Tableau, psql, any ORM) get row-level security for free — no plugin, no integration.
- A single policy definition covers every consumer of the data source.
- Security reviews are tractable: the policy set is a small, named, versioned thing, not a sprawl of `WHERE` clauses across a codebase.

## Who this is for

- **Platform engineers and DBAs** who need row-level security on an existing PostgreSQL database and don't want to rewrite their app.
- **Security and compliance teams** evaluating whether a proxy-based approach meets their threat model.
- **Data team leads** who want consistent access controls across BI tools, notebooks, and applications.

## Use with your own LLM

Every page has a **Copy as Markdown** button — click it to copy the page content, then paste it into ChatGPT, Claude, Gemini, or any chat model as context for your questions.

For the entire docs set at once, fetch [`/llms-full.txt`](/llms-full.txt) (or give the URL to a model with web-reading support). A shorter section index is at [`/llms.txt`](/llms.txt).

## Where to go next

- **[Quickstart](/start/quickstart)** — install the proxy, connect a database, write your first policy, verify it works. Under 15 minutes.
- **[Policy Model](/concepts/policy-model)** — the philosophy: zero-trust defaults, deny-wins, visibility-follows-access.
- **[Security Overview](/concepts/security-overview)** — for security and compliance reviewers. Trust boundaries, guarantees, and the deployment checklist.
- **[Architecture](/concepts/architecture)** — two-plane design, request lifecycle, how policies are applied during query planning.
- **[Threat Model](/concepts/threat-model)** — the full attack-vector catalog with defenses and tests.
