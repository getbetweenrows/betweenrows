# Roadmap

## MVP Checklist

- [x] **Roles (RBAC)** — DAG-based role hierarchy for policy assignment and datasource access. `role`, `role_member`, `role_inheritance` tables. Policy assignments can target a role (`assignment_scope='role'`), and users in the role (including via inheritance) receive those policies. Includes cycle detection, depth cap (10), soft delete, admin audit log, effective policy preview, and immediate cache invalidation for active connections.
- [x] **User Attributes (ABAC)** — Schema-first attribute system: `attribute_definition` table defines allowed keys with types (`string`/`integer`/`boolean`), entity type scoping, and optional enum constraints. User attribute values stored as JSON column on `proxy_user`. Available as typed `{user.*}` template variables in filter/mask expressions. Available in decision function context as first-class fields on `ctx.session.user` (e.g., `ctx.session.user.region`) with typed JSON values. `time.now` (RFC 3339 evaluation timestamp) added to decision context for time-windowed access. ABAC and TBAC (resource tags) unified as "attributes" — same concept applied to different entity types. Resource-level attributes planned for future. No IDP sync for MVP. **Tech debt**: reserved attribute key list is derived from ORM columns + manual extras; consider unifying ORM/DTO/context layers into a formal user identity schema if aliasing diverges.
- [x] **Conditional Policies** — ~~Dropped as a separate feature.~~ Covered by existing mechanisms: `CASE WHEN {user.*}` expressions handle conditional logic in `row_filter` and `column_mask`; decision functions handle conditional gating for all five policy types (including `column_deny`, `table_deny`, `column_allow` which have no expression field). Adding a dedicated `condition` field would duplicate what decision functions already do with no new capabilities — see "ABAC expression patterns" in `docs/permission-system.md` for examples.
- [ ] **Shadow Mode** — Per-policy dry-run state. Instead of blocking/masking, log what would have happened. Removes "fear of breaking prod" adoption blocker. Each policy gets an `action_status` field: `enforce` (default) or `shadow`.
- [ ] **Governance Workflows** — Per-datasource `governance_workflow` setting: none (default, today's behavior), draft (stage changes in sandboxes, deploy to go live), or code (YAML in repo, CI/CD deploys). Includes sandboxes, unified apply endpoint, and version history. See [Governance Workflows](#governance-workflows) below.

## Policy System

### Remaining Integration Test Cases

All TC-* scenarios are now covered by integration tests in `proxy/tests/policy_enforcement.rs`. The list below is empty — new scenarios should be added here before implementation.

### Configurable Policies

Superseded by [Governance Workflows](#governance-workflows) — covers YAML-as-code, version control, audit history, and CI/CD deployment.

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
  - **WASM Linear Memory Limit**: Add a configurable per-function memory cap (e.g., 10 MB default) to prevent a single decision function from allocating unbounded memory. wasmtime supports `Store::limiter()` for this — wire it into `WasmDecisionRuntime::evaluate_bytes()` alongside the existing fuel limit.
  - **Module Cache in PolicyHook**: Pre-compile WASM modules once per `(decision_function_id, version)` and cache them in PolicyHook, instead of recompiling from bytes on every query evaluation. Evict on decision function update/delete. Reduces per-query overhead from ~ms compilation to ~us lookup.
  - **Decision Function Integration Tests**: Add integration tests in `policy_enforcement.rs` that exercise decision functions through the full proxy stack (real WASM evaluation via pgwire). Current coverage is unit tests only (`hooks/policy.rs`). Requires javy CLI in CI.
- **Validated Purpose (PBAC)**: Move beyond roles to "Purposes." Require a validated claim (e.g., a ticket ID from a ticketing system) to unlock specific data lenses.
- **Clean Room Joins**: Support "Blind Joins" where two tables can be joined on a sensitive key, but the proxy guarantees the key cannot be leaked in results or filters.
- **User Attribute Sync**: Dynamically pull ABAC attributes (Region, Department, Clearance) from identity claims at connect time. (Base ABAC with admin-defined attributes is shipped; IDP sync is the next step.)
- **Impact Analysis Engine**: Run a "What-If" simulation of a policy change against historical query logs to identify breaking changes before enforcement.
- **Policy Impersonation (Sudo Mode)**: Admin tool to "Run as User X" to verify policy enforcement and visibility in real-time.

---

## Governance Workflows

### Overview

Governance state (logical schema, policies, decision functions, assignments) needs safe authoring, review, and deployment workflows. A per-datasource `governance_workflow` setting controls whether and which workflow is used:

- **None** (`governance_workflow: none`, default) — no workflow. Edit in UI, changes take effect immediately. Zero ceremony, lowest barrier to entry. Best for getting started or solo admins. This is today's behavior.
- **Draft** (`governance_workflow: draft`) — all governance edits go to a **sandbox** (staged changeset), must be deployed to go live. Safety layer for teams that want review/preview before production changes.
- **Code** (`governance_workflow: code`) — author governance as YAML files in a git repo, review via PRs, deploy via CI/CD. Admin UI is read-only for governance entities on this datasource.

Natural progression: start with none (getting started), graduate to draft (team growing, want safety), then code (mature team, CI/CD everything).

**Terminology:** "Draft" and "code" are the two governance workflows. "Sandbox" is the underlying mechanism — the named, isolated changeset overlay where staged changes live. Sandboxes are used in both draft (mandatory) and code (optional, for local testing) workflows.

### Key Design Decisions

**1. Three tiers of governance rigor**

No workflow, draft workflow, and code workflow form a progression — not interchangeable modes:

| | None | Draft Workflow | Code Workflow |
|---|---|---|---|
| **Author** | UI forms | UI forms | YAML files in any editor |
| **Review** | None — immediate | Sandbox preview via proxy | Git PR review |
| **Test** | Changes are live | Sandbox members connect, see overlay | Apply to sandbox locally, connect, verify |
| **Deploy** | Implicit (on save) | Click "Deploy to production" in UI | CI calls apply endpoint with `target=production` |

**2. No hybrid — one workflow per datasource**

Supporting both UI and code editing on the same datasource creates a two-source-of-truth sync nightmare (see: Datadog + Terraform, Looker LookML git sync). Instead, each datasource picks one workflow. The DB is always the runtime state; the question is who's allowed to write to it.

Alternatives considered:
- *Looker approach* (code editor in UI) — rejected. Requires building an in-app IDE, still needs git sync for proper versioning, and the sync between internal state and git is notoriously fragile.
- *dbt/Cube.js approach* (code lives in repo, UI is for consumption) — adopted for code workflow. Target users (data/platform engineers) already live in repos. Editing happens in whatever editor they already use. Git PR reviews, branch protection, and CI checks come free.

**3. Sandboxes are the universal staging area**

A sandbox is a named changeset overlay stored in the database. The apply endpoint doesn't care how the changeset was authored:

```
POST /api/v1/datasources/{id}/apply?target=production
POST /api/v1/datasources/{id}/apply?target=sandbox/mask-ssn-rollout
```

- In draft workflow: UI forms build up the changeset, saved to a sandbox
- In code workflow: YAML is parsed into a changeset, applied to a sandbox for local testing, or directly to production via CI

**4. Code workflow does not require sandboxes**

In code workflow, git branches + PR review ARE the review process. The typical CI/CD flow:

1. Edit YAML locally
2. (Optional) Apply to a sandbox for local proxy testing
3. Open PR — code review is the gate
4. PR merges → CI applies to production target

Sandboxes are optional in code workflow — useful for local dev testing but not mandatory. In draft workflow, sandboxes are the primary safety mechanism.

**5. Sandboxes do NOT live in git**

Sandboxes are ephemeral, stored in the database. Putting them in git adds friction (must commit to test) for zero benefit. The only thing in git (in code workflow) is the YAML representing production-intended state.

**6. Sandbox membership is admin-controlled and audited**

Admins can add users to a sandbox, routing their proxy connections through the overlay. This uses the same trust model as "admin can change your policies right now" — sandboxes actually make it safer because the change isn't live yet. All sandbox creation, membership changes, and proxy routing are fully audited. No opt-in required from users for v1.

**Future consideration — opt-in sandboxes:** Instead of admins silently routing users through a sandbox, users could be *invited* and must explicitly accept before their connections are affected. They could opt in (e.g., via a toggle in a user portal or a connection parameter) and opt out anytime. This gives users control over when they experience sandbox changes. Not needed for v1 since the admin trust model + audit trail is sufficient, but worth revisiting if the user base grows beyond a small trusted team.

**7. Permission model for deployment targets**

- **Sandbox target** — any admin user can create sandboxes and push to them
- **Production target** — restricted. In code workflow, only a CI/CD service account (API key auth) can deploy to production. In draft workflow, deploy is available to admins through the UI deploy flow with gates.

**8. Physical schema vs logical schema**

Two distinct layers:
- **Physical schema** — what actually exists in the upstream database, populated by catalog discovery. Source of truth for what's available.
- **Logical schema** — what BetweenRows exposes to end users. Defined by which schemas/tables/columns are "selected" from the physical catalog, plus policies that restrict visibility.

Today, logical schema selection (the "selected" state on discovered_schema/table/column) lives only in the DB, editable only via UI. In code workflow, the logical schema is declared in YAML alongside policies:

```yaml
# governance/production.yaml
schema:
  public:
    users:
      - id
      - name
      - email
      - created_at
      # ssn deliberately excluded — not even visible
    orders:
      - id
      - user_id
      - amount
      - status
    # credit_scores table not listed — not exposed at all

policies:
  - name: mask-email-for-analysts
    type: column_mask
    targets:
      - schemas: [public]
        tables: [users]
        columns: [email]
    definition:
      mask_expression: "'***@' || split_part(email, '@', 2)"
    assignments:
      - role: analyst
```

The catalog discovery wizard remains useful as a bootstrapping tool — discover what's upstream, generate the initial YAML (like `terraform import`), then the YAML becomes the source of truth.

**9. Migration across workflows is a natural progression**

1. New user starts with direct (default), clicks around, learns the system
2. Team grows, wants safety → switch to `workflow: draft`, edits now require sandboxes
3. Team matures, wants CI/CD → "Export to YAML" generates config from current DB state
4. Commit the YAML, flip datasource to `workflow: code`
5. Apply endpoint is the only writer; UI becomes read-only for that datasource

### Sandboxes

Named changeset overlays for staging governance changes before they go live. Used in draft workflow (mandatory) and code workflow (optional, for local testing). Not used in direct workflow.

**Data model:**
- `governance_sandbox` — named changeset per datasource with `base_revision` tracking
- `governance_sandbox_member` — users whose proxy connections route through the sandbox overlay
- `governance_history` — immutable record of each deployment for audit and revert
- `governance_revision` on `data_source` — monotonic counter incremented on every deploy

**Changeset format:** Map keyed by `(entity_type, id)`. Each entry is a full entity snapshot (upsert) or a delete marker. Map structure provides automatic dedup — editing the same policy 5 times results in one entry with the final state. Covers: policies, decision functions, policy assignments, catalog (logical schema selection).

**Deploy gates:**
1. Sandbox must be rebased to current `governance_revision` (production hasn't moved ahead)
2. Validation must pass — catalog entries checked against physical schema (missing upstream columns block deploy, type drift is a warning)

**Version history and revert:** Each deploy is recorded with the changeset and an auto-generated summary. Revert creates a new sandbox with an inverse changeset — goes through the same sandbox/review/deploy flow, not a direct rollback.

See `Governance Dev Mode with Branch-Based Changesets.md` for the full technical design including data model, overlay layer, engine changes, API endpoints, and implementation phases. (Note: that doc uses "branch" terminology — will be updated to "sandbox" when implementation begins.)

### Code Workflow

Declarative YAML-based governance management applied via CI/CD.

**Apply endpoint:** `POST /api/v1/datasources/{id}/apply`
- Accepts a YAML manifest, diffs against current state, reconciles
- Supports `dry_run: true` to preview changes without applying
- Idempotent — running multiple times produces the same result
- Name-based identity (not UUIDs) so YAML files are human-readable
- `target` parameter: `production` or `sandbox/<name>`

**CI auth:** API tokens / service accounts for machine-to-machine authentication. Long-lived API keys mapped to a BetweenRows user identity, so audit trail captures the service account. Separate from JWT-based admin UI auth.

**CI/CD flow (e.g., GitHub Actions):**
```
PR merged → GitHub Action runs →
  1. POST /api/v1/datasources/{id}/apply?target=production&dry_run=true  (preview)
  2. POST /api/v1/datasources/{id}/apply?target=production                (apply)
  → Policies and logical schema updated, connections rebuilt
```

**Local dev flow:**
```bash
# Edit YAML locally
# Apply to a sandbox for local testing
curl -X POST .../apply?target=sandbox/my-experiment -d @governance.yaml
# Connect through proxy, verify behavior
# Happy? Open PR → CI deploys to production on merge
```

**Scope of YAML manifest (v1 — minimal):** Policies, policy assignments, and logical schema selection (which tables/columns to expose). Roles and datasources referenced by name and must already exist — they're infrastructure concerns set up once and rarely changed. Future versions may expand scope.

**What code workflow does NOT include (v1):**
- A CLI tool — `curl` + the apply endpoint is sufficient for any CI system
- Managing datasources, users, or roles declaratively
- Pruning/deleting unmanaged resources — additive-only for safety

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

### Expression Parser Coverage (`sql_ast_to_df_expr`)

The custom expression converter in `PolicyHook` (`proxy/src/hooks/policy.rs`) manually converts sqlparser AST to DataFusion `Expr`. It handles a subset of SQL syntax and each new SQL construct must be added by hand. Currently supported: identifiers, literals, binary ops, unary ops, IS NULL/NOT NULL, BETWEEN, LIKE, IN LIST, CAST, scalar functions (via registry), and CASE WHEN. Not yet supported (add as use cases arise):

- ILike (case-insensitive LIKE)
- IsTrue / IsFalse / IsNotTrue / IsNotFalse
- IsDistinctFrom / IsNotDistinctFrom
- InSubquery (subquery in IN clause — needed for RE-11 relationship-based filtering)
- Extract (EXTRACT(field FROM expr))
- Substring, Trim, Overlay, Position (SQL string functions — workaround: use UDF names via function registry)
- Exists / Subquery (correlated subqueries)
- JsonAccess (-> / ->> operators)
- TypedString (e.g., DATE '2024-01-01')
- Interval

**Save-time validation**: as of v0.8, `validate_expression()` dry-run parses filter/mask expressions at policy create/update time and returns 422 if the syntax is unsupported. This prevents silent failures at query time.

**Future refactor — delegate to DataFusion's planner**: Instead of the custom converter, wrap the expression in `SELECT {expr} FROM __dummy__` and let DataFusion's `SqlToRel` handle the full SQL-to-Expr conversion natively. This would support every SQL expression DataFusion supports (CASE, EXTRACT, subqueries, window functions, etc.) without maintaining a custom converter. The previous attempt (pre-bug #13) had issues with qualified column references and double aliasing — a new attempt should address those by using unqualified dummy columns and stripping aliases from the extracted expression. This is a significant refactor that needs careful design and should be covered by the existing integration test suite as a safety net.

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

### Conditional Policies — Resolved (No Dedicated Feature Needed)

Conditional policy behavior is fully covered by existing mechanisms without a dedicated `condition` field:

**For `row_filter` and `column_mask`** — embed `CASE WHEN {user.*}` directly in the expression. User attribute variables are substituted before parsing, so CASE branches evaluate to constants and DataFusion optimizes away the dead branch. See "ABAC expression patterns" in `docs/permission-system.md` for examples.

**For `column_deny`, `table_deny`, `column_allow`** — these have no expression field, so conditional behavior requires a decision function. Decision functions are a strict superset of what a `condition` field would provide (arbitrary JS logic, time checks, multi-attribute combinations).

A dedicated `condition` field was considered but rejected because:
- It adds a second gating mechanism alongside decision functions with no new capabilities
- The ~1ms WASM overhead of decision functions is negligible for real deployments
- Admin UX can be improved by offering simple decision function templates instead

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

Given complexity of new policy system (interaction with DataFusion and PostgreSQL), detailed security test plan needed (see docs/security-vectors.md):

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

Expand beyond PostgreSQL to support additional upstream datasource types. Policy enforcement (row filters, column masks, deny/allow rules, decision functions) operates at the DataFusion `LogicalPlan` level and is already backend-agnostic — the work is in the connection/discovery/dialect plumbing.

#### `datafusion-table-providers` Backends

The same crate we already depend on (`v0.10`, `postgres` feature) ships multiple backends behind Cargo feature flags:

| Feature | Backend | Covers |
|---|---|---|
| `postgres` | PostgreSQL | PG, Redshift, CockroachDB, Aurora, AlloyDB |
| `mysql` | MySQL | MySQL, MariaDB, TiDB |
| `sqlite` | SQLite | SQLite |
| `duckdb` | DuckDB | DuckDB (+ extensions for Iceberg, Delta, S3) |
| `clickhouse` | ClickHouse | ClickHouse |
| `flight` | Arrow Flight SQL | Databricks, Dremio, Trino (experimental) |
| `adbc` | Arrow Database Connectivity | Snowflake (via `adbc_snowflake` driver) |
| `odbc` | ODBC | Anything with an ODBC driver |

All backends implement the same `SqlTable` / `TableProvider` interface with filter pushdown.

#### Cloud Warehouse Paths

| Backend | Best Path | Notes |
|---|---|---|
| **Redshift** | `postgres` feature — Redshift speaks PG wire protocol | Dialect differences (forked from PG 8.x) |
| **Snowflake** | ADBC — official `adbc_snowflake` Rust driver exists | Medium maturity |
| **Databricks** | Flight SQL — native support in SQL Warehouses | Good maturity |
| **Trino/Presto** | Flight SQL (experimental in Trino 411+) or ODBC | Low-medium maturity |
| **BigQuery** | Custom `TableProvider` using Storage Read API | Must build — returns Arrow natively |
| **Athena** | ODBC or custom REST wrapper | Hardest target — async REST-only API |

#### Recommended Priority

1. **PostgreSQL wire protocol as a "type"** — immediately unlocks Redshift, CockroachDB, Aurora, AlloyDB with minimal work (mostly dialect-aware SQL unparsing)
2. **Flight SQL** — gets Databricks, positions for Trino/Dremio
3. **ADBC** — gets Snowflake specifically
4. **MySQL** — large user base, mature support in `datafusion-table-providers`

#### Design: `DatasourceBackend` Trait

Introduce a `DatasourceBackend` trait with a factory function, matching the existing `DiscoveryProvider` pattern. Three concerns, three traits:

| Concern | Trait | Status |
|---|---|---|
| Config field definitions | `DataSourceTypeDef` | Exists (`admin/datasource_types.rs`) |
| Catalog discovery | `DiscoveryProvider` | Exists (`discovery/mod.rs`) |
| Engine / connection / dialect | `DatasourceBackend` | **New — this is the work** |

The `DatasourceBackend` trait covers:
- **Pool creation** — build the backend-specific connection pool
- **Table provider creation** — create a DataFusion `TableProvider` for a specific table
- **SQL dialect** — return the backend-specific `Dialect` impl for filter pushdown unparsing
- **Connection params** — translate decrypted config into backend-specific parameter format

All backend-specific code lives in one directory per backend:

```
proxy/src/backend/
    mod.rs                # DatasourceBackend trait + create_backend() factory
    postgres/             # PostgresBackend, PostgresDiscoveryProvider, BetweenRowsPostgresDialect
    mysql/                # MySQLBackend, MySQLDiscoveryProvider, MySQLDialect
    flight/               # FlightSqlBackend, FlightDiscoveryProvider
```

Adding a new backend = one new directory + one match arm in the factory. Zero changes to engine, policies, RBAC, audit, or admin UI.

#### Key Risk

SQL dialect differences are the main source of bugs. Each backend has different function names, quoting rules, type casting syntax, and JSON operator support. Filter pushdown can generate invalid SQL for a specific backend if the dialect unparser doesn't handle an expression correctly. Per-backend dialect integration tests are essential.

#### Isolation Rules

Each backend must be fully isolated so that changes to one cannot break another:

- **Cargo feature flags** — each backend module is gated behind `#[cfg(feature = "mysql")]` etc. Compiling with `--features postgres` must not pull in MySQL deps or fail to compile. The `postgres` feature is the only default.
- **No cross-backend imports** — `postgres/` never imports from `mysql/`, and vice versa. The only shared surface is the trait interface in `backend/mod.rs`.
- **No `match ds_type` outside the factory** — backend-specific logic lives in the trait impl, not in `if ds_type == "snowflake"` branches scattered through engine code.
- **Test isolation** — each backend's integration tests are in a separate test binary or behind a feature flag so `cargo test --features postgres` doesn't require MySQL Docker to be running.

#### Testing

Two tiers: (1) **DataFusion-native tests** — register in-memory Arrow tables as `TableProvider`s, test all policy logic regardless of backend, free and fast in CI. (2) **Per-backend integration tests** — Docker-based databases or emulators (Postgres, MySQL, LocalStack Snowflake emulator, Trino for Athena), test catalog discovery, SQL unparser correctness, filter pushdown, and type mapping.

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
- 2026-03-18: Catalog cache not invalidated after resync discovers new columns/tables/schemas — newly discovered objects are not immediately visible in queries until the cache expires or is manually cleared

### Git Commit Hook Improvements

- Current: pre-commit hook runs `cargo fmt`, `cargo clippy`, `npm run typecheck`, `npm run test:run` on ALL changes (staged + unstaged)
- Problem: Uncommitted unstaged changes cause false failures - hook fails because of code in files you haven't committed yet
- Proposed: Modify hook to only check staged changes using `git diff --staged` or `git diff --cached`
- This preserves ability to have WIP changes without them interfering with the commit process

## Frontend Architecture: Tailwind Plus Catalyst Adoption

### Approach

Adopt Tailwind Plus Catalyst as the UI component foundation. Catalyst provides accessible, well-designed React components built on Headless UI + Tailwind CSS v4. Components are copied into the project as source code — no runtime dependency on Catalyst itself.

### Setup

- **Full kit**: `admin-ui/catalyst-kit/` (gitignored) — complete Catalyst download for reference
- **Active components**: `admin-ui/src/components/ui/` — only components in use, copied from the kit as needed
- **Dependencies**: `@headlessui/react`, `motion`, `clsx`, `@heroicons/react`
- **Router integration**: `src/components/ui/link.tsx` — wraps `react-router-dom` `Link` for Catalyst compatibility
- **Docs**: https://catalyst.tailwindui.com/docs/{component-name}

### Migration Strategy

Incremental, page-by-page — no big-bang rewrite. Swap raw HTML elements for Catalyst components alongside feature work:
1. Core primitives first: Button, Input, Select, Textarea, Checkbox
2. Layout: Sidebar layout, Navbar (replaces current `Layout.tsx`)
3. Data display: Table, Badge, Description list, Dialog
4. Forms: Fieldset, Radio, Switch, Combobox, Listbox

### What This Replaces

The previous plan (CVA, Atomic Design, strict design tokens, `cn()` helper) is superseded. Catalyst provides variant management, accessible primitives, and consistent design out of the box. No additional abstraction layers needed at the current scale (~25 pages, ~29 components).

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
