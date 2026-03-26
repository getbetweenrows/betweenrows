# Roadmap

## MVP Checklist

- [x] **Roles (RBAC)** — DAG-based role hierarchy for policy assignment and datasource access. `role`, `role_member`, `role_inheritance` tables. Policy assignments can target a role (`assignment_scope='role'`), and users in the role (including via inheritance) receive those policies. Includes cycle detection, depth cap (10), soft delete, admin audit log, effective policy preview, and immediate cache invalidation for active connections.
- [ ] **User Attributes (ABAC)** — Key-value attributes on users (`user_attributes` table: `user_id, key, value`). Available as `{user.*}` template variables in filter/mask expressions and conditions. Extends the current hardcoded `{user.tenant}`, `{user.username}`, `{user.id}`. No IDP sync for MVP.
- [ ] **Conditional Policies** — Optional `condition` field on all policy types. Policy only applies when condition evaluates to true. Uses same expression syntax and `{user.*}` substitution as filter/mask expressions. Enables attribute-based policy activation (e.g., `user.role != 'admin'`).
- [ ] **Shadow Mode** — Per-policy dry-run state. Instead of blocking/masking, log what would have happened. Removes "fear of breaking prod" adoption blocker. Each policy gets an `action_status` field: `enforce` (default) or `shadow`.
- [ ] **YAML Import/Export** — Export all policies for a datasource as YAML. Import YAML to create/update policies. Enables version-controlled policy-as-code workflows and easy promotion between environments (dev → staging → prod).

## Policy System

### Remaining Integration Test Cases

All TC-* scenarios are now covered by integration tests in `proxy/tests/policy_enforcement.rs`. The list below is empty — new scenarios should be added here before implementation.

### Configurable Policies

- Configure policies, RLS, per user access to schema, table, columns, data masking
- Assign by user, group, or role (decision needed)
- Represent rules/policies/tags in single YAML file for "configuration as code" (CLI experience for developers)
- "As code" approach enables version control
- Keep audit history for all policy updates (track when who changed what)

> **See also:** AU-03 (YAML policy-as-code), CC-07 (version control & audit trail for policy changes)

### Policy-Level Shadow Mode

Shadow Mode is a "dry-run" state for individual SQL security policies. Instead of blocking a query that violates a rule, the proxy allows it but logs exactly what would have happened.

- **Action Status**: Each policy can be set to `Enforce` (Block/Mask) or `Shadow` (Log only).
- **Risk Mitigation**: Eliminates the fear of "breaking prod" by testing new constraints against live traffic without blocking.
- **Policy Refinement**: Helps identify false positives before enforcement.
- **Unified Logging**: Shadow matches look identical to blocks in logs, allowing users to visualize their security posture before committing.

> **See also:** DM-04 (canary rollout for testing policies on subset of users)

### Policy Creation Assistant

- Policy creation can be hard without deep understanding
- Consider: policy templates, policy examples, or AI-assisted policy creation
- Allow create (paste) and/or import policy from json/yaml, as a way to help users to quickly copy polices from testing env to prod env.
- auto scan upstream tables, fetch a few rows, then use LLM to suggest a bunch of policies, users can pick to add/enable them, and/or future tweak them manually or by iterating with LLM.

### Logic Decoupling & Tag-Based Access Control (TBAC)

- **Policy Templates**: Separate transformation logic (e.g., `REGEXP_REPLACE`) from policy definitions. Allows updating logic in one place for many policies.
- **Metadata Tagging Layer**: Allow admins and auto-scanners to apply tags (e.g., `pii`, `financial`, `deprecated`) to DataSources, Schemas, Tables, and Columns.
- **Inherited Tagging**: Tags applied to a Database or Table automatically flow down to child Columns unless overridden.
- **Tag-Based Policies**: Update `policy_match.rs` to allow targeting policies via tag patterns (e.g., `"target": "tag:pii"`) instead of just names.
- **Context-Aware Masking**: Support multi-column context in masking expressions (e.g., mask `salary` based on `region` column value).
- **Auto-Classification**: Add pattern-matching scanners (Regex, Luhn, NLP) to the Discovery Job to automatically tag sensitive data.

### Governance, Security Lineage & JIT

