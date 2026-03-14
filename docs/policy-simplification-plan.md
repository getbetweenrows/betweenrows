# Plan: Unified Policy & Access Model Simplification

> **Status**: This document tracks a multi-phase simplification of the policy engine.
> Phases marked ✅ are **already implemented** in the current codebase.
> Phases marked ⬜ are **proposed future work** and do **not** reflect current behavior.

---

## 1. Goal
Simplify the security model by consolidating "Allow/Deny" logic into the **Policy Effect** and unifying schema, table, and column permissions into a single **`access`** obligation type.

---

## ⬜ Phase 1 — Unified `AccessDef` Struct (`proxy/src/policy_match.rs`)

**Status: Not implemented.** The codebase still uses the separate `ColumnAccessDef` and `ObjectAccessDef` structs.

- **New `AccessDef` Struct**: Replace `ColumnAccessDef` and `ObjectAccessDef` with a single unified structure.
  ```rust
  pub struct AccessDef {
      pub schema: String,               // Supports "*" and globs
      pub table: Option<String>,        // None = Schema level metadata
      pub columns: Option<Vec<String>>, // None = Table level metadata
  }
  ```
- **Explicit Grant Hierarchy (for Permit Policies)**:
  - `{ "schema": "S" }`: Grants visibility to schema `S` (metadata only).
  - `{ "schema": "S", "table": "*" }`: Grants visibility to all tables in `S` (metadata only).
  - `{ "schema": "S", "table": "T", "columns": ["*"] }`: Grants query access to all columns in `T`.
- **Blacklist Hierarchy (for Deny Policies)**:
  - `{ "schema": "S" }`: Blocks the entire schema and everything within.
  - `{ "schema": "S", "table": "T" }`: Blocks the entire table.
  - `{ "schema": "S", "table": "T", "columns": ["C"] }`: Blocks only column `C`.

---

## ✅ Phase 2 — Zero-Trust Enforcement & Column Allow Patterns (`proxy/src/hooks/policy.rs`)

**Status: Implemented.**

- **Zero-Trust Enforcement**: In `policy_required` mode, `row_filter` and `column_mask` obligations are strictly transformations. They do **not** grant table visibility or query access. A separate `Permit` + `column_access "allow"` obligation is always required for access.
- **`column_allow_patterns`**: Permit policies with `column_access` obligations drive a whitelist of allowed columns. Queries are rewritten to project only permitted columns (qualified column names to support JOINs).
- **Current deny row_filter behavior** (unchanged): If a deny policy has a matching `row_filter` obligation, the query is **rejected immediately** with an error. See `permission-system.md` for details.

---

## ✅ Phase 3 — Engine Catalog Visibility (`proxy/src/engine/mod.rs`)

**Status: Implemented.**

- **`compute_user_visibility` Update**: Iterates over `column_access "allow"` obligations to determine which tables are visible to a user.
- Metadata visibility (schemas/tables visible in catalog) is gated on having at least one `column_access "allow"` obligation for that table.
- `object_access "deny"` obligations hide schemas/tables from the user's `SessionContext`.

---

## ⬜ Phase 3b — Soft Deny Row Filters (`proxy/src/hooks/policy.rs`)

**Status: Not implemented. This is a proposed future change.**

> ⚠️ **This contradicts current behavior.** Currently, a deny policy with a `row_filter` match **aborts the query with an error**. The proposal below describes a different future behavior.

- **Proposed Row Filter Refactoring**:
  - **Permit Policy**: Results in `AND (expression)`.
  - **Deny Policy**: Results in `AND NOT (expression)` — rows matching the deny filter are silently excluded instead of aborting.
  - **Soft Deny**: Deny filters will no longer abort the query; they will simply filter out the matching rows.

---

## ⬜ Phase 4 — DTO & Handler Cleanup (`dto.rs`, `policy_handlers.rs`)

**Status: Not implemented.**

- Remove redundant action-based validations that are superseded by the unified `AccessDef` hierarchy.

---

## ⬜ Phase 5 — Comprehensive Tests

**Status: Not implemented.**

- Add tests for the unified hierarchy and "Deny wins" scenarios.
