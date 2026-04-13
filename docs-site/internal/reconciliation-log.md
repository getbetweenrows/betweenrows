# Docs Reconciliation — Closed

**Date closed**: 2026-04-13

## Decision

The earlier plan to reconcile and delete every file in `../docs/` is abandoned. A content-coverage audit showed that four of the five files are betweenrows design-time material with no viable public equivalent, and the split between `../docs/` (design) and `docs-site/docs/` (public) is stable and meaningful.

**Architectural model**: `../docs/` is design-time material owned by betweenrows development; `docs-site/` is a downstream consumer. The dependency flows one way — docs-site reads from `../docs/` and from source code, but nothing in the betweenrows repo outside `docs-site/` references `docs-site/`. betweenrows developers don't think about `docs-site/` when building features.

Drift between the two trees is addressed by the `/docs-sync` command, invoked automatically by `/release` as step 1 of every release and runnable manually mid-cycle or as a whole-codebase deep audit.

See `../../.claude/CLAUDE.md` → "Documentation architecture" and `../../.claude/commands/docs-sync.md` for the full rules and sync mechanism.

## Per-file outcome

| Legacy file | Outcome | Notes |
|---|---|---|
| `../docs/deploy-fly.md` | **Deleted** | `installation/fly.md` is a superset — adds macOS troubleshooting, IPv4-only section, "Next steps" links. No inbound references from code or README. |
| `../docs/permission-system.md` | **Kept as design doc** | Source of development rationale (namespace design, rename fragility, policy semantics). Referenced from Rust and TSX code comments, `proxy/CLAUDE.md`, `.claude/CLAUDE.md`, `README.md`, `CONTRIBUTING.md`. Drives development. |
| `../docs/security-vectors.md` | **Kept as design doc** | Transcluded verbatim into `concepts/threat-model.md` via `<!--@include:-->`. This is the continuous-sync path — every VitePress build rebuilds the public page from the current design source. Zero drift possible. |
| `../docs/roadmap.md` | **Kept as design doc** | Internal roadmap rationale, tech-debt notes, industry parity comparisons, governance YAML examples. Public `about/roadmap.md` is an editorially curated ~11% summary, not a port. Curation happens via `/docs-sync` at release time. |
| `../docs/permission-stories.md` | **Kept as design doc** | ~70 persona-tagged user stories in P0/P1/P2 tiers driving feature prioritization. Two stories ported to `guides/recipes/` (`multi-tenant-isolation.md`, `deny-exceptions.md`); the rest remain internal as a feature-planning source. |

## Why this log is closed

The "status: pending" entries that used to live here assumed a migration was in progress. That migration isn't happening — the split is permanent. Future drift is handled by `/docs-sync`, not by manual reconciliation tracking. If something material changes about the architectural decision itself (e.g., docs-site becomes a sibling repo, or a new design doc gets added), update `../../.claude/CLAUDE.md` → "Documentation architecture" — not this file.