- **Sticky Security (Security Lineage)**: Ensure security rules "stick" to data as it moves through CTEs, subqueries, and views.
- **Data Domains**: Group DataSources into "Domains" (e.g., Finance, HR) for delegated administration.
- **Delegated Security**: Implement permissions on the policies themselves—allow a user to manage policies for a domain without having query access to that domain.
- **Invisible Security (Stealth Mode)**: Option to hide the "shape" of security logic from `EXPLAIN` plans and audit logs to prevent leaking internal security posture (Secure Virtual Views).
- **Just-in-Time (JIT) Access**: Support temporary, windowed policy assignments (e.g., 2-hour elevation) triggered by approval workflows or ticket validation.

### Programmable Governance (The "Leapfrog" Layer)

- **Programmable Policies (WASM Decision Functions)**: **Implemented.** Policy decision functions are JS functions compiled to WASM via Javy and evaluated at query time via wasmtime. Each policy can reference an optional `decision_function_id`. The function receives session and/or query context and returns `{ fire: boolean }`. Fuel-limited to 1M WASM instructions. `on_error` controls fail-secure vs fail-open behavior. Decision results are captured in the query audit log. See `docs/permission-system.md` for full details.
  - **WASM Linear Memory Limit**: Add a configurable per-function memory cap (e.g., 10 MB default) to prevent a single decision function from allocating unbounded memory. wasmtime supports `Store::limiter()` for this — wire it into `evaluate_wasm_sync` alongside the existing fuel limit.
  - **Module Cache in PolicyHook**: Pre-compile WASM modules once per `(decision_function_id, version)` and cache them in PolicyHook, instead of recompiling from bytes on every query evaluation. Evict on decision function update/delete. Reduces per-query overhead from ~ms compilation to ~us lookup.
  - **Decision Function Integration Tests**: Add integration tests in `policy_enforcement.rs` that exercise decision functions through the full proxy stack (real WASM evaluation via pgwire). Current coverage is unit tests only (`hooks/policy.rs`). Requires javy CLI in CI.
- **Validated Purpose (PBAC)**: Move beyond roles to "Purposes." Require a validated claim (e.g., a ticket ID from a ticketing system) to unlock specific data lenses.
- **Clean Room Joins**: Support "Blind Joins" where two tables can be joined on a sensitive key, but the proxy guarantees the key cannot be leaked in results or filters.
- **User Attribute Sync**: Dynamically pull ABAC attributes (Region, Department, Clearance) from identity claims at connect time.
- **Impact Analysis Engine**: Run a "What-If" simulation of a policy change against historical query logs to identify breaking changes before enforcement.
- **Policy Impersonation (Sudo Mode)**: Admin tool to "Run as User X" to verify policy enforcement and visibility in real-time.

---

## Deep Dive: Technical Concepts & Industry Parity

This section details the core mechanisms we are implementing to achieve parity with enterprise standards and beyond.

### 1. Tag-Based Access Control (TBAC) & Inherited Tagging
**Design Pattern: Metadata-Driven Security**

**Concept:** Instead of mapping security rules to object names, rules target **Tags**. Objects (DataSources, Tables, Columns) are labeled with metadata.

**How it works:**
1.  **Tag Assignment:** An admin or auto-scanner applies a tag like `pii:true` or `classification:sensitive` to an entity.
2.  **Inheritance Logic:** Tags follow a tree structure. A tag on a `DataSource` flows to all its `Schemas`. A tag on a `Table` flows to all its `Columns`. 
    *   *Example:* If `table:customers` is tagged `sensitivity:high`, every column within it (email, name, id) inherits that tag unless explicitly overridden.
3.  **Policy Evaluation:** The `PolicyHook` resolves the tags for every `TableScan` and `Column` in the logical plan. If a policy targets `tag:pii`, it applies to every column that carries or inherits that tag.
4.  **Benefits:** Zero-touch security. Adding a new table to the database requires zero policy updates if it follows a tagged naming convention or inherits from a tagged schema.

### 2. Logic Decoupling (Policy Templates)
**Design Pattern: Logic-Assignment Separation**

**Concept:** Separation of **Transformation Logic** (The "How") from **Security Mapping** (The "Where").

**How it works:**
1.  **Logic Template:** Define a reusable SQL expression snippet, e.g., `Template(name="last_4_mask", logic="'***-**-' || RIGHT(val, 4)")`.
2.  **Assignment:** A `Policy` links a `Column` (or `Tag`) to a `Template`.
3.  **Late Binding:** At query time, the proxy fetches the template logic and binds it to the specific column being queried.
4.  **Benefits:** Maintainability. Changing the legal requirement for SSN masking (e.g., from last-4 to last-3) is done in one single template, instantly updating every column assigned to it.

