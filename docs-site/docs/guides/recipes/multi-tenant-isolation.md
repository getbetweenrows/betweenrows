---
title: Multi-Tenant Isolation with Attributes
description: The flagship BetweenRows use case — one row_filter policy, an arbitrary number of tenants, driven by user attributes.
---

# Multi-Tenant Isolation with Attributes

Multi-tenancy is the classic row-level security problem. You have a shared database with rows from many tenants, and each user belongs to one (or a few) tenants. Every query must be scoped so that users only see their own tenant's data — *without* rewriting every query in your application.

BetweenRows solves this with **one `row_filter` policy** plus **one user attribute**. Adding new tenants is a matter of creating a user and setting their `tenant` attribute — no new policies, no redeploy.

## Setup

This guide uses the canonical e-commerce schema from the demo:

- **`customers`** — customer records, each tagged with an `org`.
- **`orders`** — order records, each tagged with an `org`.
- **`products`** — product records, each tagged with an `org`.
- **`support_tickets`** — support tickets, each tagged with an `org`.

Three tenants exist: `acme`, `globex`, `stark`. Each tenant has 10 customers, 20 products, 34 orders, and ~50 support tickets (numbers come from the canonical `scripts/demo_ecommerce` seed).

## 1. Define the tenant attribute

1. Go to **Attribute Definitions → Create**.
2. Fill in:
   - **Key:** `tenant`
   - **Entity type:** `user`
   - **Display name:** `Tenant`
   - **Value type:** `string`
   - **Allowed values:** `["acme", "globex", "stark"]` (optional enum constraint)
   - **Description:** `The customer tenant this user belongs to`
3. Save.


The `allowed_values` list makes the admin UI show a dropdown when setting the attribute on a user, and the API rejects values outside the enum with a 422.

## 2. Create three users

1. Create `alice`, set `attributes.tenant = "acme"`.
2. Create `bob`, set `attributes.tenant = "globex"`.
3. Create `charlie`, set `attributes.tenant = "stark"`.


## 3. Grant data source access

On your `demo_ecommerce` data source, add all three users under **User Access**. (In a real deployment, you would use a `data_source_access` with scope `all` or assign via a role.)

## 4. Create one row filter policy

This is the key step. **One** policy covers all three tenants.

1. **Policies → Create.**
2. Fill in:
   - **Name:** `tenant-isolation`
   - **Policy type:** `row_filter`
   - **Targets:** one entry that covers every tenant-scoped table:
     ```json
     [
       {
         "schemas": ["public"],
         "tables": ["customers", "orders", "products", "support_tickets"]
       }
     ]
     ```
   - **Definition:** `{ "filter_expression": "org = {user.tenant}" }`
3. Save.
4. On the data source page, assign `tenant-isolation` with scope `all`.


That's the entire policy layer. No per-tenant policy, no per-user policy.

## 5. Verify

Connect as Alice:

```sh
psql 'postgresql://alice:Demo1234!@127.0.0.1:5434/demo_ecommerce' -c "SELECT org, COUNT(*) FROM orders GROUP BY org;"
```

Expected:

```
 org  | count(*)
------+----------
 acme |       34
```

Only `acme` rows. As Bob:

```sh
psql 'postgresql://bob:Demo1234!@127.0.0.1:5434/demo_ecommerce' -c "SELECT org, COUNT(*) FROM orders GROUP BY org;"
```

```
  org   | count(*)
--------+----------
 globex |       34
```

As Charlie:

```
  org  | count(*)
-------+----------
 stark |       34
```

![Query Audit Log showing alice, bob, and charlie each running the same SELECT org, COUNT(*) FROM orders query against the shared demo_ecommerce datasource — each audit row carries the same tenant-isolation policy and returns a different per-tenant result](/screenshots/multi-tenant-audit-v0.14.png)

## 6. Verify that bypass attempts fail

Connect as Alice and try every clever thing a curious SQL author might try:

```sql
-- 1. Alias bypass
SELECT * FROM orders AS o;

-- 2. CTE bypass
WITH t AS (SELECT * FROM orders) SELECT * FROM t;

-- 3. Subquery bypass
SELECT * FROM (SELECT * FROM orders) sub;

-- 4. JOIN bypass
SELECT o.id, c.first_name
  FROM orders o
  JOIN customers c ON o.customer_id = c.id;

-- 5. OR short-circuit
SELECT * FROM orders WHERE 1=1 OR org != 'acme';
```

