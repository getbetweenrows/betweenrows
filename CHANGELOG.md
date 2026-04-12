# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.15.0] - 2026-04-11

### Added

- **[Proxy] Extensive policy enforcement tests for aggregates, HAVING, window functions, CTEs, and subqueries** — +1240 lines in `policy_enforcement.rs` covering how column masks, column denies, and column allows interact with `COUNT(DISTINCT)`, `GROUP BY` / `HAVING`, `ROW_NUMBER() OVER (ORDER BY ...)`, CTEs, and subqueries. Ensures masked values cannot leak through aggregates or window ordering.
- **[Docs] Security vectors documentation overhaul** — major expansion of `docs/security-vectors.md` with new attack vectors, defenses, and test back-references; `docs/permission-system.md` updated in lockstep.
- **[Demo] Ecommerce demo refresh** — new `compose.demo.yaml`, new `setup.sh` automation script, updated `schema.sql` and `seed.py`, refreshed `policies.yaml` and `requirements.txt`, and a rewritten README.

### Changed

- **[Proxy] BREAKING: `ctx.query.tables` is now an array of objects, not strings** — decision functions with `evaluate_context = "query"` previously received `ctx.query.tables` as `string[]` (e.g. `["public.orders"]`). It is now `Array<{datasource, schema, table}>`, so decision function JS must access the fields explicitly. Bare references like `SELECT * FROM orders` now also resolve to the session's default schema (e.g. `public`) rather than an empty schema segment, so qualified and unqualified references produce identical entries.

  Migration for any decision function that inspected `ctx.query.tables`:

  ```js
  // Before:
  ctx.query.tables.includes("public.orders")
  ctx.query.tables.some(t => t.startsWith("public."))

  // After:
  ctx.query.tables.some(t => t.schema === "public" && t.table === "orders")
  ctx.query.tables.some(t => t.schema === "public")
  ```
- **[Admin UI] Form polish** — small tweaks to `CatalogDiscoveryWizard`, `DecisionFunctionModal`, `PolicyForm`, and `DataSourceEditPage`.

### Fixed

- **[Proxy] Security: bare table references could bypass schema-scoped policies** — unqualified references like `FROM orders` previously used an empty schema segment as the policy lookup key, so a policy targeting `schemas: ["public"]` would not match and could be bypassed by omitting the prefix. Bare references now fall back to the session's default schema, which DataFusion is already configured with at connect time (`SET search_path` is blocked upstream by `ReadOnlyHook`). Tracked as vector #71 in `docs/security-vectors.md`.

### Infrastructure

- **[CI] Pre-commit hook runs `docs-site` VitePress build when docs-site changes are staged** — guarded by a `docs-site/node_modules` check so fresh clones without docs deps installed are not blocked.
- **[CI] `docs-site` GitHub Actions job added then disabled** — the job is commented out until `docs-site/` lands in the repo.

## [0.14.1] - 2026-04-09

### Changed

- **[Both] Remove git commit hash from version display** — simplifies the build and `/health` endpoint
  - drops `GIT_COMMIT_SHORT` env var and git-based `build.rs` logic from the proxy
  - sidebar now shows `vX.Y.Z` instead of `vX.Y.Z (abc1234)`
- **[Admin UI] Polish admin UI tables and forms**
  - attribute definitions table: combine display name + description into a single column, show `entity.key` as a monospaced code chip, and render value type as a type signature (e.g. `list<string> ∈ {us, eu}`)
  - roles list: fold description under role name, drop the standalone description column
  - user form: add a permissions description explaining what admin access grants
- **[Admin UI] Rename audit nav and standardize route paths**
  - sidebar section renamed from "Activity" to "Audit"
  - nav labels changed to "Query Logs" / "Admin Logs"
  - `/audit` route renamed to `/query-audit` for consistency with `/admin-audit`

## [0.14.0] - 2026-04-08

### Added

- **[Both] Entity search, copyable IDs, and audit improvements** — server-side search for attribute definitions, entity search dropdowns on audit pages, copyable UUID components across list pages, debounce hook, and new admin/query audit page tests
  - Proxy: search filter on `GET /attribute-definitions`, copyable IDs in audit responses, policy enforcement test coverage for missing attribute defaults
  - Admin UI: `CopyableId` component, `EntitySelect` component, `useDebounce` hook, admin audit & query audit page tests