### 3. Context-Aware Masking
**Design Pattern: Multi-Column Visibility**

**Concept:** Masking logic that depends on other data in the same row or user attributes.

**How it works:**
1.  **Expression Context:** The `mask_expression` is no longer limited to the column itself (`val`). It can reference any column from the source table.
2.  **Logic Example:** `CASE WHEN user.role = 'manager' OR department = user.department THEN salary ELSE '***' END`.
3.  **Implementation:** The proxy injects a `CASE` statement into the projection that includes the necessary "context columns" from the underlying scan.

### 4. Sticky Security (Security Lineage)
**Design Pattern: Plan-Aware Protection**

**Concept:** Security constraints are not bypassed by "wrapping" a table in a View, CTE, or Subquery.

**How it works:**
1.  **Recursive Rewriting:** The `PolicyHook` doesn't just look at the top-level table. It recursively walks the DataFusion `LogicalPlan`.
2.  **Metadata Propagation:** If a leaf node (TableScan) has a mask applied, that mask "infects" the column as it passes through CTEs and Views.
3.  **Implementation:** Uses `transform_up` to ensure that even if a user queries `SELECT email FROM (SELECT * FROM users)`, the `email` column is masked at the source `users` scan before reaching the outer query.

### 5. Attribute-Based Access Control (ABAC) & Sync
**Design Pattern: Identity-Driven Security**

**Concept:** Using a flexible map of user properties (Attributes) rather than just a flat Role, synced from identity providers.

**How it works:**
1.  **Identity Registry:** Users carry a JSON bundle of attributes (e.g., `{"clearance": 3, "region": "EMEA"}`).
2.  **IDP Sync:** At handshake, the proxy extracts claims from the identity token (e.g., `groups`, `department`) and populates the session attributes.
3.  **Dynamic Substitution:** Every `{user.attribute}` placeholder in a policy is replaced at query time with the corresponding value from the user's session.

### 6. Validated Purpose (PBAC) & JIT Elevation
**Design Pattern: Just-in-Time Governance**

**Concept:** Enforcing Purpose Limitation and temporary access elevation.

**How it works:**
1.  **Purpose Handshake:** The client connection includes metadata: `purpose=audit` and `justification=INC-123`.
2.  **External Validation:** The proxy calls a webhook (e.g., to a ticketing system) to verify that `INC-123` is a valid, open ticket assigned to this user.
3.  **Temporary Lenses:** If validated, the "Audit Lens" is activated for a limited window (e.g., 2 hours). After expiry, the connection automatically reverts to a masked/restricted state without requiring a reconnect.

### 7. Programmable Policies (Scripted Logic)
**Design Pattern: Logic-as-Code**

**Concept:** Moving beyond static SQL for governance logic.

**How it works:**
1.  **Scripting Sandbox:** Admins can upload small scripts (e.g., Rhai) as "Policy Templates."
2.  **Runtime Execution:** The proxy registers these scripts as UDFs (User Defined Functions) in the plan.
3.  **Use Cases:** Calling an external vault for decryption, applying custom NLP models for scrubbing, or performing complex calculations that SQL cannot handle efficiently.

---

### Performance of PolicyHook

- Complex queries or large number of active policies could introduce overhead in transform_up operations on logical plan
- Need performance benchmarks with high volume of policies and complex queries

> **See also:** AU-02 (monitoring query rewrite latency)

### ALTER TABLE ADD COLUMN Idempotency

- Migration rules mention no idempotency guard in SeaORM for ADD COLUMN
- Users must not interrupt this migration
- Explore mitigation at framework level or provide better tooling/guidance

### Allow Testing/Preview Policy Before Deployment

- Allow sudo as a user to test out the policy if the policy isn't for the admin

> **See also:** DM-05 (verbose mode to explain why row filtered/masked), DM-04 (canary rollout for testing policies on subset of users)

### Conditional Column Masking

- **Use case**: Mask sensitive columns only when certain user attributes match a condition. For example:
  - Mask `salary` when `user.team != 'finance'`
  - Mask `ssn` when `user.role != 'admin'`
  - Mask `customer_email` when `user.region != user.customer_region`
