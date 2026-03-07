# Project Instructions

## Use Context7 by Default
Always use context7 when I need code generation, setup or configuration steps, or library/API documentation. Automatically use the Context7 MCP tools to resolve library id and get library docs without me having to explicitly ask.

## Project Overview
QueryProxy is a Rust PostgreSQL wire protocol proxy with a React admin UI. See `README.md` for full details.

App-level instructions:
- Rust proxy → `proxy/CLAUDE.md`
- React admin UI → `admin-ui/CLAUDE.md`

## Repo Structure
```
proxy/          Rust proxy server (pgwire, DataFusion, axum)
admin-ui/       React admin frontend (Vite, TanStack Query, Tailwind)
migration/      SeaORM migrations
.githooks/      pre-commit: cargo fmt, clippy, admin-ui tests
.github/workflows/cicd.yml   CI test → Docker publish → Fly deploy
```

## Pre-commit Hook
`.githooks/pre-commit` runs `cargo fmt --check`, `cargo clippy`, and `admin-ui` tests. Enable once per clone:
```bash
git config core.hooksPath .githooks
```

## CI/CD
Push to `main` → GitHub Actions runs Rust tests + admin-ui tests → builds Docker image → deploys to Fly.io.

## Migrations (`migration/`)

### Rules (violations here cause hard-to-fix production incidents)

**One DDL statement per file.**
Never combine `CREATE TABLE` + `CREATE INDEX`, multiple `ALTER TABLE`s, etc. in a single migration. SeaORM does NOT wrap SQLite migrations in a transaction — if a multi-step migration fails mid-way, the already-executed DDL commits immediately and cannot be rolled back. The migration is then not recorded in `seaql_migrations`, so on the next run it retries from the top and fails again on the already-applied step. A single-statement migration is either fully applied or not applied at all.

**Always use `.if_not_exists()`.**
Every `Table::create()` must have `.if_not_exists()`. Every `Index::create()` must have `.if_not_exists()`. This makes migrations safe to retry if a previous run applied the DDL but crashed before recording in `seaql_migrations`.

**Always name indexes.**
SQLite requires a name for every `CREATE INDEX`. A nameless `Index::create()` generates `CREATE INDEX ON ...` which is a syntax error in SQLite. Always call `.name("idx_<table>_<column>")` before `.table(...)`.

**Never rename or delete a migration file that has been applied.**
SeaORM records the file name (without extension) as the migration version in `seaql_migrations`. Renaming or deleting a file whose version is already recorded causes a fatal startup error: "Migration file of version '...' is missing". If you need to change what a migration does, add a new migration — never touch old ones.

**Never touch the SQLite DB directly.**
Never run `rm *.db`, `DROP TABLE`, `DELETE FROM`, `DROP COLUMN`, or modify `seaql_migrations` yourself. If the DB needs manual intervention (e.g. a partial migration left stale state), ask the user to do it. Describe exactly what SQL to run and why.

### Checklist before writing a new migration
1. One DDL operation only
2. `Table::create()` → `.if_not_exists()`
3. `Index::create()` → `.if_not_exists()` + `.name("idx_...")`
4. `ALTER TABLE ADD COLUMN` — no idempotency guard exists in SeaORM; document that users must not interrupt this migration
5. Register the new file in `migration/src/lib.rs` in the correct order
