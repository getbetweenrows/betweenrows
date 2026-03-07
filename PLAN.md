# Policy System + JSONB Support — Design Plan

## Context

QueryProxy has two hardcoded hooks: `ReadOnlyHook` (allowlist) and `RLSHook` (tenant WHERE injection). The goal is a **configurable policy system** with RLS rules, column masking, schema/table/column access control — assignable per user, group, or role. JSONB support is a prerequisite. The existing tenant-based RLS will be absorbed into the new policy system.

### Key Decisions Made
- **Groups + Roles + direct user assignment** (most flexible)
- **Absorb RLS into policy system** (unified, template variables like `{user.tenant}`)
- **YAML policy-as-code included from start**
- **JSONB**: Layer 1 = map to Utf8 (basic). Layer 2 = virtual table custom queries (modeled columns) — deferred to separate design phase as it's essentially a semantic layer feature

---

## Part 1: JSONB Support (with pushdown)

### Problem
`datafusion-table-providers` v0.9.3 drops JSONB/JSON columns (`arrow_type = NULL`). Users can't query JSONB at all.

### Solution: Full round-trip with pushdown
Map JSONB to `Utf8` in Arrow, support JSON operators via UDFs, and push down to upstream PG native syntax via custom Dialect.

**Complete round-trip:**
```
User: SELECT payload->>'name' FROM events WHERE payload->0->>'id' = '42'
  → JsonExprPlanner (from datafusion-functions-json): converts ->> to json_as_text() UDF calls
  → DataFusion: plans with UDFs, pushes filter to TableProvider
  → BetweenRowsPostgresDialect unparses: ... "payload"->>'name' ... WHERE "payload"->0->>'id' = '42'
  → Upstream PG: native JSONB operators, uses indexes
```

### Step 1: Discovery — map json/jsonb to Utf8
- **File**: `proxy/src/discovery/postgres.rs` — `discover_columns()` ~line 221
- After `get_schema()` loop, for columns where `arrow_type` is `None` and `data_type` is `"json"` or `"jsonb"` → set `arrow_type = Some("Utf8")`
- JSONB columns become visible in catalog and queryable

### Step 2: Register JSON UDFs via `datafusion-functions-json`
- **File**: `proxy/Cargo.toml` — add `datafusion-functions-json` dependency
- **File**: `proxy/src/engine/mod.rs` — in `create_session_context_from_catalog()`, call `datafusion_functions_json::register_all(&mut ctx)?`
- This single call registers:
  - UDFs: `json_get`, `json_get_str`, `json_get_int`, `json_get_float`, `json_get_bool`, `json_get_json`, `json_get_array`, `json_as_text`, `json_length`, `json_contains`
  - **`JsonExprPlanner`**: handles `->`, `->>`, `?` operators natively at planning time — converts them to UDF calls automatically
  - **`JsonFunctionRewriter`**: optimizes nested access `json_get(json_get(col,'a'),'b')` → `json_get(col,'a','b')`, and casts `json_get(...)::int` → `json_get_int(...)`
- **No sql_rewrite.rs changes needed** — the ExprPlanner handles PG JSON operators directly
- All functions accept variadic `str | int` keys, so `payload->0->>'id'` and chained access work out of the box

### Step 3: Custom dialect — unparse UDFs back to PG JSON operators for pushdown
- **File**: `proxy/src/engine/mod.rs` (or new `proxy/src/engine/dialect.rs`)
- Create `BetweenRowsPostgresDialect` wrapping/extending `PostgreSqlDialect`
- Override `scalar_function_to_sql_overrides()`:
  - `json_as_text(col, 'key')` → `ast::Expr::BinaryOp { col, op: ->> , 'key' }`
  - `json_get(col, 'key')` → `ast::Expr::BinaryOp { col, op: -> , 'key' }`
  - `json_contains(col, 'key')` → `ast::Expr::BinaryOp { col, op: ? , 'key' }`
  - Handle variadic args for chained access: `json_get(col, 'a', 0, 'b')` → `col->'a'->0->'b'`