- **Proposed syntax**:
  ```json
  {
    "schema": "hr",
    "table": "employees",
    "column": "salary",
    "mask_expression": "'***'",
    "condition": "user.team != 'finance'"
  }
  ```
- **Behavior**: If the condition evaluates to true, apply the mask. If false, return the original column value.
- **Implementation**: Extend `ColumnMaskDef` with an optional `condition` field. At query time, evaluate the condition (similar to how `{user.*}` variables are substituted) and only apply the mask expression if true.
- **Alternative**: Could also support "mask else original" semantics where a different mask is applied when condition is false, but the simple conditional masking covers most use cases.

### Conditional Policies (All Types)

Should every policy type support conditional application? This would allow policies that activate only under certain conditions:

| Policy Type | Conditional Use Case | Complexity |
|-------------|---------------------|------------|
| `row_filter` | Filter rows only for non-admin users | Low - filter_expression IS the condition |
| `column_mask` | Mask sensitive data for non-permissioned users | Medium - proposed above |
| `column_deny` | Hide columns from non-admin users | Medium |
| `table_deny` | Hide schemas/tables from certain teams | Medium |

**Option A: Per-policy condition field (recommended)**

Each policy type gets an optional `condition` field:

```json
// Row filter - condition is redundant, filter_expression IS the condition
{
  "policy_type": "row_filter",
  "condition": "user.role != 'admin'",  // redundant, but consistent
  "targets": [{ "schemas": ["orders"], "tables": ["*"] }],
  "definition": { "filter_expression": "tenant_id = {user.tenant}" }
}

// Column mask - condition adds fine-grained control
{
  "policy_type": "column_mask",
  "condition": "user.role != 'admin'",
  "targets": [{ "schemas": ["hr"], "tables": ["employees"], "columns": ["ssn"] }],
  "definition": { "mask_expression": "'***-**-****'" }
}

// Column deny - hide columns conditionally
{
  "policy_type": "column_deny",
  "condition": "user.clearance_level < 5",
  "targets": [{ "schemas": ["secret"], "tables": ["files"], "columns": ["content"] }]
}

// Table deny - hide tables conditionally
{
  "policy_type": "table_deny",
  "condition": "user.team != 'executive'",
  "targets": [{ "schemas": ["analytics"], "tables": ["*"] }]
}
```

**Option B: Condition IN the policy definition**

Embed the condition inside the definition object rather than as a top-level field. More compact but less consistent across types.

**Option C: Split into two policies**

Current workaround: Create separate policies for each condition. For example:
- Policy 1: `row_filter` for regular users
- Policy 2: No policies for admins (implicit allow)

This works but creates policy explosion when combining multiple conditions.

**Recommendation**: Go with **Option A** - add optional `condition` field to all policy types. This provides:
- Consistency across all policy types
- Clear semantics: policy only applies when condition is true
- Future-proof: easy to extend with more complex condition expressions
- Backward compatible: condition is optional, existing policies work unchanged

**Condition expression syntax**:
- Reuse same expression parser as `filter_expression` / `mask_expression`
- Available variables: `{user.*}` substitutions (tenant, username, id, role, team, etc.)
- Operators: `=`, `!=`, `<`, `>`, `<=`, `>=`, `AND`, `OR`, `NOT`, `IN`
- Examples: `user.role = 'admin'`, `user.team NOT IN ('sales', 'marketing')`, `user.clearance_level >= 3`

**Priority when multiple conditions match**:
- If multiple policies have conditions that all evaluate to true, apply all policies (AND semantics, same as now)
- If a condition evaluates to false, that policy is skipped
- Order: evaluate all conditions first, then apply matching policies

**Implementation plan**:
1. Add `condition` field to policy struct (optional, nullable)
2. Add condition evaluation helper (reuses existing `{user.*}` substitution logic)
3. Update `PolicyEffects::collect()` to check condition before including each policy
4. Update tests to cover conditional policies

> **See also:** Related to DM-03 (mask vs. hide decision), DM-04 (canary rollout for testing policies on subset of users)

## AI Integration & Model Context Protocol (MCP)

### Model Context Protocol (MCP) Server

Expose BetweenRows as an MCP server so that AI agents (Claude Desktop, VS Code Copilot, Cursor, etc.) can interact with it natively — both for querying data and for managing the system — without needing a PostgreSQL driver or direct UI access.

