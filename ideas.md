# Ideas

## Policy System

### Visibility follows Access

- Differentiate metadata access (virtual schema) and data access (cells, values)
- Philosophy: "Visibility follows Access" - if you can access a schema/table/column, you can see it in your SQL client sidebar
- To avoid evaluating policies twice (connection time for virtual schema, query time for filtering):
  - Store CompiledVisibility in admin store, updated on policy create/update
  - Need to determine best approach: database trigger vs code in API
- Need to define schema for storing pre-compiled visibility

### Configurable Policies

- Configure policies, RLS, per user access to schema, table, columns, data masking
- Assign by user, group, or role (decision needed)
- Represent rules/policies/tags in single YAML file for "configuration as code" (CLI experience for developers)
- "As code" approach enables version control
- Keep audit history for all policy updates (track when who changed what)

> **See also:** AU-03 (YAML policy-as-code), CC-07 (version control & audit trail for policy changes)

### Policy Creation Assistant

- Policy creation can be hard without deep understanding
- Consider: policy templates, policy examples, or AI-assisted policy creation
- Allow create (paste) and/or import policy from json/yaml, as a way to help users to quickly copy polices from testing env to prod env.
- auto scan upstream tables, fetch a few rows, then use LLM to suggest a bunch of policies, users can pick to add/enable them, and/or future tweak them manually or by iterating with LLM.

### Performance of PolicyHook

- Complex queries or large number of active policies could introduce overhead in transform_up operations on logical plan
- Need performance benchmarks with high volume of policies and complex queries

> **See also:** AU-02 (monitoring query rewrite latency)

### ALTER TABLE ADD COLUMN Idempotency

- Migration rules mention no idempotency guard in SeaORM for ADD COLUMN
- Users must not interrupt this migration
- Explore mitigation at framework level or provide better tooling/guidance

### Error Handling for Policy Definitions

- How are errors in filter_expression or mask_expression handled at runtime?
- Need robust validation during policy creation/update to prevent syntactically incorrect or semantically invalid expressions
- Phase D mentions "Definition validation (parse expressions, check catalog references)" - good

> **See also:** Implementation status mentions "Definition validation (parse expressions, check catalog references)"

### Allow Testing/Preview Policy Before Deployment

- Allow sudo as a user to test out the policy if the policy isn't for the admin

> **See also:** DM-05 (verbose mode to explain why row filtered/masked), DM-04 (canary rollout for testing policies on subset of users)

### Glob Pattern Matching for Schema/Table/Column Names in Obligations

- **Current**: obligation `schema`/`table`/`column` fields support exact match or `*` (match all). No prefix/suffix patterns.
- **Use case**: naming conventions are common — `raw_*` schemas, `tmp_*` tables, `*_id` columns. Forcing a separate obligation per object is verbose and fragile (breaks when new tables are added).
- **Recommendation**: support trailing-`*` glob only (e.g., `table: "raw_*"`). Not `*_foo` or mid-string patterns.
  - Rationale: trailing prefix match covers ~90% of real naming conventions; trivial to implement (`starts_with`); unambiguous to read and write.
  - Full regex: reject — footgun for policy authors, no standard representation across tools.
  - `starts_with`/`ends_with`/`contains` keywords: possible future extension if trailing-`*` proves insufficient.
- **Implementation**: add pattern matching in `visibility_matches()` (`engine/mod.rs`) and `matches_schema_table()` (`hooks/policy.rs`) — same predicate in both to stay consistent.

### Wildcard `*` Support for Column Names in `column_access`

- **Current**: `column_access` columns field is a plain list of exact column names — no wildcard support.
- **Use case**: `columns: ["*"]` combined with `schema: "*"` / `table: "orders"` would deny all columns in a table, effectively making it invisible at the column-metadata level without needing `policy_required` mode.
- **Implementation**: check for `"*"` in the columns list inside the `column_access` deny block in `compute_user_visibility()` (`engine/mod.rs`) and `PolicyHook` (`hooks/policy.rs`). If present, expand to all column names from the matched table's Arrow schema.

### Schema & Table-level Deny Obligation (`object_access: deny`)

