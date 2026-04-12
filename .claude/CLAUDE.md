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
- Push to `main` → tests only (Rust + admin-ui)
- Push `v*` tag → tests → build & publish Docker image (tagged `X.Y.Z` + `X.Y`) → deploy to Fly.io
- `workflow_dispatch` in GitHub Actions → redeploy a specific existing version

Use `/release` to prepare the changelog, bump versions, commit, and tag. Use `/commit` for day-to-day commits.

## General Rules
- **Never hardcode secrets** — API keys, passwords, credentials, and tokens must come from environment variables or encrypted config, never literals in source code.
- **Run the full test suite before considering a task complete** — `cargo test` for proxy, `npm run test:run` for admin-ui. Don't declare done until tests pass.
- **TDD for all new code** — write failing tests first, then implement until they pass. This applies to new features and bug fixes alike, not just the bug fix protocol.

## Planning & Feature Design

### Design-First, Discuss Before Building
For any non-trivial feature, enter plan mode and work through the design iteratively with the user before writing code. Don't jump to implementation — discuss trade-offs, edge cases, and security implications first. The goal is alignment on approach before any code is written.

**Planning workflow:**
1. **Explore** — read the relevant code paths end-to-end. Understand what exists before proposing what to build.
2. **Design** — propose the approach with concrete trade-offs. Present options with pros/cons, not just one solution.
3. **Discuss** — ask the user targeted questions about design decisions. Don't make assumptions on ambiguous points. Use AskUserQuestion for specific choices, not open-ended "what do you think?" questions.
4. **Harden** — after the core design is agreed, proactively ask: "What else can we improve?" Look for security gaps, edge cases, performance concerns, and missing test coverage. Iterate until the user says "enough."
5. **Finalize** — write the plan with all decisions documented, then exit plan mode.

### Test Vector Design During Planning (Non-Optional)
Every feature plan MUST include a comprehensive test case inventory before implementation begins. Tests are designed during planning, not added as an afterthought. The test cases serve as the specification — if you can't write the test case, you don't understand the feature well enough.

**Systematic test categories to cover for every feature:**

| Category | What to ask | Examples |
|----------|------------|---------|
| **Happy path** | Does the basic flow work? | CRUD operations, expected inputs, normal usage |
| **Attack vectors** | Can it be exploited? | SQL injection, parameter tampering, scope mismatches, privilege escalation |
| **Deny-wins / security invariants** | Do security guarantees hold? | Deny overrides allow, deactivation blocks access, audit can't be tampered |
| **State interactions** | How does it interact with existing features? | is_active flags, is_enabled flags, access_mode, template variables |
| **FK cascades / data integrity** | What happens when related entities are deleted? | Delete parent → child cleanup, unique constraint violations |
| **Cache consistency** | Do changes take effect immediately? | Mutation → cache invalidation → next query reflects change |
| **Timing / concurrency** | What about race conditions? | Mid-session changes, concurrent mutations, rapid successive operations |
| **Edge cases** | What about boundary conditions? | Empty sets, max lengths, zero members, duplicate entries |
| **API validation** | Are invalid inputs rejected? | Missing fields, wrong types, out-of-range values, conflicting parameters |
| **Audit integrity** | Are all mutations tracked? | Every CRUD op logged, correct actor, accurate before/after snapshots |
| **Multi-entity interaction** | How do multiple instances interact? | Multiple roles, multiple datasources, overlapping policies, priority conflicts |
| **Backward compatibility** | Does existing functionality still work? | Old API formats, migration of existing data, default values |

**Test naming convention:** Group tests by category with descriptive names. Map security-relevant tests to vector numbers in `docs/security-vectors.md`.

**Adding a new security vector:** When a new feature or bug fix touches access control, policy resolution, or data visibility, add (or update) an entry in `docs/security-vectors.md` using the standard schema defined at the top of that file. Section order is fixed: `**Vector**` → `**Attacks**` → `**Defense**` → `**Previously**` *(only if strengthening an earlier defense)* → `**Status**` *(only for unmitigated threats or accepted trade-offs)* → `**Tests**`. Every attack variant must either have a test back-reference (`— attack N`) in the `**Tests**` section or be explicitly marked under `**Status**`. Never use `**Bug**` as a section label — historical fix context goes in `**Previously**` in past tense. See the top of `docs/security-vectors.md` for the full schema description and `### 13` for a canonical example with multiple attack variants plus a `Previously` section.

### Security-First Thinking
This is a data security product. Every feature that touches access control, policy resolution, or data visibility must be evaluated through a security lens:
- **What can an attacker do?** — enumerate bypass vectors before building defenses
- **What breaks when state changes?** — deactivation, deletion, membership changes, policy mutations
- **What's the blast radius?** — how many users/connections are affected by a change?
- **Is the audit trail complete?** — can every mutation be traced back to who did it and when?

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