**Goal**: An AI agent should be able to handle the full BetweenRows workflow end-to-end — onboard a datasource, discover its schema, create and assign policies, query data, and review audit logs — entirely through tool calls, without ever touching the UI.

**Marketing line**: "BetweenRows: The Firewall for AI-to-SQL Interactions. Policies you can trust, for queries you didn't write."

#### Two categories of MCP tools

**1. Data Access** — AI agents querying databases through BetweenRows:
- `execute_query`: Accepts a datasource name and raw SQL; runs it through the policy engine (row filters, column masks, deny rules); returns JSON results. All existing enforcement and audit logging applies automatically.
- `describe_schema`: Returns the tables and columns the calling user is permitted to see (policy-filtered catalog), giving the LLM the right context to write correct SQL without exposing hidden schema.
- Shadow Mode integration: flag AI-generated queries in audit logs for visibility into what LLMs are accessing.

**2. Admin/Management** — AI agents managing BetweenRows itself:
- Policy management: create, update, delete, and assign policies via tool calls. An agent could interpret a natural-language request ("mask SSN for all non-finance users") and produce the correct policy.
- User management: create users, update credentials, assign datasource access.
- Datasource management: register new datasources, trigger catalog discovery.
- Audit log querying: fetch and summarize recent query activity.

#### Architecture

The MCP server is a thin wrapper over the existing admin API:
1. Add a `POST /query` endpoint to the admin API that accepts `{ datasource, sql }` and executes through the policy engine — same enforcement path as the PostgreSQL wire protocol.
2. Build an MCP server (separate sidecar or standalone process) that maps MCP tool calls to admin API HTTP requests. Policy logic stays in the Rust proxy; the MCP layer is stateless and thin.
3. Authenticate MCP clients via API key mapped to a BetweenRows user identity, so user-specific policies apply correctly.

#### Open questions
- MCP server implementation: separate Node/Python sidecar (e.g., `fastmcp`) vs. embedded in the Rust binary?
- Streaming: large query results may need pagination rather than a single JSON response.
- Scope of admin tools in v1: start with policy CRUD only, expand to users/datasources later?

## UI/UX Improvements

### Proxy Connection Info & User Self-Service

- **Problem:** There is no clear instruction or UI for users on how to connect to the proxy. After an admin creates a user and assigns datasource access, the user has no easy way to find their connection details (host, port, database/datasource name, username).
- **Admin UI — "Connect" panel:** Add a connection info section (per datasource or global) that shows proxy connection details: host, port, datasource name, and a copyable connection string (e.g., `psql -h <host> -p <port> -U <username> -d <datasource>`). Could also show API key in the future when that auth method is supported.
- **The regular user problem:** Currently only admins can access the admin UI. Regular proxy users have no self-service way to:
  - View which datasources they have access to
  - See their connection details
  - Reset their password
  - View their assigned policies / effective permissions
- **Possible approaches:**
  1. **User-facing portal (separate or scoped view):** A lightweight read-only UI that regular users can log into to see their own connection info and access summary. Could be a subset of the admin UI behind a different auth scope.
  2. **Welcome email / onboarding flow:** When admin creates a user, optionally generate a shareable connection guide (copy-paste or email).
  3. **CLI / SQL-based self-service:** Users could query their own metadata via the proxy itself (e.g., `SELECT * FROM betweenrows.my_access`), though this adds complexity.
  4. **API key management:** If/when API key auth is added, users need a way to generate and rotate their own keys — this further motivates a user-facing portal.
- **Open questions:**
  - Should the user portal be part of the admin UI (role-scoped) or a completely separate app?
  - What's the minimum viable self-service surface? Just connection info, or also password reset and access visibility?
  - How does this interact with future IDP/SSO integration — does the portal become unnecessary if auth is fully delegated?

### User Name, Datasource Name, Policy Name Validation

- Currently only have hints, need live validation in the UI

### Improve Hints in the UI

- Create user, datasource, policies, assignment
- Lack guidance and hints explaining what each config does
- Include risk associated with configs
- Provide recommended config/value (e.g., priority value)

### Query Audit Log UI

- Search filters: support username, datasource name, policy id/name
- Search by keyword in query text
- Consider full-text search (may be overkill)
- Log table: toggle on/off columns (icon at top right)
- Remember user preferences for column visibility
- Handle pagination (large volume of logs)

