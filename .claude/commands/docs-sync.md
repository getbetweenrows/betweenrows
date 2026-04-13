---
allowed-tools: Bash(git diff:*), Bash(git log:*), Bash(git tag:*), Bash(git describe:*), Bash(git rev-parse:*), Bash(ls:*), Bash(cd:*), Bash(npm run build:*), Read, Grep, Glob, Edit, Agent
description: Detect drift (and missing/stale coverage) between docs/ design docs + source code and docs-site/ public docs, propose fixes, apply after review
argument-hint: "[range] | --full | --page <path>"
---

## Purpose

`betweenrows/docs/` is the design-time source of truth. `docs-site/docs/` is the public VitePress site, a downstream consumer. This command detects three problems — in-place drift, new-feature coverage gaps, and removed-feature stale coverage — plus one-way dependency rule violations, and proposes topic-level fixes for human review.

Architecture rules are in `.claude/CLAUDE.md` → "Documentation architecture". Read that if you're not familiar.

## Invocation modes

- `/docs-sync` — default: diff since the last git tag
- `/docs-sync <range>` — e.g., `/docs-sync main..HEAD` or `/docs-sync v0.14.1..HEAD`
- `/docs-sync --full` — whole-codebase audit via parallel subagents (slow)
- `/docs-sync --page <path>` — audit one specific docs-site page against its sources

Arguments: `$ARGUMENTS`

## Context

- Repository root: !`git rev-parse --show-toplevel`
- Current HEAD: !`git rev-parse --short HEAD`
- Last tag: !`git describe --tags --abbrev=0 2>/dev/null || echo "(no tags)"`

The changed-file list is computed in Phase 1 once the range is resolved from `$ARGUMENTS`.

## Screenshot recapture

Stale screenshot findings go in **Observations**, never auto-edited drifts — a blind markdown rename from `-v0.14.png` to `-v0.15.png` produces broken image links. If the user opts in, drive recapture end to end by following `docs-site/CLAUDE.md` → "Demo stack (for screenshot capture)" and "Screenshot capture (Playwright MCP)". That file has the full recipe (demo stack commands, Playwright MCP settings, URL map, teardown) — don't invent a procedure.

## Three-phase workflow

Follow this exact flow. Do not skip phases. Do not edit docs-site files until the user confirms in Phase 3.

### Phase 1 — Detect

1. **Resolve the range** from `$ARGUMENTS`:
   - Empty → use `<last-tag>..HEAD` (or `<first-commit>..HEAD` if no tags exist)
   - `--full` → skip the diff-based path and jump to the full-audit subflow below
   - `--page <path>` → skip the diff-based path and jump to the page-audit subflow below
   - Anything else (e.g., `main..HEAD`, `v0.14.1..HEAD`) → use as the range literal

2. **For diff mode**, get the changed file list, split by operation:
   ```sh
   git diff --name-only <range>                  # all touched
   git diff --name-only --diff-filter=A <range>  # added (new feature candidates)
   git diff --name-only --diff-filter=D <range>  # deleted (removed feature candidates)
   ```

3. **Classify each changed file** against the mapping table below. A file may match multiple rows; collect the union of affected docs-site pages. For added/deleted files, also scan for coverage gaps:
   - **Added + mapped**: grep the target page for a distinctive symbol from the new file. If absent, flag as `[new feature]` drift.
   - **Added + unmapped**: flag as Observation — the user decides whether a new page is needed and where.
   - **Deleted**: grep all of `docs-site/docs/` for references to the removed module or its distinctive symbols. Any hits are `[removed feature]` drift.

4. **For each affected page**, use `Read` to load both the current docs-site page content and the relevant design source(s).

5. **Inspect the actual diff** in the source (`git diff <range> -- <file>`) so you can describe *what changed*, not just *that something changed*.

6. **Identify drift**: does the design-source change invalidate something claimed on the public page? Examples of real drift:
   - A new field added to a Rust entity; the matching guide's "Field reference" table doesn't list it
   - A policy semantic changed in `docs/permission-system.md`; the public concept page still describes the old behavior
   - A new template variable added; the `reference/template-expressions.md` table is missing it
   - A form field renamed in `admin-ui/src/components/*Form.tsx`; the corresponding guide still references the old name
   - A screenshot shows UI that no longer matches the current `admin-ui/` layout