- Use `.with_dialect(Arc::new(BetweenRowsPostgresDialect {}))` on SqlTable (replacing current `PostgreSqlDialect`)
- **Key**: `default_filter_pushdown()` tests `Unparser::expr_to_sql()` — our dialect makes JSON expressions unparse successfully → `Exact` pushdown
- **Note**: The `JsonFunctionRewriter` flattens nested calls to variadic form, so the dialect only needs to handle the variadic case

### Step 4: pgwire type mapping (optional enhancement)
- **File**: `proxy/src/arrow_conversion.rs`
- Optionally map JSONB columns to `Type::JSONB` instead of `Type::VARCHAR` so PG clients know it's JSON
- Requires knowing the original `data_type` at encoding time (may need metadata on the Arrow field)

### Step 5: Admin UI
- **File**: `admin-ui/src/components/CatalogDiscoveryWizard.tsx`
- JSONB columns now appear with `arrow_type: "Utf8"` — selectable like any column
- Show original `data_type` ("jsonb") as a badge/label so users know the native type

### Key files for JSONB
- `proxy/Cargo.toml` — add `datafusion-functions-json` dependency
- `proxy/src/discovery/postgres.rs` — discovery fix (json/jsonb → Utf8)
- `proxy/src/engine/mod.rs` — `register_all()` call + `BetweenRowsPostgresDialect` for pushdown
- `admin-ui/src/components/CatalogDiscoveryWizard.tsx` — UI update

---

## Part 2: Data Model

### Groups & Roles

```
role                — named permission role (e.g., "analyst", "viewer")
user_role           — user ↔ role assignment (many-to-many)
policy_group        — named group of users (e.g., "finance-team")
policy_group_member — user ↔ group assignment (many-to-many)
```

**Groups** = organizational grouping (team membership). **Roles** = permission templates.
Policies can be assigned to: a specific user, a group, or a role.

#### Entity: `role`
| Field | Type | Notes |
|-------|------|-------|
| id | Uuid (v7) | PK |
| name | String | unique |
| description | Option\<String\> | |
| created_at | DateTime | |
| updated_at | DateTime | |

#### Entity: `user_role`
| Field | Type | Notes |
|-------|------|-------|
| id | Uuid (v7) | PK |
| user_id | Uuid | FK → proxy_user, CASCADE |
| role_id | Uuid | FK → role, CASCADE |
| created_at | DateTime | |

Unique index on `(user_id, role_id)`.

#### Entity: `policy_group`
| Field | Type | Notes |
|-------|------|-------|
| id | Uuid (v7) | PK |
| name | String | unique |
| description | Option\<String\> | |
| created_at | DateTime | |
| updated_at | DateTime | |

#### Entity: `policy_group_member`
| Field | Type | Notes |
|-------|------|-------|
| id | Uuid (v7) | PK |
| group_id | Uuid | FK → policy_group, CASCADE |
| user_id | Uuid | FK → proxy_user, CASCADE |
| created_at | DateTime | |

Unique index on `(group_id, user_id)`.

### Policies

#### Entity: `policy`
| Field | Type | Notes |
|-------|------|-------|
| id | Uuid (v7) | PK |
| name | String | unique, human-readable |
| description | Option\<String\> | |
| policy_type | String | `"row_filter"`, `"column_mask"`, `"column_access"` |
| definition | String | JSON — type-specific rule (see below) |
| is_enabled | bool | default true |
| created_at | DateTime | |
| updated_at | DateTime | |

#### Entity: `policy_assignment`
| Field | Type | Notes |
|-------|------|-------|
| id | Uuid (v7) | PK |
| policy_id | Uuid | FK → policy, CASCADE |
| data_source_id | Uuid | FK → data_source, CASCADE |
| user_id | Option\<Uuid\> | FK → proxy_user, CASCADE |
| group_id | Option\<Uuid\> | FK → policy_group, CASCADE |
| role_id | Option\<Uuid\> | FK → role, CASCADE |
| priority | i32 | lower = higher priority |
| created_at | DateTime | |

Constraint: exactly one of `user_id`, `group_id`, or `role_id` must be non-null.