### Client-Side "Cold Start" UX

- Distinguish Fly.io auto-stop "sleep" state from total service outage
- Core requirements:
  - Delayed Loading Notice: Show "Waking up servers..." only if request pending > 1.5 seconds
  - Status Differentiation:
    - Cold Start: Connection stays open/pending → show "Waking up"
    - Service Down: Connection fails immediately with 502/503 or Refused → show "Service Unavailable"
  - Global Implementation: Handle at network layer (interceptor) for all API-driven actions
  - Automatic Resolution: Clear notices once request succeeds or reaches timeout (e.g., 10s)
- User benefit: Reduces "broken app" perception during 3-5 second boot sequence

### User Experience for Policy Creation (Admin UI)

- row_filter and column_mask policies use raw SQL/DataFusion expressions
- This can be complex for less technical administrators
- Consider: guided/templated approach for common scenarios
- Consider: DSL that simplifies expression writing

### UI/UX Redesign

- Focus on look and user experience improvements
- Not related to fundamental product features

### Policy Assignment UI Consolidation

- **Problem:** Currently, policy assignment is managed in the DataSource UI (`DataSourceEditPage.tsx`), but policy creation happens in the Policy UI (`PolicyEditPage.tsx`). This creates a fragmented user experience where users have to jump between pages after creating a policy.

- **Proposed Solution:**
  1. **Consolidate Policy Assignment in Policy UI:** Move the editable policy assignment UI from `DataSourceEditPage.tsx` to `PolicyEditPage.tsx` (or a new dedicated "Policy Assignment" page). This allows users to create a policy and assign it to datasources in one place.
  2. **DataSource UI becomes Read-Only:** Replace the editable `PolicyAssignmentPanel` in `DataSourceEditPage.tsx` with a read-only view of assigned policies. This view should link to the respective Policy edit pages.
  3. **API Alignment:** Introduce policy-centric API endpoints:
     - `POST /policies/:policy_id/assignments` - Create an assignment for a policy
     - `DELETE /policies/:policy_id/assignments/:assignment_id` - Remove an assignment
     - `GET /policies/:policy_id/assignments` - List all datasources a policy is assigned to
     - `PUT /policies/:policy_id/assignments/:assignment_id` - Update assignment (e.g., priority)
  4. **Deprecation:** Deprecate `POST` and `DELETE` on `/datasources/:id/policies`. Keep `GET /datasources/:id/policies` for the read-only view in DataSource UI.

- **Database:** No schema changes needed. The existing `policy_assignments` table already stores all necessary data.

- **Benefits:**
  - Single source of truth for policy assignment management
  - Improved user workflow (create policy -> assign to datasources in one place)
  - Clear separation of concerns: Policy UI manages policies & assignments, DataSource UI manages datasource configuration

## Testing & Performance

### The Unified E2E Strategy

Two distinct "flows" in CI/CD pipeline:

1. **Web UI & API (Playwright Track)**
   - Goal: React frontend correctly communicates with Rust REST/GraphQL API
   - Tool: Playwright
   - Mechanism: Playwright spins up React dev server and Rust binary, mimics real user actions
   - Why: Handles web UI "flakiness" (waiting for elements) better than other tools

2. **TCP SQL Proxy (psql Track)**
   - Goal: Rust TCP server correctly proxies Postgres Wire Protocol without dropping packets or mangling queries
   - Tool: psql (CLI)
   - Mechanism: Shell script runs real SQL query against Proxy Port (5433) instead of database port (5432)
   - Validation: `psql -h localhost -p 5433 -U test_user -d test_db -c "SELECT 1;"`
   - If returns 1, Rust proxy successfully handles handshake, authentication, and data frames

**GitHub Actions Implementation:**

- Stage: Start Postgres Docker container in "services" section
- Build: Build Rust binary and install Node dependencies
- Setup: Run Rust migrations against Docker DB
- Test (UI): Run `npx playwright test` - Playwright starts UI and API
- Test (Proxy): Run psql command pointing to Rust Proxy port
- Artifacts: Upload Playwright Trace Viewer files if UI test fails

### Testing Strategy

Given complexity of new policy system (interaction with DataFusion and PostgreSQL), detailed security test plan needed (see docs/permission-security-tests.md):