- **[Both] Version display** — app version and git commit hash shown in sidebar footer
  - Proxy: `/api/version` endpoint serving version from `Cargo.toml` + build-time git commit
  - Admin UI: `useVersion` hook, version display in Layout

### Changed

- **[Both] Debounced search and `keepPreviousData`** — replaced form-submit search with real-time debounced search across all list pages (Users, Roles, Policies, Data Sources, Attributes); added `keepPreviousData` to prevent layout flash during transitions
- **[Admin UI] Sidebar navigation redesign** — grouped nav into Access Control / Data / Activity sections with Heroicons; added "Report an issue" link in footer; username prefixed with `@`
- **[Admin UI] Default value UX improvements** — type-specific placeholders, inline NULL badge when empty, icon-based clear button in attribute definition form
- **[Admin UI] Attribute definitions table** — added entity type column, reordered entity type filter before search input
- **[Admin UI] Audit timeline** ��� reduced page size from 20 to 5 for inline timelines; left-aligned pagination
- **[Admin UI] Table header styling** — consistent `text-xs` sizing across all list page headers
- **[Both] NULL terminology standardized** — replaced inconsistent "no default (null)" phrasing with explicit "NULL" across UI, docs, code comments, and security vectors

### Fixed

- **[Proxy] Zero-column scan** — fixed `EmptyProjectionFixRule` handling when all columns are denied

## [0.13.0] - 2026-04-06

### Added
- **Tenant as custom attribute** — the built-in `tenant` column on `proxy_user` has been removed; tenant is now managed entirely through the ABAC attribute definition system
  - Migration 055 drops the `tenant` column from `proxy_user`
  - `{user.tenant}` template variable still works — resolves from the user's custom attributes
  - `BR_ADMIN_TENANT` env var removed (was already deprecated)
  - `tenant` is no longer a reserved attribute key — can be created/deleted like any other custom attribute
  - Admin UI tenant field removed from user forms and list pages

## [0.12.0] - 2026-04-06

### Added
- **BetweenRows rebrand** — renamed from QueryProxy to BetweenRows across CLI, admin UI, Dockerfile, and configuration files
- **Auto-persisted secrets** — encryption key and JWT secret now follow a three-tier resolution: env var → persisted file → auto-generate and save
  - Keys are persisted to `.betweenrows/` state directory alongside the database, surviving container restarts without explicit env vars
  - Persistence warning on startup if the state directory is missing alongside existing data (likely unmounted volume)
  - Data directory inferred from `BR_ADMIN_DATABASE_URL` for consistent state file placement
- **Startup banner** — displays version and tagline on boot
- **Linux aarch64 support for Javy** — `build.rs` now downloads the correct Javy binary for `linux/aarch64` (ARM servers, Graviton, etc.)
- **Docker quickstart compose** — `compose.quickstart.yaml` for one-command local setup
- **Governance workflows roadmap** — detailed design for three-tier governance (none → draft → code) with sandboxes, YAML-as-code, and CI/CD deployment

### Changed
- **README rewritten as user-facing quickstart** — streamlined for new users with Docker quick start, 5-minute walkthrough, configuration reference, and policy overview
- **Developer docs moved to CONTRIBUTING.md** — architecture details, data model, API reference, and performance notes relocated from README
- **Fly.io deployment docs** — extracted to `docs/deploy-fly.md`
- **SQLx logging suppressed below DEBUG** — `sqlx_logging` now only enabled when `RUST_LOG` includes DEBUG or lower, reducing noise in default `info` mode
- **Dockerfile sets `BR_ADMIN_DATABASE_URL`** — explicitly sets the SQLite path to `/data/proxy_admin.db` for consistent data directory detection

### Infrastructure
- **`.betweenrows/` added to `.gitignore`** — auto-persisted state directory excluded from version control

## [0.11.0] - 2026-03-29

### Fixed
- **Decision function test context** — mock context in the expression editor nested user attributes under an `attributes` key instead of flattening them as top-level fields on `ctx.session.user`, causing runtime errors when testing functions that access custom attributes (e.g. `ctx.session.user.departments`). Added cross-reference comments between `context.rs` and `DecisionFunctionModal.tsx` to prevent future drift.

## [0.10.0] - 2026-03-29

### Changed
- **Flatten user attributes in decision function context** — Custom attributes are now first-class fields on `ctx.session.user` (e.g., `ctx.session.user.region`) instead of nested under `ctx.session.user.attributes`. Built-in fields (`id`, `username`, `tenant`, `roles`) always take priority on collision.
  - **BREAKING**: Existing decision functions referencing `ctx.session.user.attributes.*` must be updated to `ctx.session.user.*`

