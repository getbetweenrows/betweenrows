---
title: Per-User Exceptions to Role-Level Denies
description: Grant a single user in a role access to a column that's denied for the rest of the role, without weakening the deny-wins invariant.
---

# Per-User Exceptions to Role-Level Denies

## Problem

You have a role — say `analysts` — with a `column_deny` on a sensitive column like `customers.ssn`. Most analysts must not see SSNs. But one specific analyst, Alice, needs access: she runs the quarterly compliance audit and the raw SSN is part of her job.

The naïve instinct is to add a `column_allow` scoped to Alice. **That does not work.** BetweenRows enforces [deny-wins](/concepts/policy-model#_2-deny-always-wins) across all roles, scopes, and priorities — a permit policy cannot override a deny policy regardless of source. This is a security invariant, formalized in the [threat model](/concepts/threat-model#_38-deny-wins-across-roles). You can't grant your way out of a deny.

So how do you express "everyone in `analysts` is denied SSN, except Alice"?

## Ingredients

- **[Decision Functions](/guides/decision-functions)** — JavaScript attached to a policy that decides whether the policy fires at all
- **[User Attributes (ABAC)](/guides/attributes)** — custom per-user key-value pairs
- **[Column Allow & Deny](/guides/policies/column-allow-deny)** — the deny policy whose application we're going to make conditional

The decision function gives us a way to skip the deny for specific users *without* altering deny-wins at the engine level. The deny still exists in the policy list; it just doesn't apply to users who meet the exception criteria.

## Solution

We'll grant the exception via a user attribute so that adding or removing exceptions is a single attribute flip rather than a JavaScript edit.

### 1. Define the exception attribute

Go to **Attribute Definitions → Create** and define:

- **Key:** `pii_access`
- **Entity type:** `user`
- **Display name:** `PII Access Exemption`
- **Value type:** `boolean`
- **Default value:** `false`
- **Description:** `When true, the user is exempt from the role-level PII column deny.`

Setting the default to `false` means any user without the attribute explicitly set is treated as not exempt — fail-closed by construction.

### 2. Create the decision function

Go to **Decision Functions → Create**. Name it `pii-exemption-check` and paste:

```js
function evaluate(ctx, config) {
  // Skip the policy (fire: false) if the user is exempt.
  // Any other user: policy fires normally.
  if (ctx.session.user.pii_access === true) {
    return { fire: false };
  }
  return { fire: true };
}
```

Set:

- **Evaluate context:** `session` — evaluates once per connection at the visibility layer. Cheaper than per-query, and the exception is static per user, so session context is the right choice.
- **On error:** `deny` — if the function throws, default to firing the deny (fail-closed — see the Pitfalls section for the counterintuitive naming).
- **Log level:** `off` for production; `info` while you're iterating.

Save.

### 3. Create the `column_deny` policy with the decision function attached

Go to **Policies → Create**:

- **Name:** `analysts-deny-ssn`
- **Policy type:** `column_deny`
- **Targets:**
  ```json
  [
    {
      "schemas": ["public"],
      "tables": ["customers"],
      "columns": ["ssn"]
    }
  ]
  ```
- **Decision function:** `pii-exemption-check` (selected from the dropdown)

Save, then assign the policy to the `analysts` role on your data source.

### 4. Grant the exception

On Alice's user page, set `attributes.pii_access = true`. Save.

The change propagates to all of Alice's active connections on her next query — no reconnect needed. BetweenRows invalidates her cached session state and rebuilds her virtual schema in the background.

### 5. Verify

As Alice (exempt):

```sh
psql 'postgresql://alice@proxy:5434/demo' -c "SELECT id, name, ssn FROM customers LIMIT 3;"
```

Alice sees the `ssn` column in the result set.

As Bob (another `analysts` member without the attribute):

```sh
psql 'postgresql://bob@proxy:5434/demo' -c "SELECT id, name, ssn FROM customers LIMIT 3;"
```

Bob gets an error: `column "ssn" does not exist`. The column isn't in his virtual schema at all — visibility-level enforcement removed it at connect time.

As Bob with `SELECT *`:

```sh
psql 'postgresql://bob@proxy:5434/demo' -c "SELECT * FROM customers LIMIT 3;"
```

Bob gets rows back with `id`, `name`, and the other non-denied columns. No SSN, no error. The `SELECT *` expands against his virtual schema, which never contained `ssn`.

## Why this works

The `column_deny` policy is still assigned to the `analysts` role and Bob is still a member — deny-wins is unchanged. What the decision function does is **gate whether the deny applies at all**, per user, at the point where policies are resolved into an effective set.

When a user connects, BetweenRows walks their role memberships and scopes to build their effective policy set. For each deny policy, it evaluates any attached decision function *before* the policy lands in the user's deny set. If the function returns `{ fire: false }`, the policy is skipped — it never becomes part of that user's effective denies, so downstream enforcement has no deny to apply to them.

For Alice, the analyst deny set for `customers.ssn` is empty. For Bob, the deny is present and the visibility layer removes `ssn` from his virtual schema at connect time. Both flow through the same enforcement pipeline; only the *input* (whose deny set contains the policy) differs.

Because `evaluate_context` is `session`, the decision runs once when each user connects — not per query. Alice's virtual schema is computed without the deny; Bob's is computed with it. Both are cached until the next policy or attribute mutation triggers a rebuild.

This preserves the deny-wins invariant: the deny *still* wins wherever it applies. The exception isn't a workaround that overrides deny-wins — it's a declaration, upstream of enforcement, that the policy doesn't apply to certain users in the first place.

## Variations

### Exception by role membership instead of by attribute

If the exception is shaped like "users in a specific role get PII access", check the built-in `roles` field instead of a custom attribute:

```js
function evaluate(ctx, config) {
  if (ctx.session.user.roles.includes('pii-access')) {
    return { fire: false };
  }
  return { fire: true };
}
```

Grant the exception by adding users to the `pii-access` role. The decision function stays static, exception management routes through standard role membership (already captured in the admin audit log), and you don't need an attribute definition.

### Static list of exempt users

For a small, rarely-changing exception list, check username directly:

```js
function evaluate(ctx, config) {
  const exempt = ['alice', 'dana', 'erin'];
  if (exempt.includes(ctx.session.user.username)) {
    return { fire: false };
  }
  return { fire: true };
}
```

This is less scalable than the attribute approach but fine for 2–5 exempt users that don't change often. The trade-off: every change is a JavaScript edit plus a decision function version bump.

### Exception with a time window

Combine the attribute check with a time window so the exception auto-expires:

```js
function evaluate(ctx, config) {
  if (ctx.session.user.pii_access === true &&
      ctx.session.time.now < config.exempt_until) {
    return { fire: false };
  }
  return { fire: true };
}
```

Set `exempt_until` in the decision function's `config` JSON and update it as the compliance window changes. `ctx.session.time.now` is an ISO 8601 / RFC 3339 string, so lexicographic comparison with another ISO string does the right thing.

### Audit trail for exception events

Because the exception is stored as a user attribute, every grant and revoke is automatically captured in the admin audit log — attribute changes record which attribute changed, the before/after values, and who made the change. See [Audit Log Fields](/reference/audit-log-fields) for the schema and [Admin REST API](/reference/admin-rest-api) for querying.

This turns "who has PII access and why?" from a maintenance burden into a queryable record.

## Pitfalls

### Don't try to `column_allow` your way out

The most natural-feeling "fix" is to create a `column_allow` policy scoped to the exempt user, with a high-priority assignment. **This does not work.** BetweenRows enforces deny-wins across all roles, scopes, and priorities — a permit cannot override a deny regardless of source. The allow and the deny meet in the enforcement layer, and the deny always takes precedence.

See [Policy Model → Deny always wins](/concepts/policy-model#_2-deny-always-wins) and the [threat model entry on cross-role deny-wins](/concepts/threat-model#_38-deny-wins-across-roles), which defines this as a security invariant backed by an integration test.

Decision functions work because they operate *before* deny-wins — they control whether the deny policy applies at all for a given user, not whether it can be overridden at enforcement time.

### Don't use `evaluate_context: query` for static exceptions

If the exception is based on user identity alone (username, role, attribute — none of which change mid-session), use `evaluate_context: session`. The session context evaluates once at connect time and the decision is baked into the user's cached virtual schema.

`evaluate_context: query` evaluates the decision function on *every query*, which is wasted work. More subtly: with `query` context, the deny is *deferred* — the column stays visible in `information_schema` for every user, and is only removed from query results at query time. That changes discoverability: exempt users and non-exempt users see the same schema metadata, but non-exempt users get runtime errors when they explicitly reference the column. If you want metadata-level hiding (the 404-not-403 property), you need `session` context.

Reserve `query` context for decisions that genuinely depend on query shape (table count, row count estimate, time of day, query text patterns).

### `on_error: deny` on a deny policy is counterintuitive — but it is the fail-closed setting

If the decision function throws (a JavaScript runtime error, a missing context field, a corrupted `config`), the `on_error` setting decides what to do. Two options:

- `on_error: deny` — the policy fires on error. For a `column_deny` policy, "the policy fires" means the column is denied. That is fail-closed: on any ambiguity, the sensitive column stays hidden.
- `on_error: skip` — the policy does not fire on error. For a `column_deny` policy, this means the column stays visible. That is fail-open and should be avoided for anything protecting real sensitive data.

The word "deny" in `on_error: deny` refers to the *policy's effect*, not "deny access." For a deny policy, the effect of firing *is* denial — so `on_error: deny` reads awkwardly but means the right thing. Double-check every deny policy you write uses `on_error: deny`, not `skip`.

### Don't forget the attribute default

If you define `pii_access` without a `default_value`, users without the attribute set will have `ctx.session.user.pii_access === undefined`. The comparison `=== true` still correctly evaluates to `false`, so fail-closed holds by default — but it's brittle. An explicit `default_value: false` documents the intent and protects against someone later changing the comparison to `!== false` or `!= true` (which would flip the semantics and silently open the exemption to everyone).

### Don't confuse "exempt from deny" with "allowed to query"

The decision function only controls whether the `column_deny` applies. It does not grant any other access. In `policy_required` access mode, Alice still needs a matching `column_allow` for `customers.ssn` to actually see the column — the allow grants visibility, the deny (when it fires) removes it. The exemption just means the deny step is skipped for Alice; the allow step is still required.

In `open` access mode, no `column_allow` is needed — tables and columns are visible by default, so once the deny is skipped for Alice, she sees the column. Most deny-exception patterns target tables that are already broadly readable with one sensitive column carved out, and open mode or a broad role-level allow is usually already in place.

## Related

- [Multi-Tenant Isolation with Attributes](./multi-tenant-isolation) — the same attribute-gated pattern applied to row filters
- [Decision Functions](/guides/decision-functions) — full reference on context shape, error handling, performance, and the test harness
- [User Attributes (ABAC)](/guides/attributes) — how attributes work, default values, list-valued attributes, and cache invalidation
- [Policy Model → Deny always wins](/concepts/policy-model#_2-deny-always-wins) — the invariant this recipe preserves
- [Threat Model → Deny-wins across roles](/concepts/threat-model#_38-deny-wins-across-roles) — the security vector this invariant defends against
