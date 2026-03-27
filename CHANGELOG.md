# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
- **Docs** — `docs/permission-system.md` (user guide) and `docs/permission-security-tests.md` (security test plan)

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