#### Entity: `policy_audit_log`
| Field | Type | Notes |
|-------|------|-------|
| id | Uuid (v7) | PK |
| actor_id | Uuid | who made the change (no FK — survives user deletion) |
| actor_username | String | denormalized for readability |
| action | String | `"create"`, `"update"`, `"delete"`, `"assign"`, `"unassign"` |
| resource_type | String | `"policy"`, `"policy_group"`, `"role"`, `"policy_assignment"` |
| resource_id | Uuid | the entity that changed |
| old_value | Option\<String\> | JSON snapshot before |
| new_value | Option\<String\> | JSON snapshot after |
| created_at | DateTime | |

### Policy Definition JSON by Type

**`row_filter`** — injects WHERE clause:
```json
{
  "schema": "public",
  "table": "orders",
  "filter_expression": "region = '{user.tenant}'"
}
```
Template variables: `{user.tenant}`, `{user.username}`, `{user.id}`
Use `"table": "*"` for all tables in a schema.

**`column_mask`** — replaces column value with expression:
```json
{
  "schema": "public",
  "table": "customers",
  "column": "email",
  "mask_expression": "CONCAT(LEFT(email, 1), '***@', SPLIT_PART(email, '@', 2))"
}
```

**`column_access`** — hides columns entirely:
```json
{
  "schema": "public",
  "table": "customers",
  "columns": ["ssn", "credit_card_number"],
  "action": "deny"
}
```

### RLS Migration
The current hardcoded tenant filter becomes a **system policy** auto-generated per datasource. When a datasource is created, if RLS is enabled, a system `row_filter` policy is created:
```json
{
  "schema": "*",
  "table": "*",
  "filter_expression": "tenant = '{user.tenant}'"
}
```
This system policy is assigned to all users of the datasource. Admins can disable or customize it.

---

## Part 3: Policy Engine (Hook)

### `PolicyHook` replaces `RLSHook`

Hook chain: `ReadOnlyHook` → `PolicyHook`

**Flow on each query:**
1. Skip if statement is not `Query` (SELECT) — pass through
2. Skip if only system tables (same logic as current RLS)
3. Load applicable policies for this user + datasource (from session cache)
   - Direct user assignments + group memberships + role assignments
   - Merge by priority (lower number = higher priority)
4. Build logical plan from AST
5. Apply transformations via `transform_up`:
   - **row_filter**: Inject `Filter` node above matching `TableScan`
   - **column_mask**: Wrap matching `TableScan` in `Projection` replacing column with mask expression
   - **column_access deny**: Remove denied columns from `Projection`
6. Execute transformed plan, stream results

### Conflict Resolution
- **row_filter**: AND all matching filters (most restrictive)
- **column_mask**: highest-priority mask wins (lowest `priority` number)
- **column_access deny**: union of all denied columns
- Direct user > group > role at equal priority numbers

### Caching
- Policies loaded once per pgwire session, cached in handler
- `EngineCache` gets a policy cache keyed by `(user_id, datasource_name)`
- Policy CRUD invalidates relevant cache entries

### Key files
- `proxy/src/hooks/policy.rs` — new PolicyHook implementation
- `proxy/src/hooks/rls.rs` — removed (absorbed)
- `proxy/src/hooks/mod.rs` — update trait, register PolicyHook
- `proxy/src/handler.rs` — pass policy cache, update hook registration
- `proxy/src/engine/mod.rs` — policy cache storage + invalidation

---

## Part 4: Admin API

### Groups
| Method | Path | Description |
|--------|------|-------------|
| GET | `/groups` | List groups (paginated) |
| POST | `/groups` | Create group |
| GET | `/groups/{id}` | Get group with members |
| PUT | `/groups/{id}` | Update group |
| DELETE | `/groups/{id}` | Delete group |
| PUT | `/groups/{id}/members` | Set members (replace-all) |

### Roles
| Method | Path | Description |
|--------|------|-------------|
| GET | `/roles` | List roles (paginated) |
| POST | `/roles` | Create role |
| GET | `/roles/{id}` | Get role with users |
| PUT | `/roles/{id}` | Update role |
| DELETE | `/roles/{id}` | Delete role |
| PUT | `/roles/{id}/users` | Set role users (replace-all) |
| GET | `/users/{id}/roles` | Get user's roles |
| PUT | `/users/{id}/roles` | Set user's roles |