- **Problem**: In `open` mode with a global tenant-isolation policy (wildcard `row_filter`), there is no way to hide a specific schema or table from a specific user without switching the entire datasource to `policy_required` mode. `column_access: deny` hides individual columns within a table but cannot hide entire schemas or tables from the catalog.
- **Current workaround**: Switch to `policy_required` — but this forces every user to have explicit permit assignments for every schema and table they need.
- **What this is NOT**: This is not about hiding columns (that's `column_access: deny`). This is about hiding entire schemas or entire tables from the catalog — they become invisible in `information_schema.schemata`/`information_schema.tables`, SQL client sidebars, and query execution.
- **Proposed**: Add a new obligation type that feeds into `compute_user_visibility()` at connect time (alongside the existing `column_access: deny`):
  - **Schema deny**: `schema: "analytics"` → entire schema and all its tables are excluded from the user's filtered `SessionContext`
  - **Table deny**: `schema: "public", table: "payments"` → specific table is excluded while the rest of the schema remains visible
- **Use cases**:
  - Hide an internal `analytics` schema from external partners in `open` mode
  - Hide a `payments` table from support agents who only need `orders` and `customers`
  - Combine with glob patterns (if implemented): hide all `raw_*` schemas from non-engineering users
- This lets operators stack targeted schema/table hiding on top of an `open`-mode datasource without restructuring all assignments.

> **See also:** DS-14 (schema-level deny), DS-15 (table-level deny) in `permission_stories.md`

## UI/UX Improvements

### User Name, Datasource Name, Policy Name Validation

- Currently only have hints, need live validation in the UI

### Improve Hints in the UI

- Create user, datasource, policies, obligations, assignment
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

## Infrastructure & Deployment

### Password & Authentication

- Forget password and reset password
- 2FA or OTP support

## Bugs

- 2026-03-04: DataFusion query error - Invalid function 'pg_get_function_identity_arguments'. Did you mean 'pg_get_statisticsobjdef_columns'?
- 2026-03-04: DataFusion query error - table 'postgres.pg_catalog.pg_statio_user_tables' not found
- 2026-03-04: DataFusion query error - table 'postgres.information_schema.table_constraints' not found
- 2026-03-04: DataFusion query error - Invalid function 'quote_ident'. Did you mean 'date_bin'?
- 2026-03-08: Column masking obligation doesn't work - tested with SSN column, still see the whole value instead of masked
- 2026-03-08: Row filter policy interaction bug - when two separate row filter policies are enabled (e.g., tenant filter on tenant='foo' AND state filter on state!='WY'), the result contains more rows than either policy alone. Both tenant 'foo' rows AND non-WY state rows appear, rather than rows satisfying BOTH conditions.
- Sometimes SQL queries take long time and cause UI to hang - need performance testing, may be missing indexes

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

### Audit Log Bug - Tenant Filter Not Shown in Rewritten Query

- For queries that should have tenant filter, the rewritten query in audit log does not show the tenant filter
- However, data is filtered correctly when querying via the proxy
- Suspect: Query is working correctly but not recorded correctly in audit log
- Need to investigate how rewritten queries are captured in audit log

## Code Review & Refactoring

### Permission Policy Logic & Catalog Context Cache Review

- Review how permission policy logic is currently handled
- Review catalog context cache implementation
- Ensure logic is robust and not scattered everywhere
- Consider big refactoring if needed to reduce tech debt and improve maintainability

Specific areas identified from 2026-03-08 bug fixes — worth revisiting in a dedicated refactoring pass:

#### Duplicated matching logic

`matches_schema_table()` in `hooks/policy.rs` and `visibility_matches()` in `engine/mod.rs` implement the same schema/table wildcard matching predicate. They were written independently and must be kept in sync by hand. A shared utility (e.g., `engine::policy_match::matches_schema_table`) should replace both.

#### `column_access deny` logic is in three places

After the recent fixes, column deny logic lives in:

1. `PolicyHook` permit loop — strips denied columns from permit-policy obligations
2. `PolicyHook` deny loop (newly added) — strips denied columns from deny-policy obligations
3. `compute_user_visibility()` — hides denied columns from the schema at connect time

All three must stay consistent (same matching rules, same column name comparison). A single `collect_column_denies(policies, user_tables)` helper that all three call would eliminate the duplication.

#### `handle_query` is too large

`PolicyHook::handle_query` does plan loading, row filter injection, column mask injection, column access deny, empty-projection guard, access_mode gating, and audit logging — all in one method (~300+ lines). It is hard to unit test individual obligations in isolation. Consider splitting into smaller, testable functions: `apply_row_filters`, `apply_column_masks`, `apply_column_denies`, `check_access_mode`.

#### `rebuild_contexts_for_datasource` has a brief staleness window

When a policy changes, `rebuild_contexts_for_datasource` spawns background tasks that rebuild each active connection's `SessionContext`. Between the policy write and the rebuild completing (typically milliseconds), an in-flight query on an affected connection still sees the old schema. For most use cases this is acceptable, but it is a known gap worth documenting explicitly if stricter guarantees are ever needed (e.g., serialize the rebuild before returning the API response).

#### Session cache TTL vs. immediate invalidation

`PolicyHook` caches loaded policies per `(datasource_id, username)` for 60 seconds. `invalidate_datasource` clears the cache eagerly on policy mutations, so in practice the staleness window is zero for well-behaved callers. However, nothing prevents a future code path from skipping invalidation. A comment or assertion at the cache boundary would make this invariant explicit.

#### `ConnectionEntry` may grow

`ConnectionEntry` in `handler.rs` now holds `ctx`, `user_id`, and `datasource_name`. If future features need additional per-connection state (e.g., active transaction, client application name, audit context), this struct is the right place — but worth a deliberate review rather than ad-hoc field additions.

## Column Access Behavior Configuration

### Problem Context

- Currently: Return "column not found" error when user doesn't have access to a column (preferred, more secure, prevents metadata leak)
- Problem: When admin changes permission, downstream integrations (e.g., BI tools) may fail because they suddenly lose access to a column — they expect it to exist

### Option 1: Datasource-Level Config

- Add a datasource-level config to switch behavior globally per datasource
- Option 1: Return "column not found" error (default, more secure, prevents metadata leak)
- Option 2: Return empty column silently (compatibility mode)

### Option 2: Policy/Obligation Config (Alternative)

- Instead of datasource-level, make this a policy/obligation config for finer-grained control
- When creating a `column_access deny` obligation, add an option to pick the behavior:
  - Option 1: Throw "column not found" error (default, more secure, hides existence of column)
  - Option 2: Return empty/null column silently (compatibility mode, prevents integration failures)
- Rationale: Allows per-policy control over security vs compatibility tradeoff
- Need deeper discussion to decide between Option 1 vs Option 2
