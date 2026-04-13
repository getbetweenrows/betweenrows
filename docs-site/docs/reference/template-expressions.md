---
title: Template Expressions
description: Reference for variables in row_filter and column_mask expressions — built-in fields, custom attributes, supported SQL syntax, and NULL semantics.
---

# Template Expressions

Template variables let row filter and column mask expressions reference the authenticated user's identity and custom attributes. The value is substituted as a typed SQL literal at query time, after the expression has been parsed. This makes template variable substitution immune to SQL injection even if the attribute value contains SQL syntax.

## Built-in variables

| Placeholder | Value | Type |
|---|---|---|
| `{user.username}` | The user's username | string |
| `{user.id}` | The user's UUID | string |

Built-in variables always take priority over custom attributes with the same name. An attribute definition with key `username` or `id` is rejected at the API.

### What else is (and isn't) available

Template expressions deliberately expose a minimal surface: the two built-ins above plus any custom attributes you've defined via **Attribute Definitions**. A few notable omissions, with the intended alternative:

| Not available in expressions | Use instead |
|---|---|
| `{user.roles}` — the user's role memberships | Assign the policy at **role scope** (`scope = "role"`). The policy only fires for members of that role, and role resolution happens inside the policy hook — no need to encode `roles.includes(...)` inside the filter body. Embedding role checks in the expression would duplicate work the hook already does and wouldn't show up in the admin UI's per-role policy view. |
| The admin-plane `is_admin` flag on `proxy_user` | Don't gate data-plane logic on this. Admin UI access and data-plane access are separate concerns. If you need "privileged users bypass this row filter", assign the policy with `scope = "role"` and make the exempt users members of the role. |
| `is_active` on `proxy_user` | Deactivated users can't connect to the proxy at all — they never reach the filter evaluator, so there's no expression need. |
| Decision-function context (`ctx.query.*`, `ctx.session.time.*`, `ctx.session.datasource.*`) | Expressions are evaluated once per row against user state and have no visibility into the query plan, wall-clock time, or the data source. If you need any of those, attach a [decision function](/guides/decision-functions) to the policy. |

::: info Expressions vs. decision functions
The template expression path and the decision function path expose slightly different shapes of `user` on purpose:

| | Template expressions (`{user.KEY}`) | Decision functions (`ctx.session.user.KEY`) |
|---|---|---|
| Built-ins | `username`, `id` | `id`, `username`, `roles` |
| Custom attributes | All of them, as typed SQL literals | All of them, as typed JSON values |
| Roles | Use role-scoped assignments instead | `ctx.session.user.roles: string[]` |

Decision functions get the roles array because they run JavaScript and often need role-conditional branching *inside* the function body (for example, `return { fire: !ctx.session.user.roles.includes("admin") }` as a privileged-user bypass). Template expressions don't need the array — the same effect is already expressible by assigning the policy at role scope.
:::

## Custom attribute variables

Any attribute defined via **Attribute Definitions** can be referenced as `{user.KEY}`. The produced literal is typed according to the definition's `value_type`.

| `value_type` | Placeholder example | Produced literal |
|---|---|---|
| `string` | `{user.region}` | `'us-east'` (Utf8) |
| `integer` | `{user.clearance}` | `3` (Int64) |
| `boolean` | `{user.is_vip}` | `true` (Boolean) |
| `list` | `{user.departments}` | `'eng', 'sec'` (multiple Utf8 — use with `IN`) |

### List attributes

List attributes expand to multiple comma-separated string literals — use with `IN`:

```sql
department IN ({user.departments})
```

For a user with `departments = ["engineering", "security"]`, this becomes:

```sql
department IN ('engineering', 'security')
```

An empty list expands to `NULL`:

```sql
department IN (NULL)
```

which evaluates to false (zero rows), consistent with SQL three-valued logic.

## Parse-then-substitute pattern

BetweenRows parses the filter or mask expression into a DataFusion expression tree **first**, then replaces placeholder identifiers with typed literal values. The attribute value never passes through the SQL parser.

```
org = {user.tenant}
       ↓ parsed (placeholder is a binding)
Expr::BinaryExpr(col("org"), Eq, Placeholder("user.tenant"))
       ↓ substituted at query time with user's tenant attribute
Expr::BinaryExpr(col("org"), Eq, Literal(Utf8("acme")))
```

A tenant attribute containing `' OR '1'='1` produces the literal `'x'' OR ''1''=''1'` (one escaped string) — not an injection. The user's bytes are bounded inside a single literal node.

**Unquoted placeholders in expressions.** Because substitution happens at the expression tree level, placeholders in filter and mask expressions must NOT be quoted:

```sql
-- Correct
tenant = {user.tenant}

-- Wrong (parses as a literal string "...{user.tenant}..." and doesn't substitute)
tenant = '{user.tenant}'
```

## Missing attribute behavior

When a user lacks an attribute that a policy references, BetweenRows resolves the value from the attribute definition's `default_value`:

| User has attribute? | `default_value` | Template variable result |
|---|---|---|
| Yes | (irrelevant) | User's actual value (typed literal) |
| No | Non-NULL (e.g. `"acme"`) | Default as typed literal |
| No | NULL (default) | SQL `NULL` literal |

### SQL NULL semantics

Comparisons with SQL `NULL` evaluate to `NULL` (three-valued logic), which is treated as **false** in `WHERE` clauses. A user without a `tenant` attribute, where the definition has a NULL default, sees **zero rows** because `org = NULL` is never true.

This behavior is consistent across DataFusion and upstream PostgreSQL (where filter pushdown happens). `=`, `!=`, `>`, `<`, `IN`, and most comparison operators all short-circuit to false on NULL.