### Added
- **Expression editor with autocomplete** — Filter and mask expression fields in PolicyForm now use a CodeMirror editor with `{user.*}` template variable autocomplete (built-in + custom attribute definitions).
- **Server-side expression validation** — New `POST /policies/validate-expression` endpoint and "Check" button in the expression editor to validate filter/mask syntax before saving.
- **ORM-derived reserved attribute keys** — Reserved user attribute keys are now computed from the `proxy_user` ORM columns (+ virtual fields like `roles`), preventing accidental collisions with DB-level field names.
- **Conditional policy documentation** — Comprehensive ABAC expression patterns and conditional policy examples for all five policy types added to `docs/permission-system.md`. Conditional Policies marked as resolved in roadmap (covered by `CASE WHEN` expressions + decision functions).

## [0.9.0] - 2026-03-28

### Added
- **List attribute type for ABAC user attributes** — new `"list"` value type for attribute definitions, storing arrays of strings (max 100 elements)
  - Use with `IN ({user.KEY})` in filter expressions; list expands into comma-separated placeholders
  - Empty lists expand to `NULL` (effectively returning no rows)
  - API validates list values as JSON arrays of strings; `allowed_values` constrains individual elements
  - Decision function context includes list attributes as JSON arrays
  - Admin UI: tag/chip input for free-form lists, multi-select checkboxes for lists with allowed values
  - Extracted `AttributeDefinitionForm` component (matches `RoleForm`/`DataSourceForm` pattern)
  - Added PolicyForm validation: blocks submit when decision function toggle is on but no function is attached or reference is stale
  - DecisionFunctionModal: autocomplete hints for per-attribute `ctx.session.user.attributes.*`, test context pre-populated from current user's real attributes
  - Config JSON validation: blocks save on invalid JSON instead of silently defaulting to `{}`

## [0.8.0] - 2026-03-28

### Added
- **User Attributes (ABAC)** — schema-first attribute system for attribute-based access control
  - `attribute_definition` table defines allowed keys with types (`string`/`integer`/`boolean`), entity type scoping, optional enum constraints, and reserved key protection
  - User attribute values stored as JSON column on `proxy_user` with full-replace semantics and write-time validation
  - Typed `{user.KEY}` template variables in filter/mask expressions (Utf8/Int64/Boolean literals)
  - User attributes available in decision function context as `ctx.session.user.attributes` with typed JSON values
  - `time.now` (RFC 3339 evaluation timestamp) added to decision function context for time-windowed access
  - Admin UI: attribute definition list/create/edit pages, user attribute editor with type-aware inputs
  - CRUD API with `?force=true` cascade delete (SQLite `json_remove()` / PostgreSQL `jsonb -`)
  - 3 new migrations (052–054)
- **Save-time expression validation** — filter and mask expressions are validated at policy create/update time; unsupported SQL syntax returns 422 immediately instead of failing silently at query time
  - CASE WHEN expression support added to the expression parser

### Changed
- **Shared WASM runtime** — consolidated `WasmDecisionRuntime` into a single `Arc` singleton created at startup, shared by `PolicyHook`, `EngineCache`, and `AdminState` (replaces per-use instantiation)
- **Security vectors doc renamed** — `docs/permission-security-tests.md` → `docs/security-vectors.md`; added vectors 59–68 covering predicate probing, aggregate inference, EXPLAIN leakage, HAVING bypass, CASE expression bypass, window function ordering, timing side channels, and ABAC-specific vectors

## [0.7.0] - 2026-03-26

