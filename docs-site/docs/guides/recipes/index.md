---
title: Recipes
description: Applied patterns that combine BetweenRows features to solve common real-world access control problems.
---

# Recipes

Recipes are worked examples that combine BetweenRows features to solve concrete real-world problems. Where the [feature guides](/guides/data-sources) explain each feature on its own, recipes show how to assemble them into the solutions users actually reach for.

Each recipe follows the same shape:

- **Problem** — the real-world need the recipe addresses
- **Ingredients** — which BetweenRows features the recipe uses
- **Solution** — step-by-step walkthrough with concrete policies, attributes, and queries
- **Why this works** — how the solution maps to the policy model invariants
- **Variations** — common tweaks and extensions
- **Pitfalls** — naïve approaches that look right but don't work, and why
- **Related** — adjacent patterns

## Available recipes

- **[Multi-Tenant Isolation with Attributes](./multi-tenant-isolation)** — one `row_filter` policy plus one user attribute scales to any number of tenants. The flagship BetweenRows use case.
- **[Per-User Exceptions to Role-Level Denies](./deny-exceptions)** — grant one user in a role access to a column that's denied for the rest of the role, using decision functions and user attributes — without weakening the deny-wins invariant.

## More recipes coming

A running list of patterns we plan to document as recipes:

- **Break-glass / emergency access** — temporary attribute flips with a full audit trail
- **Time-based access windows** — decision functions using `ctx.session.time.now`
- **Progressive disclosure by seniority** — masked for juniors, visible for seniors
- **Per-environment data visibility** — dev sees scrubbed values, prod sees real data
- **Cross-tenant support access** — how a support user gets read access across tenants without breaking single-tenant patterns
- **Decision function debugging** — using the audit log and decision function results to diagnose why a policy fired or didn't

## Have a question? Open an issue.

If you're trying to figure out how to model a specific access control need and aren't sure which features to reach for, [open an issue on GitHub](https://github.com/getbetweenrows/betweenrows/issues) and describe your situation. We'll help you find the right pattern — pointing you to an existing recipe if one fits, or working through the approach with you and queueing a new recipe if not.

"How do I do X with BetweenRows?" is a welcome issue type. Use-case questions directly shape which recipes get written next, so don't hesitate to ask.
