//! Transitive-scope column resolution for row-filter policies.
//!
//! When a row filter references a column that isn't present on the target
//! child table (e.g. `org = {user.tenant}` applied to `payments` when only
//! the parent `orders` carries `org`), the rewriter walks admin-designated
//! `column_anchor` rows through `table_relationship` FKs to inject an INNER
//! JOIN chain that exposes the column. The filter is positioned inside the
//! rewrite, with a `Project([target.*])` on top to preserve the scan's
//! output schema.
//!
//! Safety invariants:
//! - Every relationship used has a PK-or-single-col-unique parent column
//!   (validated at `table_relationship` insert time), so INNER JOIN cannot
//!   fan child rows out.
//! - Each `(child_table, resolved_column_name)` has at most one anchor
//!   (DB-enforced via partial unique index).
//! - Max chain depth is hardcoded at 3.
//!
//! See `docs/security-vectors.md` → "Transitive tenancy bypass".

pub mod graph;