### Added
- **Decision functions** — custom JavaScript functions that control when policies fire, evaluated in a sandboxed WASM runtime
  - Two evaluation contexts: `session` (evaluated once at connect) and `query` (evaluated per query)
  - Configurable error handling: `skip` (policy doesn't fire) or `deny` (query blocked)
  - Console log capture with configurable log levels
  - CRUD API with test endpoint for dry-running functions against mock contexts
  - Integrated into visibility-level evaluation: `column_deny` and `table_deny` policies respect decision function results at connect time
- **Decision function admin UI** — modal for creating/editing decision functions with CodeMirror editors
  - JavaScript and JSON editors with `ctx.*`/`config.*` autocomplete
  - Templates for common patterns in create mode
  - Test panel with client-side pre-check and server-side WASM execution
  - Fire/skip/error result badges, shared function warning, optimistic concurrency
  - PolicyForm integration: toggle-based attachment (create new / select existing / edit / detach)

### Fixed
- **Stale decision function reference dead-end** — detaching a deleted function now correctly reveals create/select buttons instead of leaving the user stuck
- **Testcontainers leak** — label containers and clean up orphans to prevent Docker resource exhaustion during test runs

## [0.6.0] - 2026-03-23

### Added
- **RBAC with transactional audit enforcement** — role-based access control with `AuditedTxn` wrapper
  - Roles with hierarchical membership (BFS traversal, cycle detection, depth cap)
  - Policy assignments scoped to user, role, or all
  - `AuditedTxn` ensures every admin mutation is atomically committed with its audit log entries
  - Role deactivation/reactivation cascades to policy visibility
  - Datasource access gating by role membership

### Infrastructure
- **Remove LICENSE from git history** — dropped LICENSE commit from history, added `LICENSE*`/`LICENCE*` to `.dockerignore`

## [0.5.2] - 2026-03-18

### Added
- **Scan-level column masking** — Column masks now apply at the `TableScan` level via `transform_up` instead of only at the top-level Projection, preventing CTE and subquery nodes from bypassing masks by changing the DFSchema qualifier.
  - Masks run before row filters so filters evaluate against raw (unmasked) data
  - Integration tests for multi-table JOINs with scoped column deny, CTE mask bypass prevention, subquery mask enforcement, and combined mask+deny+filter scenarios

## [0.5.1] - 2026-03-17

### Changed
- **Dependency upgrades** — Rust 1.94, DataFusion 52.3, Vite 7, TypeScript 5.9

### Fixed
- **Read-only hook test assertions** — use `as_db_error()` instead of `to_string()` for reliable error matching
- **Row filter projection expansion** — fix CI test failures related to projection expansion and `table_deny` audit status
- **Unused variable warning** — fix compiler warning in `catalog_handlers`

### Infrastructure
- **CI actions upgraded to v5** — `actions/checkout` and `actions/setup-node` updated from v4 to v5 for Node.js 24 support

## [0.5.0] - 2026-03-16

### Added
- **Flat policy type model** — replaced the obligation model with a flat `policy_type` field
  - 5 types: `row_filter`, `column_mask`, `column_allow`, `column_deny`, `table_deny`
  - Removed `column_access` action field; type alone determines behavior
  - 5 new migrations (019–023) to migrate existing schema
- **Zero-trust column model** — qualified projection for JOINs; per-user column visibility enforcement
- **Cast support** — SQL type cast expressions now handled in query processing
- **Catalog hints** — contextual hints surfaced in the catalog discovery UI
- **Policy-centric assignment panel** — rule assignment UI redesigned around policies rather than datasources
- **Datasource assignment on create** — assign a datasource when creating a new rule
- **Audit status tracking** — queries now record a status field in the audit log
- **Audit write rejections** — rejected write queries are now captured in the audit log
- **Container-based integration tests** — replaced manual test scripts with a Docker-based test suite

### Fixed
- **Column mask and row filter bugs** — fixed incorrect mask application and cross-policy row filter interactions
- **Audit duration and rewritten query** — fixed these fields not being recorded correctly
- **SPA routing** — production build now serves `index.html` for client-side routes


## [0.4.0] - 2026-03-08

### Added
- **Policy system** — configurable row filtering, column masking, and column access control via named policies assigned to datasources and users
  - `policy`, `policy_version`, `policy_obligation`, `policy_assignment`, `query_audit_log` database tables (migration 007)
  - `PolicyHook` replaces `RLSHook`; supports `row_filter`, `column_mask`, and `column_access` obligation types
  - Template variables (`{user.tenant}`, `{user.username}`, `{user.id}`) with parse-then-substitute injection safety
  - Wildcard matching (`schema: "*"`, `table: "*"`) in obligation definitions
  - `access_mode` on datasources: `"policy_required"` (default) or `"open"`
  - Optimistic concurrency via `version` field (409 Conflict on mismatch)
  - Immutable `policy_version` snapshots on every mutation for audit traceability
  - Deny policies short-circuit with error before plan execution
  - 60-second per-session cache with `invalidate_datasource` / `invalidate_user` hooks
- **Policy API** — admin-only CRUD and assignment endpoints
  - `GET/POST /policies`, `GET/PUT/DELETE /policies/{id}`
  - `GET/POST /datasources/{id}/policies`, `DELETE /datasources/{id}/policies/{assignment_id}`
- **Query audit log** — async logging of every proxied query
  - `GET /audit/queries` with pagination and filtering by user, datasource, date range
- **Visibility-follows-access** — per-connection, per-user filtered `SessionContext`
  - Users only see tables and columns their policies permit
  - Policy changes take effect immediately without reconnect
- **JSON/JSONB support** — `json` and `jsonb` columns via DataFusion v52 and `datafusion-functions-json`
  - `->` / `->>` operators and JSON UDFs available in queries
  - Filter pushdown to upstream PostgreSQL for supported operators
- **EXPLAIN support** — `EXPLAIN <query>` returns a PostgreSQL-compatible single-column `QUERY PLAN` response
- **Admin UI — Policies** — list, create, and edit policies with an obligation builder; inline enable/disable toggle
- **Admin UI — Policy Assignments** — assign/remove policies per datasource with optional user scope and priority
- **Admin UI — Query Audit** — paginated audit log with original query, rewritten query, and applied policy snapshots
- **Demo e-commerce schema** — `scripts/demo_ecommerce/` with schema, seed script, and example policies
- **Docs** — `docs/permission-system.md` (user guide) and `docs/security-vectors.md` (security test plan)

### Changed
- **Arrow encoding** — migrated to `arrow-pg`; handler simplified; removed `arrow_conversion` and `sql_rewrite` modules

### Infrastructure
- **CI/CD** — split into CI (tests on every push to `main`) and CD (publish + deploy on `v*` tag only)
  - Docker images tagged `X.Y.Z` and `X.Y`; deploy uses explicit version tag for prod traceability
  - `workflow_dispatch` added for manual redeployment of an existing version

## [0.3.0] - 2026-03-04

### Added
- Password toggle visibility on login/password fields
- Password complexity validation
- Catalog viewer page for browsing the discovered catalog
- Button on the data source list view to open the catalog viewer

### Fixed
- `tsc -b` typecheck failure in `client.test.ts`; aligned `typecheck` script accordingly

## [0.2.2] - 2026-03-04

### Fixed
- TypeScript errors in test files (`as unknown as` casts, unused imports) that
  were silently ignored by Vitest/esbuild but caught by `tsc` during Docker build

### Infrastructure
- Add `typecheck` script (`tsc --noEmit`) to `admin-ui` and run it in the
  pre-commit hook before tests, so type errors are caught locally before CI

## [0.2.1] - 2026-03-04

### Added
- `/commit` slash command for Claude Code
- `/release` skill with semver Docker image tagging

### Infrastructure
- Admin-ui test suite with Vitest, integrated into CI

## [0.2.0] - 2026-03-04

### ✨ Added
- **Multi Data Source Management**: The proxy now supports connecting to multiple, dynamically configured upstream data sources.
- **Data Source Admin API & UI**: New endpoints and UI pages for creating, editing, and testing data source configurations.
- **User-to-Data Source Access Control**: Implemented a many-to-many permission model to assign users to specific data sources.
- **Encryption at Rest**: Sensitive data source configuration fields (e.g., passwords) are now encrypted with AES-256-GCM in the database.
- **Engine Cache**: Implemented a cache for DataFusion `SessionContext`s, one for each active data source, to improve performance and resource management.
- **Structured Logging**: Replaced `println!` with `tracing` for structured, level-based logging.

### ♻️ Changed
- **Authentication Flow**: The PostgreSQL `database` parameter in the connection string is now used to select the target data source.
- **Project Version**: Incremented crate versions to `0.2.0` to reflect new feature set.
- **Schema Alias Support**: Catalog discovery now supports schema aliases for more flexible data source mapping.
- **Per-Column Selection**: Catalog discovery wizard allows selecting individual columns per table.
- **Idle Connection Timeout**: pgwire proxy now closes idle connections after a configurable timeout.
- **Fly.io Auto Stop/Start**: Deployment is configured to automatically stop and start machines based on traffic.

## [0.1.0] - (Initial Release)

- Initial implementation of the PostgreSQL wire protocol proxy.
- Authentication for proxy users via Argon2id password hashing.
- Basic query processing using the Apache DataFusion engine.
- Rudimentary admin REST API for user management.
- Initial Admin UI for listing and creating users.