7. **Not drift** (don't flag these):
   - Changes to tests, internal tooling, migrations that don't affect user-visible behavior
   - Changes to `docs/roadmap.md` tech-debt sections or implementation notes (not surfaced publicly)
   - Changes to `docs/permission-stories.md` unless a new P0/P1 user-facing story was added
   - Internal refactors with no observable behavior change

### Phase 2 — Propose (print to chat)

**Output the findings directly in your chat response.** No file, no directory, no intermediate artifact. The user reads the findings inline and replies inline.

**Format is topic-level narrative, not a code review.** Each finding is one short paragraph (~2–4 sentences) describing *what* is out of sync and *why*, in prose. **Do NOT** include current-text / proposed-text blocks, line numbers, diff snippets, or confidence scores. The user reviews the actual code edits via `git diff docs-site/` after Phase 3.

**Two separate lists**: **Drifts** are auto-editable in Phase 3; **Observations** are decisions the user needs to make (stale screenshots, unmapped new features, architecture violations, borderline divergences the audit couldn't classify). Don't bury borderline findings in explanatory prose — if in doubt, put it in Observations.

**Output template**:

```
Drifts found: <count>

1. <docs-site page path> — <one-line summary>
<2–4 sentences in prose: what changed in the source, what the page currently says,
what the edit will do. Prefix with "[new feature]" or "[removed feature]" when this
came from an addition/deletion scan.>

2. ...

Observations: <count>

O1. <short title> — <one-line summary>
<1–3 sentences: what the audit noticed, why it isn't a drift, what decision the
user needs to make.>

O2. ...

When you say "apply":
  - Edit the pages listed in Drifts
  - Run `cd docs-site && npm run build` for a dead-link check
  - Report files modified and any build warnings
  (Observations are never applied automatically — they need explicit instructions.)

Reply: `apply` | `apply 1,3` | `skip 2` | `skip all` | `explain 3` | `explain O1`
```

**Edge case — nothing at all**: if both lists are empty, print "No drift detected and no observations." and stop.

**After printing the findings, STOP.** Do not call the Edit tool on any docs-site file during Phase 2. Wait for the user's next message.

### Phase 3 — Apply (only after user confirmation)

1. **Parse the user's reply**:
   - `apply` / `apply all` → apply every Drift. Observations are never applied automatically.
   - `apply N,M,...` → apply only those Drifts
   - `skip N,M,...` → apply every Drift except those
   - `skip all` → abort cleanly, report nothing applied
   - `explain N` or `explain O1` → re-read the relevant source + docs-site files and give a deeper explanation (this IS the place for current/proposed text blocks if helpful), then wait without applying anything
   - Free-form instructions on Observations (e.g. "recapture the screenshots for O2", "draft a new guide for O1") → do exactly what the user asked. For recapture, follow the "Screenshot recapture" section above.
   - Anything else → ask for clarification; do not guess

2. **For each approved finding**, use the `Edit` tool to make the actual change to the docs-site page. Each edit should preserve the page's existing structure (frontmatter, H1, section order). Follow the docs-site writing conventions (see `docs-site/CLAUDE.md`): plain Markdown, absolute paths in cross-page links, `title:` + `description:` frontmatter intact.

3. **After all edits are applied**, run the dead-link check:
   ```sh
   cd docs-site && npm run build
   ```
   If `npm run build` fails, report the specific error to the user and stop — do not try to "fix it up." Dead links or missing includes need human attention.

4. **Final report** to the user:
   - List of pages modified
   - List of findings applied vs. skipped
   - Build result (pass / warnings / fail)
   - Reminder: "Review the actual edits with `git diff docs-site/`"

## Full-audit subflow (`--full`)

Skip the diff-based mapping. Spawn **6 parallel subagents** (`subagent_type: Explore`) covering every docs-site cluster plus a coverage/architecture sweep. Each subagent's prompt must:

- Describe its scope (pages + sources to compare)
- Return structured findings only — no file writes
- Split findings into **Drifts** and **Observations** (never bury borderline items in prose)
- Include read-only / computed fields (`created_at`, `updated_at`, `last_login_at`, etc.) in field-reference checks, but exclude secrets (`password_hash`, `secure_config`, `decision_wasm`)
- Treat stale screenshot version suffixes as Observations, not drifts
- Cap at ~400 words (~600 for Subagent E, which is the largest cluster)

### Subagent clusters

**Subagent A — Policies**
- docs-site pages: `docs-site/docs/guides/policies/*.md` (row-filters, column-masks, column-allow-deny, table-deny, index)
- Sources: `proxy/src/entity/policy.rs` and related entity files, `admin-ui/src/components/*Form.tsx` form fields, `docs/permission-system.md` policy type sections

**Subagent B — Users, roles, attributes**
- docs-site pages: `docs-site/docs/guides/users-roles.md`, `docs-site/docs/guides/attributes.md`
- Sources: `proxy/src/entity/role.rs`, `proxy/src/entity/user.rs`, `proxy/src/entity/attribute_definition.rs`, matching `admin-ui/src/components/*Form.tsx`, `docs/permission-system.md` sections on RBAC/ABAC

**Subagent C — Concepts**
- docs-site pages: `docs-site/docs/concepts/*.md` (policy-model, security-overview, architecture; threat-model is auto-transcluded, skip)
- Sources: `docs/permission-system.md`, `docs/security-vectors.md`, `proxy/src/hooks/policy.rs`, `proxy/src/decision/*.rs`

**Subagent D — Reference**
- docs-site pages: `docs-site/docs/reference/*.md` (policy-types, template-expressions, audit-log-fields, configuration, admin-rest-api, cli, demo-schema, glossary)
- Sources: `proxy/src/entity/*.rs`, environment variable definitions in `proxy/src/config*.rs`, `docs/permission-system.md` template variables section

**Subagent E — Installation, start, operations & getting started**
- docs-site pages: `docs-site/docs/installation/*.md`, `docs-site/docs/start/*.md`, `docs-site/docs/operations/*.md`, `docs-site/docs/guides/data-sources.md`, `docs-site/docs/guides/decision-functions.md`, `docs-site/docs/guides/audit-debugging.md`
- Sources: `Dockerfile`, `scripts/demo_ecommerce/`, `fly.toml` (if exists), `proxy/src/decision/*.rs`, actual environment variables, `Cargo.toml` + `admin-ui/package.json` version strings, `docs/roadmap.md` for operations/known-limitations cross-references
- This cluster is the largest — cap its response at ~600 words instead of the default ~400

**Subagent F — Coverage sweep & architecture audit**
- The *inverse* of drift: find things that exist in source but aren't mentioned anywhere in `docs-site/docs/`, and flag architecture-rule violations.
- Enumerate symbols from `proxy/src/admin/*_handlers.rs` (API surface), `proxy/src/entity/*.rs` (field-reference targets), `admin-ui/src/pages/*Page.tsx` (guide candidates), and env vars parsed in `proxy/src/main.rs` + `proxy/src/admin/mod.rs` + `proxy/src/config*.rs`. For each, grep `docs-site/docs/` for the symbol. Unmentioned symbols are **Observations** (coverage gaps) with a suggested docs home.
- One-way dependency sweep: `grep -rn "docs-site" proxy/ admin-ui/ migration/ docs/ README.md CONTRIBUTING.md CHANGELOG.md`. Hits are **Observations** flagged as architecture violations per `.claude/CLAUDE.md` → "Documentation architecture". Exclude this command file from complaints.
- Cap at ~500 words.

Main thread aggregates Drifts and Observations sequentially across clusters (`1, 2, 3...` and `O1, O2, O3...`), then proceeds to Phase 2 / Phase 3.

## Single-page subflow (`--page <path>`)

When invoked with `--page <path>`, read the target page and identify its sources of truth from the mapping table below (or by inspecting its content for obvious anchors: field tables → entity files + forms; concept claims → design docs + proxy source; version strings → `Cargo.toml`). Then verify the page against those sources, producing drift findings in the same format. Skip the diff-based mapping entirely.

## Mapping table

This is the heart of the diff-mode classification. Each row maps a trigger file pattern to the docs-site pages that might need editorial review.

| Trigger file pattern | docs-site pages to review | Notes |
|---|---|---|
| `docs/security-vectors.md` | *(none — auto-synced)* | Transcluded into `concepts/threat-model.md` via `<!--@include:-->`. Only action: verify `npm run build` still passes. Never flag as drift. |
| `docs/permission-system.md` | `concepts/policy-model.md`, `reference/template-expressions.md`, `reference/policy-types.md`, `guides/policies/*`, `guides/users-roles.md`, `guides/attributes.md`, `guides/decision-functions.md` | Cross-check which public page(s) claim the thing that changed. |
| `docs/roadmap.md` | `about/roadmap.md` | Most internal-only changes (tech debt, implementation notes, bug lists, performance notes) do NOT surface publicly. Only flag if the change adds/removes a user-facing capability. |
| `docs/permission-stories.md` | *(rarely)* `guides/recipes/*` | Only flag if a new P0/P1 story was added that maps to a new user-facing feature worth a recipe. |
| `proxy/src/entity/policy*.rs` | `reference/policy-types.md`, `guides/policies/<type>.md` | Field additions/removals/renames. |
| `proxy/src/entity/role.rs` | `guides/users-roles.md`, `reference/policy-types.md` | |
| `proxy/src/entity/user.rs` | `guides/users-roles.md` | |
| `proxy/src/entity/attribute_definition.rs` | `guides/attributes.md`, `reference/template-expressions.md` | |
| `proxy/src/entity/datasource.rs` | `guides/data-sources.md`, `reference/configuration.md` | |
| `proxy/src/hooks/policy.rs` | `concepts/policy-model.md`, `guides/policies/*` (behavior sections) | Policy evaluation semantics. |
| `proxy/src/decision/*.rs` | `guides/decision-functions.md`, `concepts/policy-model.md` | Decision function runtime. |
| `proxy/src/config*.rs` (new env var) | `reference/configuration.md`, `installation/*` | |
| `admin-ui/src/components/*Form.tsx` | Matching guide's "Field reference" table | Also consider screenshot re-capture if the form layout changed meaningfully. |
| `admin-ui/src/components/Layout.tsx` | `docs-site/CLAUDE.md` (sidebar labels / URL map section); likely screenshot re-capture for affected pages | Navigation changes affect every page's sidebar context. |
| `admin-ui/src/pages/*Page.tsx` | Corresponding guide | Field names, validation messages, tabs. |
| `migration/src/*.rs` (new schema column, env var, or config change) | `reference/configuration.md`, `installation/*` if install steps change | Most migrations don't affect public docs. |
| `Cargo.toml` or `admin-ui/package.json` version bump | Full docs-site version-bump checklist (see `docs-site/CLAUDE.md`) | Version strings in install guides, Docker tags in `installation/docker.md`, Fly commands in `installation/fly.md`. |
| `scripts/demo_ecommerce/*` | `reference/demo-schema.md`, any guide that references demo users/tables | |
| `proxy/src/**` renames, new limits, or new known issues | `operations/known-limitations.md`, `operations/rename-safety.md`, `operations/troubleshooting.md`, `operations/upgrading.md` | Operations pages describe runtime behavior, guardrails, and upgrade paths — cross-check whenever a proxy change adds a constraint, a cap, or a subtle cross-cutting concern. |

Files not in this table (tests, internal tooling, CI configs, CHANGELOG, README edits, migration files that don't change public behavior) should not produce findings.

## Guardrails

- **Never** write to any file in `docs-site/docs/` during Phase 1 or Phase 2. Only Phase 3, and only after explicit user confirmation.
- **Never** propose edits that introduce references from the betweenrows repo back to `docs-site/`. The one-way dependency rule is absolute — see `.claude/CLAUDE.md` → "Documentation architecture". Existing violations discovered by the Subagent F architecture sweep are reported as Observations, not auto-fixed.
- **Never** propose edits to `docs/*.md` files (the design source). `/docs-sync` only touches the public site.
- **Never auto-rename screenshot image references** from one version suffix to another. Screenshot filename versions track capture version; renaming without a real new capture produces broken image links. Route all screenshot version findings through the Observations bucket and the "Screenshot recapture workflow" section.
- **Never drop borderline findings into explanatory prose.** If a subagent judges something "technically fine" but the observation itself is interesting, it goes in Observations. Silent dismissal is the failure mode this command exists to prevent.
- **Never omit read-only / computed fields** (`created_at`, `updated_at`, `last_login_at`, `version`, etc.) from field-reference tables just because they aren't form inputs. They are part of the entity's public contract. Secrets are the only exclusion (`password_hash`, `secure_config`, `decision_wasm`, etc.).
- If `npm run build` fails in Phase 3, stop and report the error. Do not attempt automated fixes — dead links and missing includes need human attention.
- If a finding is ambiguous ("maybe this drifted, maybe it's intentional"), include it in Drifts with a prose note about the ambiguity if the page still needs editing, or in Observations if the decision itself is what's uncertain. Don't drop it.