::: warning
`IS NULL` and `IS NOT NULL` are the exceptions — they explicitly match NULL. Avoid writing `WHERE foo IS NULL` with user attributes unless the behavior is intentional.
:::

### Undefined attributes

If a policy references `{user.foo}` but no attribute definition named `foo` exists at all (not just "the user doesn't have it set"), the query fails with a parse error. This catches typos and stale policies that reference deleted attributes.

## Save-time validation

Filter and mask expressions are validated at policy create/update time:

- The expression is dry-run parsed with placeholder bindings.
- If the expression contains unsupported SQL syntax (e.g., `EXTRACT`, correlated subqueries), the API returns 422 immediately.
- The policy is not saved until validation passes.

This prevents silent failures at query time.

## Supported SQL syntax

Filter and mask expressions both share the same structural grammar but differ in which functions they accept. The split is deliberate: row filter expressions live on the security-critical query path and are intentionally kept narrow, while column mask expressions have access to the full DataFusion function registry because they only transform values that have already cleared filtering.

### Structural syntax (both filter and mask)

Both expression kinds accept:

- **Column references** from the target table
- **Comparison operators:** `=`, `!=`, `<>`, `>`, `<`, `>=`, `<=`
- **Logical operators:** `AND`, `OR`, `NOT`
- **`IN` / `NOT IN`** with literal lists (and list attributes — see above)
- **`BETWEEN`**, `IS NULL`, `IS NOT NULL`
- **`LIKE`** with literal patterns
- **`CASE WHEN ... THEN ... ELSE ... END`**
- **Arithmetic operators** (`+`, `-`, `*`, `/`) and string concatenation (`||`)
- **`CAST(expr AS type)`** for numeric and string types
- **Parentheses** for grouping

### Functions in filter expressions

Row filter expressions (`filter_expression` on `row_filter` policies) whitelist exactly one function:

- `COALESCE(a, b, …)`

Any other function call returns a 422 at save time with a message like *"Function 'LEFT' in filter expressions is not supported. For complex expressions, use column masks instead."* If you need string manipulation or numeric transformation to build a filter predicate, derive the value upstream and reference it as a user attribute via `{user.KEY}` instead.

### Functions in mask expressions

Column mask expressions (`mask_expression` on `column_mask` policies) resolve function calls through the full DataFusion UDF registry. This includes everything DataFusion ships with — notably:

- **String:** `LEFT`, `RIGHT`, `SUBSTR`, `SUBSTRING`, `SPLIT_PART`, `CONCAT`, `CONCAT_WS`, `UPPER`, `LOWER`, `LENGTH`, `CHAR_LENGTH`, `LTRIM`, `RTRIM`, `BTRIM`, `REPLACE`, `REGEXP_REPLACE`, `REVERSE`, `REPEAT`, `LPAD`, `RPAD`
- **Numeric:** `ROUND`, `FLOOR`, `CEIL`, `ABS`, `MOD`, `POWER`, `SQRT`, `LOG`
- **Conditional:** `COALESCE`, `NULLIF`, `CASE WHEN` (see above)
- **Type conversion:** `CAST` (see above), `TO_CHAR`, `TO_NUMBER`

The full list is whatever is registered on the session's `FunctionRegistry` — if it exists in DataFusion, it works in a mask expression.

::: warning Use function-call form, not ANSI keyword form
Some SQL functions have two parse forms. In mask expressions you must use the function-call form:

| Use this | Not this |
|---|---|
| `SUBSTRING(ssn, 1, 5)` | `SUBSTRING(ssn FROM 1 FOR 5)` |
| `TRIM(name)`, `LTRIM(name)`, `RTRIM(name)` | `TRIM(BOTH ' ' FROM name)` |

The ANSI-keyword forms parse as dedicated AST nodes that the expression converter doesn't currently handle, and save with a 422 error. This will be relaxed in a future release.
:::

### Not supported in either context

- `EXTRACT(field FROM expr)` — use `DATE_PART('field', expr)` instead
- Correlated subqueries and `EXISTS` / `IN (SELECT ...)`
- Window functions
- User-defined SQL functions written in SQL (only registered UDFs work)
- `ILIKE` (use `LIKE` on lowercased columns instead)
- `IS TRUE` / `IS FALSE` / `IS NOT TRUE` / `IS NOT FALSE` — compare against literals directly
- JSON operators `->`, `->>` inside expressions (pushed down to upstream for simple column access, but not available in filter/mask templates)
- Interval literals and `AT TIME ZONE`

If you hit an unsupported construct, the validation error at save time tells you which node failed. For common workarounds, see the [Troubleshooting](/operations/troubleshooting) page.

## Examples

### String attribute in filter

```sql
org = {user.tenant}
```

### Integer attribute in filter

```sql
sensitivity_level <= {user.clearance}
```

### Boolean attribute in CASE

```sql
CASE WHEN {user.is_vip} THEN true ELSE tenant_id = {user.tenant} END
```

### List attribute in IN

```sql
department IN ({user.departments})
```

### Mask expression with CASE

```sql
CASE WHEN {user.department} = 'hr' THEN ssn ELSE '***-**-' || RIGHT(ssn, 4) END
```

### Mask referencing both row column and user attribute

```sql
CASE WHEN region = {user.region} THEN phone ELSE '[REDACTED]' END
```

## See also

- **[Policy Model](/concepts/policy-model)** — how filter/mask expressions fit into the overall policy model
- **[User Attributes](/guides/attributes)** — how attributes are defined and set
- **[Policy Types](/reference/policy-types)** — structural constraints per type