**All five return only Alice's `acme` rows.** The row filter is attached to the `TableScan` node in DataFusion's logical plan, which is resilient to:

- Aliases — the `TableScan` carries the real table name regardless of alias.
- CTEs — DataFusion inlines CTEs during planning; the `TableScan` persists.
- Subqueries — same inlining behavior; the `TableScan` persists.
- JOINs — row filters are applied to each `TableScan` independently. The filter on `orders` sits below the JOIN; the filter on `customers` sits below it too. Both apply.
- OR expressions — the injected filter is a separate `Filter` node AND'd with the user's WHERE clause. `WHERE (user_where) AND (policy_filter)`.

## 7. Add a fourth tenant — no new policies needed

Now the payoff. You sign a new customer, `initech`. Add them:

1. Go to **Attribute Definitions → tenant**. Add `initech` to `allowed_values`.
2. Create user `david`, set `attributes.tenant = "initech"`.
3. Grant David access to `demo_ecommerce`.


David can now connect and query. He sees only `initech` rows. **No new policy was created.** The single `tenant-isolation` policy covers him automatically because `{user.tenant}` expands to his attribute value at query time.

Scaling to 50 tenants? Same policy. 500? Same policy. The only thing that grows is the `users` table and their `tenant` attribute values.

## Extending the pattern

### Multiple tenants per user

A consultant who works with multiple clients needs to see rows from multiple tenants. Change the attribute type to `list`:

1. Delete the single `tenant` attribute definition (or redefine it).
2. Create a new attribute definition with `key: "organizations"`, `value_type: "list"`, `allowed_values: ["acme", "globex", "stark", "initech"]`.
3. Set `david.organizations = ["acme", "globex"]`.
4. Change the filter expression to `org IN ({user.organizations})`.


At query time, `{user.organizations}` expands to `'acme', 'globex'` (multiple typed literals), and the filter becomes `org IN ('acme', 'globex')`. David sees both tenants' rows. Alice still only sees `acme` if she has `organizations = ["acme"]`.

### Tenant isolation + column masking

Layer two separate policies:

```yaml
- name: tenant-isolation
  policy_type: row_filter
  targets: [{ schemas: ["public"], tables: ["customers"] }]
  definition: { filter_expression: "org = {user.tenant}" }

- name: mask-customer-ssn
  policy_type: column_mask
  targets: [{ schemas: ["public"], tables: ["customers"], columns: ["ssn"] }]
  definition: { mask_expression: "'***-**-' || RIGHT(ssn, 4)" }
```

Alice now sees only her tenant's customers, and their SSNs are masked. Row filters and column masks compose cleanly — the filter evaluates first against raw values, then the mask applies to the result set.

### Admin bypass

You want an admin user to see all tenants. Use a decision function that skips the policy for users in the `admin` role:

```js
function evaluate(ctx) {
  return { fire: !ctx.session.user.roles.includes('admin') };
}
```

Attach this decision function to the `tenant-isolation` policy. For users in the `admin` role, `fire: false` skips the policy entirely → they see all rows. For everyone else, the policy fires normally.

Alternatively, use a `CASE WHEN` in the filter expression:

```sql
CASE WHEN 'admin' = ANY({user.roles}) THEN true ELSE org = {user.tenant} END
```

The decision function is preferred when the same condition gates multiple policies.

## What you learned

- One row filter policy with a template variable scales to any number of tenants.
- Aliases, CTEs, subqueries, JOINs, and OR expressions cannot bypass a row filter.
- User attributes are the natural home for per-tenant values; policies stay generic.
- Adding a new tenant is a user-management task, not a policy-management task.
- Row filters and column masks compose safely.

## Next steps

- **[Audit & Debugging](/guides/audit-debugging)** — verify policies via the rewritten SQL
- **[Users & Roles](/guides/users-roles)** — RBAC model and role inheritance
- **[User Attributes](/guides/attributes)** — defining and assigning tenant attributes
- **[Template Expressions](/reference/template-expressions)** — all variable types and NULL semantics
- **[Threat Model](/concepts/threat-model)** — the security vectors this pattern addresses