### Policies
| Method | Path | Description |
|--------|------|-------------|
| GET | `/policies` | List (paginated, filterable by type) |
| POST | `/policies` | Create policy |
| GET | `/policies/{id}` | Get with assignments |
| PUT | `/policies/{id}` | Update policy |
| DELETE | `/policies/{id}` | Delete policy |
| POST | `/policies/{id}/validate` | Validate definition against catalog |

### Assignments
| Method | Path | Description |
|--------|------|-------------|
| GET | `/datasources/{id}/policies` | List assigned policies |
| POST | `/datasources/{id}/policies` | Assign policy (user/group/role) |
| DELETE | `/datasources/{id}/policies/{aid}` | Remove assignment |

### Audit
| Method | Path | Description |
|--------|------|-------------|
| GET | `/audit-log` | Paginated, filterable by resource_type, action, actor, date range |

### YAML
| Method | Path | Description |
|--------|------|-------------|
| GET | `/policies/export` | Export all as YAML |
| POST | `/policies/import` | Import YAML (with dry-run option) |
| POST | `/policies/import?dry_run=true` | Preview changes without applying |

---

## Part 5: YAML Policy-as-Code

```yaml
version: 1

roles:
  - name: analyst
    description: Read-only data analyst
  - name: admin
    description: Full access admin

groups:
  - name: finance-team
    members: [alice, bob]
  - name: engineering
    members: [charlie, dave]

policies:
  - name: tenant-isolation
    type: row_filter
    definition:
      schema: "*"
      table: "*"
      filter_expression: "tenant = '{user.tenant}'"
    assignments:
      - datasource: production
        role: analyst

  - name: mask-email
    type: column_mask
    definition:
      schema: public
      table: customers
      column: email
      mask_expression: "CONCAT(LEFT(email, 1), '***@', SPLIT_PART(email, '@', 2))"
    assignments:
      - datasource: production
        group: finance-team
      - datasource: production
        user: charlie
        priority: 10

  - name: hide-pii
    type: column_access
    definition:
      schema: public
      table: customers
      columns: [ssn, credit_card_number]
      action: deny
    assignments:
      - datasource: production
        role: analyst
```

**CLI:**
```bash
cargo run -p proxy -- policy apply -f policies.yaml              # apply
cargo run -p proxy -- policy apply -f policies.yaml --dry-run    # preview
cargo run -p proxy -- policy export -o policies.yaml             # export
```

---

## Part 6: Admin UI

### New Pages
1. **Roles** (`/roles`) — CRUD, user assignment
2. **Groups** (`/groups`) — CRUD, member assignment
3. **Policies** (`/policies`) — list with type badges, create/edit with type-specific forms:
   - Row filter: schema/table selector + SQL expression input
   - Column mask: schema/table/column selector + mask expression (with presets: email, phone, etc.)
   - Column access: schema/table/column selector + deny list
4. **Policy assignments** — on datasource detail page, assign policies to users/groups/roles
5. **Audit log** (`/audit-log`) — filterable table

### Sidebar Nav
Users | Groups | Roles | Data Sources | Policies | Audit Log

---

## Implementation Phases

### Phase A: JSONB Support (with pushdown)
1. Discovery fix: json/jsonb → Utf8 in `discover_columns()`
2. Add `datafusion-functions-json` dep, call `register_all()` — handles `->`, `->>`, `?` operators + all JSON UDFs + array access + chaining
3. Custom `BetweenRowsPostgresDialect`: unparse JSON UDFs back to PG operators for upstream pushdown
4. Replace `PostgreSqlDialect` with `BetweenRowsPostgresDialect` on SqlTable
5. Admin UI: JSONB columns selectable, show native type badge
6. Tests: discovery round-trip, dialect unparsing, end-to-end query with pushdown
- **Files**: `proxy/Cargo.toml`, `proxy/src/discovery/postgres.rs`, `proxy/src/engine/mod.rs`, `admin-ui/src/components/CatalogDiscoveryWizard.tsx`