- Edge cases for policy conflicts and priority resolution
- Negative tests to ensure policy bypasses are not possible
- Performance tests for policy application under load
- Tests for YAML import/export (malformed YAML, security implications like injecting malicious SQL via policy definitions)

> **See also:** Extensive story coverage for edge cases (CO-01 to CO-07), negative tests across all categories, YAML import/export covered by security stories

### Performance Testing

- Large tables
- Expensive queries

## Security

### SSL Configuration

- SSL mode default should be "prefer"
- Research comprehensive Postgres SSL options
- Verify naming of options follows standards

### PostgreSQL System Tables Exposure

- We expose pg\_ system tables to users (from DataFusion or pgwire)
- Understand: how does this work? What tables are supported in Postgres setup?
- Security concerns with exposing them? Can we restrict them?

### Database Name Visibility

- In pg_database, original source database name is shown instead of virtual database name
- Original source database name also exposed in error logs in SQL client (e.g., postgres.<schema>.<table> not found)
- Any way to show virtual database name instead? Existing solutions or best practices?

### Security Penetration Testing

- SQL injection
- SSL

## Data Sources

### Multi Data Source Type Support

- Expand beyond PostgreSQL to support additional data source types:
  - SQLite
  - MySQL
  - Amazon Athena
  - Amazon Redshift
  - Snowflake
  - DuckDB
  - S3 files (Parquet, CSV, JSON)
- Abstract data source connection layer to support multiple backends
- Preserve existing policy enforcement (row filter, column mask, deny) across all source types
- Catalog discovery and schema introspection per source type

## Infrastructure & Deployment

### Password & Authentication

- Forget password and reset password
- 2FA or OTP support

## Bugs

- 2026-03-04: DataFusion query error - Invalid function 'pg_get_function_identity_arguments'. Did you mean 'pg_get_statisticsobjdef_columns'?
- 2026-03-04: DataFusion query error - table 'postgres.pg_catalog.pg_statio_user_tables' not found
- 2026-03-04: DataFusion query error - table 'postgres.information_schema.table_constraints' not found
- 2026-03-04: DataFusion query error - Invalid function 'quote_ident'. Did you mean 'date_bin'?
- 2026-03-09: JOIN with duplicate column names (e.g., `id`) and `SELECT *` causes "Ambiguous reference to unqualified field id" error. — **Partially mitigated 2026-03-11**: column deny/allow now uses `DFSchema` qualifier-aware iteration so deny policies no longer collide across tables. Root ambiguity for `SELECT *` on JOINs with duplicate column names is a DataFusion limitation, not directly related to policy enforcement.
- Sometimes SQL queries take long time and cause UI to hang - need performance testing, may be missing indexes
- 2026-03-18: Catalog cache not invalidated after resync discovers new columns/tables/schemas — newly discovered objects are not immediately visible in queries until the cache expires or is manually cleared

### Git Commit Hook Improvements

- Current: pre-commit hook runs `cargo fmt`, `cargo clippy`, `npm run typecheck`, `npm run test:run` on ALL changes (staged + unstaged)
- Problem: Uncommitted unstaged changes cause false failures - hook fails because of code in files you haven't committed yet
- Proposed: Modify hook to only check staged changes using `git diff --staged` or `git diff --cached`
- This preserves ability to have WIP changes without them interfering with the commit process

## Frontend Architecture Guideline: Future-Proofing UI

### 1. Decouple Logic from Presentation

Pattern: Use Custom Hooks (e.g., useAuth, useCart) to handle all API calls, state management, and business logic.

Rule: Components should only receive data and functions via props. If we want to swap a "List View" for a "Card View" later, the logic hook remains untouched.

### 2. Implement a "Headless" Design System

Tooling: Use CVA (Class Variance Authority) to manage Tailwind variants.

Rule: Avoid hardcoding "messy" Tailwind strings directly in feature components. Define styles as Variants (e.g., intent: "primary", size: "lg") in a central UI folder.

Goal: Changing the "Look" of the app should involve editing one tailwind.config.js or a few CVA files, not hunting through 50 feature components.

### 3. Strict Design Token Usage

Rule: Zero Hex Codes in components. All colors, spacing, and border-radii must come from the tailwind.config.js theme.

Standard: Use semantic naming. Instead of text-blue-600, use text-brand-primary. When v2 arrives, we change the value of brand-primary in one place.

### 4. Component Categorization (Atomic Design)

