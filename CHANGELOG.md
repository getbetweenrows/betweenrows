# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