### Phase B: Data Model (migrations + entities)
- Migration 007: `role`, `user_role`
- Migration 008: `policy_group`, `policy_group_member`
- Migration 009: `policy`, `policy_assignment`, `policy_audit_log`
- SeaORM entities for all
- **Files**: `migration/src/`, `proxy/src/entity/`

### Phase C: Groups + Roles API
- CRUD handlers for roles, groups
- Member/user assignment handlers
- **Files**: `proxy/src/admin/role_handlers.rs`, `proxy/src/admin/group_handlers.rs`, `proxy/src/admin/dto.rs`, `proxy/src/admin/mod.rs`

### Phase D: Policy CRUD API + Audit
- Policy CRUD handlers
- Assignment handlers
- Audit log recording on all mutations
- Definition validation (parse expressions, check catalog references)
- **Files**: `proxy/src/admin/policy_handlers.rs`, `proxy/src/admin/audit.rs`

### Phase E: Policy Engine (Hook)
- Implement `PolicyHook` with plan transformations
- Absorb `RLSHook` — tenant filter as system row_filter policy
- Session-level policy caching + invalidation
- **Files**: `proxy/src/hooks/policy.rs`, `proxy/src/hooks/mod.rs`, `proxy/src/handler.rs`, `proxy/src/engine/mod.rs`

#### 3-Phase Hook System (prerequisite for plan-level RLS)
Current `QueryHook` trait has a single `handle_query` method and operates on the SQL AST.
For plan-level column masking / row filter injection, upgrade to a 3-phase model (inspired by `datafusion-postgres`):
1. **`handle_simple_query`** — intercept in simple query path (current behaviour)
2. **`handle_extended_parse`** — intercept at Parse phase of extended query protocol; receives the AST and can return a custom `LogicalPlan` directly. This is where RLS row filters and column masking should be applied — operating on the plan is more robust than SQL string rewriting and cannot be bypassed by query structure.
3. **`handle_extended_execute`** — intercept at Execute phase; receives the bound `LogicalPlan` and can short-circuit execution.

Existing `ReadOnlyHook` and `RLSHook` only use phase 1; `PolicyHook` will use phase 2 to inject `Filter` / `Projection` nodes via `transform_up`.

### Phase F: YAML Import/Export + CLI
- YAML schema + serde
- Export/import API endpoints with dry-run
- CLI `policy apply/export` commands
- **Files**: `proxy/src/admin/policy_yaml.rs`, `proxy/src/main.rs`

### Phase G: Admin UI
- Roles, Groups, Policies, Audit Log pages
- Policy assignment UI on datasource detail
- Sidebar nav updates
- **Files**: `admin-ui/src/pages/`, `admin-ui/src/components/`, `admin-ui/src/App.tsx`

### Future: Virtual Table Queries (Semantic Layer)
- Allow admins to define custom SQL queries as virtual tables (Looker-style)
- Extracts JSON fields as typed columns, enables joins, computed columns
- Column policies apply to the modeled output columns
- This is a separate design effort — essentially a semantic/modeling layer

---

## Verification
- **JSONB discovery**: Discover datasource with JSONB columns → verify `arrow_type: "Utf8"` → columns selectable in wizard
- **JSONB query**: `SELECT payload->>'name' FROM table` via psql → confirm field extraction works
- **JSONB pushdown**: Check `RUST_LOG=debug` logs → upstream SQL should contain `->>'key'` not `json_get_str`
- **JSONB filter**: `WHERE payload->>'region' = 'US'` → confirm filter pushes down (check generated SQL in logs)
- **Roles/Groups**: CRUD via API, verify user membership
- **Policies**: Create row_filter + column_mask + column_access → assign to user/group/role → connect via psql → verify enforcement
- **RLS migration**: Existing tenant filter still works after absorbing into PolicyHook
- **Audit**: Check audit log after all policy mutations
- **YAML**: Export → modify → dry-run import → apply → verify
- **Existing tests**: `cargo test -p proxy` + `cd admin-ui && npm test`