UI Components (/components/ui): Low-level, stateless elements (Buttons, Inputs, Modals). Use libraries like Radix UI or Headless UI for accessibility logic so we only have to worry about the CSS.

Feature Components (/features/): High-level components that use the UI pieces to solve a business need (e.g., PaymentForm).

### 5. Utility for Class Merging

Requirement: Use a cn() helper function (combining clsx and tailwind-merge).

Benefit: This ensures that if we need to override a "v1" style for a specific edge case, the classes merge correctly without CSS conflicts.

## Policy History & Audit Improvements

### Policy Historical Versions UI

- View past versions of policies in the policy UI
- Ability to link to policy from audit log page when user clicks on a policy listed in the query log


## Code Review & Refactoring

### Permission Policy Logic & Catalog Context Cache Review

- Review how permission policy logic is currently handled
- Review catalog context cache implementation
- Ensure logic is robust and not scattered everywhere
- Consider big refactoring if needed to reduce tech debt and improve maintainability

Specific areas identified from 2026-03-08 bug fixes — worth revisiting in a dedicated refactoring pass:

#### `rebuild_contexts_for_datasource` has a brief staleness window

When a policy changes, `rebuild_contexts_for_datasource` spawns background tasks that rebuild each active connection's `SessionContext`. Between the policy write and the rebuild completing (typically milliseconds), an in-flight query on an affected connection still sees the old schema. For most use cases this is acceptable, but it is a known gap worth documenting explicitly if stricter guarantees are ever needed (e.g., serialize the rebuild before returning the API response).

#### Session cache TTL vs. immediate invalidation

`PolicyHook` caches loaded policies per `(datasource_id, username)` for 60 seconds. `invalidate_datasource` clears the cache eagerly on policy mutations, so in practice the staleness window is zero for well-behaved callers. However, nothing prevents a future code path from skipping invalidation. A comment or assertion at the cache boundary would make this invariant explicit.

#### `ConnectionEntry` may grow

`ConnectionEntry` in `handler.rs` now holds `ctx`, `user_id`, and `datasource_name`. If future features need additional per-connection state (e.g., active transaction, client application name, audit context), this struct is the right place — but worth a deliberate review rather than ad-hoc field additions.

## Configurable Audit Tracking

### Problem Context

- Currently, all audit events are always recorded (query audit trail, admin audit logs for all entity types)
- Some deployments may want to disable certain audit streams for performance, storage, or compliance reasons
- Need a configuration system that lets admins toggle audit tracking on/off per category

### Proposed Configuration

- **Query audit trail**: Global on/off toggle for logging every SQL query execution (the `query_audit_log` entries)
- **Admin audit log per entity type**: Individual on/off toggles for each entity type tracked in admin audit logs, e.g.:
  - User mutations
  - Datasource mutations
  - Policy mutations
  - Role mutations
  - Policy assignment mutations
- Configuration could live in a `system_config` table or as environment variables / startup config

### Open Questions

- Is this a good idea at all? Turning off audit logging could be a security risk — need to debate trade-offs
- Should there be a minimum audit level that cannot be disabled (e.g., always log policy changes)?
- Where should the config live — database table (runtime changeable) vs. env vars (deploy-time only)?
- Should disabling audit log also suppress the async `tokio::spawn` insert, or just mark entries as "not stored"?
- If audit is off, should the audit UI show a banner indicating that logging is disabled?

## Column Access Behavior Configuration

### Problem Context

- Currently: Return "column not found" error when user doesn't have access to a column (preferred, more secure, prevents metadata leak)
- Problem: When admin changes permission, downstream integrations (e.g., BI tools) may fail because they suddenly lose access to a column — they expect it to exist

### Option 1: Datasource-Level Config

- Add a datasource-level config to switch behavior globally per datasource
- Option 1: Return "column not found" error (default, more secure, prevents metadata leak)
- Option 2: Return empty column silently (compatibility mode)

### Option 2: Policy Config (Alternative)

- Instead of datasource-level, make this a policy config for finer-grained control
- When creating a `column_deny` policy, add an option to pick the behavior:
  - Option 1: Throw "column not found" error (default, more secure, hides existence of column)
  - Option 2: Return empty/null column silently (compatibility mode, prevents integration failures)
- Rationale: Allows per-policy control over security vs compatibility tradeoff
- Need deeper discussion to decide between Option 1 vs Option 2
