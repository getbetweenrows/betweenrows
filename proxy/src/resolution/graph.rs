//! In-memory graph and rewrite algorithm for column resolution.
//!
//! The caller (in `hooks/policy.rs`) is responsible for providing
//! pre-planned `LogicalPlan`s for parent tables, since building a scan
//! from the session catalog requires an `async` context that `transform_up`
//! can't offer. This keeps the hot path synchronous and predictable.

use std::collections::{BTreeSet, HashMap, HashSet};

use datafusion::common::Column;
use datafusion::common::tree_node::{Transformed, TreeNode, TreeNodeRecursion};
use datafusion::error::DataFusionError;
use datafusion::logical_expr::{Expr, JoinType, LogicalPlan, LogicalPlanBuilder, SubqueryAlias};
use datafusion::sql::TableReference;
use uuid::Uuid;

/// Hardcoded max chain depth. Real-world tenant-style chains rarely exceed
/// three hops; see `docs/security-vectors.md` → transitive tenancy entry for
/// the security-vs-usability tradeoff.
pub const MAX_DEPTH: usize = 3;

/// Reserved alias prefix for internal parent-table scans injected by the
/// rewriter. Documented in the plan — user SQL should not collide.
pub const ALIAS_PREFIX: &str = "__br_anchor_";

/// A single FK → PK edge in the catalog.
#[derive(Debug, Clone)]
pub struct RelationshipEdge {
    pub id: Uuid,
    pub child_schema: String,
    pub child_table: String,
    pub child_column: String,
    pub parent_schema: String,
    pub parent_table: String,
    pub parent_column: String,
}

/// What a `column_anchor` row resolves to. Every anchor is exactly one of:
/// - `Relationship(id)` — the row-filter rewriter walks this `RelationshipEdge`
///   (and possibly further anchored hops) to reach a parent table that carries
///   the column literally under `resolved_column_name`.
/// - `Alias(actual_column_name)` — the column lives on the target table itself
///   but under a different name. The rewriter substitutes the column name
///   inside the filter expression; no join is built.
///
/// The two shapes are a strict superset: aliasing is a zero-hop shortcut that
/// preserves every other invariant (uniqueness per `(child_table, resolved_column)`,
/// deny-wins on unresolvable refs, audit logging).
#[derive(Debug, Clone)]
pub enum AnchorShape {
    Relationship(Uuid),
    Alias(String),
}

/// In-memory snapshot of relationships + anchors + per-table column lists
/// for a datasource. Built once per session by `PolicyHook::load_session`.
#[derive(Debug, Clone, Default)]
pub struct RelationshipSnapshot {
    /// Relationships keyed by id.
    pub relationships: HashMap<Uuid, RelationshipEdge>,
    /// Anchors: `(df_schema, table, resolved_column_name) → AnchorShape`.
    pub anchors: HashMap<(String, String, String), AnchorShape>,
    /// Columns discovered per `(df_schema, table)`.
    pub columns_by_table: HashMap<(String, String), HashSet<String>>,
}

impl RelationshipSnapshot {
    pub fn is_empty(&self) -> bool {
        self.relationships.is_empty() && self.anchors.is_empty()
    }

    fn table_has_column(&self, schema: &str, table: &str, column: &str) -> bool {
        self.columns_by_table
            .get(&(schema.to_string(), table.to_string()))
            .map(|cols| cols.contains(column))
            .unwrap_or(false)
    }

    fn get_anchor_shape(&self, schema: &str, table: &str, column: &str) -> Option<&AnchorShape> {
        let key = (schema.to_string(), table.to_string(), column.to_string());
        self.anchors.get(&key)
    }

    /// Return the relationship edge for a `Relationship`-shaped anchor;
    /// `None` for alias-shaped anchors or absent anchors. Used by the
    /// hop-walker to advance one FK step at a time.
    fn get_anchor_edge(
        &self,
        schema: &str,
        table: &str,
        column: &str,
    ) -> Option<&RelationshipEdge> {
        match self.get_anchor_shape(schema, table, column)? {
            AnchorShape::Relationship(rel_id) => self.relationships.get(rel_id),
            AnchorShape::Alias(_) => None,
        }
    }

    /// All parent `(schema, table)` pairs that would be traversed to resolve
    /// the given filter expression starting at `(target_schema, target_table)`.
    /// Used by the caller to precompute parent scans before entering the
    /// synchronous rewrite.
    ///
    /// Returns `Ok(empty set)` when no resolution is needed (fast path):
    /// - The snapshot has no column info for the target (e.g. tests, or a
    ///   table outside the discovered catalog). The caller's `apply_row_filters`
    ///   then checks the *scan's* schema directly and deny-wins if any filter
    ///   column is missing — see `UnresolvableFilterColumn`.
    /// - Every referenced unqualified column is present on the target.
    pub fn parents_needed_for(
        &self,
        target_schema: &str,
        target_table: &str,
        filter_expr: &Expr,
    ) -> Result<HashSet<(String, String)>, ResolutionError> {
        if !self
            .columns_by_table
            .contains_key(&(target_schema.to_string(), target_table.to_string()))
        {
            return Ok(HashSet::new());
        }

        let referenced =
            expr_columns(filter_expr).map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;
        let target_columns: HashSet<String> = self
            .columns_by_table
            .get(&(target_schema.to_string(), target_table.to_string()))
            .cloned()
            .unwrap_or_default();

        let missing: Vec<String> = referenced
            .iter()
            .filter(|c| c.relation.is_none())
            .map(|c| c.name.clone())
            .filter(|name| !target_columns.contains(name))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let mut out: HashSet<(String, String)> = HashSet::new();
        for col_name in &missing {
            let mut counter: usize = 0;
            // Alias anchors need no parent scan — they're pure filter-expression
            // rewrites. Only hop-bearing resolutions contribute parent tables.
            match resolve_column(target_schema, target_table, col_name, self, &mut counter)? {
                ResolvedColumn::Hops(chain) => {
                    for hop in chain.hops {
                        out.insert((
                            hop.edge.parent_schema.clone(),
                            hop.edge.parent_table.clone(),
                        ));
                    }
                }
                ResolvedColumn::Alias(_) => {}
            }
        }
        Ok(out)
    }
}

#[derive(Debug)]
pub enum ResolutionError {
    NoColumnAnchor {
        schema: String,
        table: String,
        column: String,
    },
    DepthLimitExceeded {
        schema: String,
        table: String,
        column: String,
    },
    ChainCycle {
        schema: String,
        table: String,
        column: String,
    },
    QualifiedParentRefNotSupported(String),
    MissingParentScan {
        schema: String,
        table: String,
    },
    /// The filter references an unqualified column that is not present on the
    /// target `TableScan`'s output schema AND resolution via parent anchors
    /// is unavailable (no snapshot / empty snapshot / no anchor on target).
    /// Without a resolution path we can't safely evaluate the filter, so
    /// deny-wins is the only safe response.
    UnresolvableFilterColumn {
        schema: String,
        table: String,
        column: String,
    },
    PlanBuild(String),
}

impl std::fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoColumnAnchor {
                schema,
                table,
                column,
            } => write!(
                f,
                "No column anchor for column '{column}' on table '{schema}.{table}'"
            ),
            Self::DepthLimitExceeded {
                schema,
                table,
                column,
            } => write!(
                f,
                "Cannot resolve column '{column}' from '{schema}.{table}': \
                 relationship chain walked {MAX_DEPTH} hops without finding a \
                 designated anchor on any parent table"
            ),
            Self::ChainCycle {
                schema,
                table,
                column,
            } => write!(
                f,
                "Chain for column '{column}' starting at '{schema}.{table}' contains a cycle"
            ),
            Self::QualifiedParentRefNotSupported(n) => write!(
                f,
                "Row-filter column '{n}' is qualified; qualified parent references are not supported in v1"
            ),
            Self::MissingParentScan { schema, table } => {
                write!(f, "Parent scan for '{schema}.{table}' was not pre-planned")
            }
            Self::UnresolvableFilterColumn {
                schema,
                table,
                column,
            } => write!(
                f,
                "Row filter on '{schema}.{table}' references column '{column}' \
                 which is neither on the scan's output schema nor resolvable \
                 via any configured column anchor"
            ),
            Self::PlanBuild(msg) => write!(f, "DataFusion plan build failed: {msg}"),
        }
    }
}

impl std::error::Error for ResolutionError {}

/// Complete rewritten plan — replaces the original `TableScan(target)` subtree.
#[derive(Debug)]
pub struct ResolutionPlan {
    pub plan: LogicalPlan,
}

#[derive(Debug, Clone)]
struct ResolvedHop {
    alias: String,
    edge: RelationshipEdge,
}

struct ColumnResolution {
    hops: Vec<ResolvedHop>,
    terminal_alias: String,
}

/// Outcome of resolving a single missing column reference.
///
/// - `Hops` — walk a chain of FK-anchored joins; the final parent table carries
///   the column literally. Used by `build_column_resolution_plan` to build
///   joins and by `parents_needed_for` to pre-plan parent scans.
/// - `Alias` — the column is the target table's own column renamed in the
///   filter expression. No joins, no parent scans — just a name swap.
enum ResolvedColumn {
    Hops(ColumnResolution),
    Alias(String),
}

fn expr_columns(expr: &Expr) -> Result<HashSet<Column>, DataFusionError> {
    let mut out: HashSet<Column> = HashSet::new();
    expr.apply(|e| {
        if let Expr::Column(c) = e {
            out.insert(c.clone());
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    Ok(out)
}

/// The set of unqualified column names referenced by `expr`. Used by the
/// row-filter rewriter's fast path to verify, before wrapping a scan with
/// a raw `Filter(expr)`, that every column the filter expects is present on
/// the scan's output schema.
pub fn expr_column_names(expr: &Expr) -> Result<HashSet<String>, DataFusionError> {
    Ok(expr_columns(expr)?.into_iter().map(|c| c.name).collect())
}

/// Rewrite a filter expression for the row-filter planner.
///
/// Three passes combined into one tree walk:
/// 1. Unqualified columns in `rename_columns` → replaced with an unqualified
///    column under the *actual* name. Used for same-table alias anchors
///    (`tenant_id` → `org_id`, no join involved).
/// 2. Unqualified columns in `requalify` → qualified with their terminal join
///    alias (`org` → `__br_anchor_orders_0.org`). Used when an FK-walk anchor
///    resolved the column to a parent scan.
/// 3. Any qualified reference to `target_table` → normalized to
///    `TableReference::bare(target_table)` so the final Filter finds the
///    column on the target side of every join.
///
/// Rename beats requalify on a collision (an alias-shape anchor always wins
/// over an accidentally-co-present FK-walk anchor, though in practice the
/// uniqueness index on `(datasource, child_table, resolved_column)` prevents
/// both from existing for the same name).
fn requalify_expr(
    expr: &Expr,
    target_table: &str,
    requalify: &HashMap<String, String>,
    rename_columns: &HashMap<String, String>,
) -> Result<Expr, DataFusionError> {
    expr.clone()
        .transform(|e| match e {
            Expr::Column(Column {
                relation: None,
                ref name,
                ..
            }) if rename_columns.contains_key(name) => {
                let actual = rename_columns[name].clone();
                Ok(Transformed::yes(Expr::Column(Column::new_unqualified(
                    actual,
                ))))
            }
            Expr::Column(Column {
                relation: None,
                name,
                ..
            }) if requalify.contains_key(&name) => {
                let alias = requalify[&name].clone();
                Ok(Transformed::yes(Expr::Column(Column::new(
                    Some(TableReference::bare(alias)),
                    name,
                ))))
            }
            Expr::Column(Column {
                relation: Some(ref rel),
                ref name,
                ..
            }) if rel.table() == target_table => Ok(Transformed::yes(Expr::Column(Column::new(
                Some(TableReference::bare(target_table.to_string())),
                name.clone(),
            )))),
            other => Ok(Transformed::no(other)),
        })
        .map(|t| t.data)
}

fn resolve_column(
    target_schema: &str,
    target_table: &str,
    column: &str,
    snapshot: &RelationshipSnapshot,
    alias_counter: &mut usize,
) -> Result<ResolvedColumn, ResolutionError> {
    // Same-table alias: a zero-hop shortcut. The first anchor lookup decides;
    // aliasing after an FK hop is not supported in v1 (the anchor system pins
    // resolution at the *target* table, not at intermediate hops).
    if let Some(AnchorShape::Alias(actual)) =
        snapshot.get_anchor_shape(target_schema, target_table, column)
    {
        return Ok(ResolvedColumn::Alias(actual.clone()));
    }

    let mut hops: Vec<ResolvedHop> = Vec::new();
    let mut current_schema = target_schema.to_string();
    let mut current_table = target_table.to_string();
    let mut visited: HashSet<(String, String)> = HashSet::new();
    visited.insert((current_schema.clone(), current_table.clone()));

    for _ in 0..MAX_DEPTH {
        let edge = snapshot
            .get_anchor_edge(&current_schema, &current_table, column)
            .ok_or_else(|| ResolutionError::NoColumnAnchor {
                schema: current_schema.clone(),
                table: current_table.clone(),
                column: column.to_string(),
            })?;

        let hop_alias = format!("{}{}_{}", ALIAS_PREFIX, edge.parent_table, *alias_counter);
        *alias_counter += 1;

        hops.push(ResolvedHop {
            alias: hop_alias.clone(),
            edge: edge.clone(),
        });

        if snapshot.table_has_column(&edge.parent_schema, &edge.parent_table, column) {
            return Ok(ResolvedColumn::Hops(ColumnResolution {
                hops,
                terminal_alias: hop_alias,
            }));
        }

        let next_key = (edge.parent_schema.clone(), edge.parent_table.clone());
        if !visited.insert(next_key.clone()) {
            return Err(ResolutionError::ChainCycle {
                schema: target_schema.to_string(),
                table: target_table.to_string(),
                column: column.to_string(),
            });
        }

        current_schema = edge.parent_schema.clone();
        current_table = edge.parent_table.clone();
    }

    Err(ResolutionError::DepthLimitExceeded {
        schema: target_schema.to_string(),
        table: target_table.to_string(),
        column: column.to_string(),
    })
}

/// Build a rewritten plan that resolves `filter_expr`'s missing columns.
///
/// `target_scan` is the original `TableScan` subtree for the target table.
/// `parent_scans` maps `(schema, table)` to pre-planned `LogicalPlan`s for
/// each parent table that will be joined. The caller is responsible for
/// populating this map — see `RelationshipSnapshot::parents_needed_for`.
///
/// Returns `Ok(None)` when every column is already on the target (fast path;
/// caller falls through to today's `Filter(expr, scan)` behavior).
pub fn build_column_resolution_plan(
    target_schema: &str,
    target_table: &str,
    target_scan: LogicalPlan,
    filter_expr: &Expr,
    snapshot: &RelationshipSnapshot,
    parent_scans: &HashMap<(String, String), LogicalPlan>,
) -> Result<Option<ResolutionPlan>, ResolutionError> {
    // No column info for this target → treat as fast path. The caller
    // (`apply_row_filters`) does the scan-schema check against the actual
    // `TableScan.schema()` and surfaces `UnresolvableFilterColumn` if the
    // filter references a column the scan doesn't carry. See vector 73
    // Defense → the 5th failure mode.
    if !snapshot
        .columns_by_table
        .contains_key(&(target_schema.to_string(), target_table.to_string()))
    {
        return Ok(None);
    }

    let referenced =
        expr_columns(filter_expr).map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;

    for c in &referenced {
        if let Some(rel) = &c.relation
            && rel.table() != target_table
        {
            return Err(ResolutionError::QualifiedParentRefNotSupported(
                c.name.clone(),
            ));
        }
    }

    let target_columns: HashSet<String> = snapshot
        .columns_by_table
        .get(&(target_schema.to_string(), target_table.to_string()))
        .cloned()
        .unwrap_or_default();

    // Collect via `BTreeSet` so iteration order is lexicographic and stable
    // across runs. The alias counter below assigns `__br_anchor_<table>_<n>`
    // in iteration order; a non-deterministic order would produce plans that
    // differ by alias suffix across identical queries, breaking EXPLAIN
    // snapshotting and split-brain load-balanced backends.
    let missing: Vec<String> = referenced
        .iter()
        .filter(|c| c.relation.is_none())
        .map(|c| c.name.clone())
        .filter(|name| !target_columns.contains(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    if missing.is_empty() {
        return Ok(None);
    }

    // Capture the target scan's output schema before moving it into the
    // builder. The top projection must emit exactly these columns (with the
    // caller's qualifiers) so downstream nodes — e.g., a mask Projection
    // that was applied above the scan — see the same schema they expected.
    // Sourcing from `columns_by_table` breaks when the scan is a partial
    // projection (`SELECT id, amount`), a zero-column projection (DF 52+
    // `SELECT COUNT(*)` optimization), or a mask-wrapped scan whose field
    // set differs from the discovered-column list.
    let target_scan_columns: Vec<Column> = target_scan.schema().columns();

    let mut alias_counter: usize = 0;
    let mut terminal_alias_by_col: HashMap<String, String> = HashMap::new();
    let mut rename_by_col: HashMap<String, String> = HashMap::new();
    // Dedup hops by their full tuple so two columns routing through the same
    // FK share one join in the final plan.
    type HopKey = (String, String, String, String, String, String);
    let mut dedup: HashMap<HopKey, String> = HashMap::new();
    let mut ordered_hops: Vec<(ResolvedHop, String)> = Vec::new(); // (hop, child_alias)

    for col_name in &missing {
        let resolution = resolve_column(
            target_schema,
            target_table,
            col_name,
            snapshot,
            &mut alias_counter,
        )?;

        match resolution {
            ResolvedColumn::Alias(actual) => {
                rename_by_col.insert(col_name.clone(), actual);
            }
            ResolvedColumn::Hops(chain) => {
                let hops_len = chain.hops.len();
                let mut prev_alias = target_table.to_string();
                let mut final_alias = chain.terminal_alias.clone();
                for (i, hop) in chain.hops.into_iter().enumerate() {
                    let key: HopKey = (
                        hop.edge.child_schema.clone(),
                        hop.edge.child_table.clone(),
                        hop.edge.child_column.clone(),
                        hop.edge.parent_schema.clone(),
                        hop.edge.parent_table.clone(),
                        hop.edge.parent_column.clone(),
                    );
                    match dedup.get(&key) {
                        Some(existing_alias) => {
                            if i == hops_len - 1 {
                                final_alias = existing_alias.clone();
                            }
                            prev_alias = existing_alias.clone();
                        }
                        None => {
                            dedup.insert(key, hop.alias.clone());
                            let child_alias = prev_alias.clone();
                            prev_alias = hop.alias.clone();
                            ordered_hops.push((hop, child_alias));
                        }
                    }
                }
                terminal_alias_by_col.insert(col_name.clone(), final_alias);
            }
        }
    }

    // Pure-alias fast path: no joins needed, just rewrite the filter's column
    // names in place. Returning the target scan unwrapped lets the caller's
    // `apply_row_filters` wrap it with `Filter(rewritten_expr, ...)` exactly
    // like the "column already on target" fast path, while still letting us
    // honor the alias. We wrap the Filter here so the caller doesn't have to
    // know about the rewrite.
    if ordered_hops.is_empty() {
        let rewritten = requalify_expr(
            filter_expr,
            target_table,
            &terminal_alias_by_col,
            &rename_by_col,
        )
        .map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;
        let plan = LogicalPlanBuilder::from(target_scan)
            .filter(rewritten)
            .and_then(|b| b.build())
            .map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;
        return Ok(Some(ResolutionPlan { plan }));
    }

    // Build the join chain.
    let mut builder = LogicalPlanBuilder::from(target_scan);
    for (hop, child_alias) in &ordered_hops {
        let parent_plan = parent_scans
            .get(&(
                hop.edge.parent_schema.clone(),
                hop.edge.parent_table.clone(),
            ))
            .cloned()
            .ok_or_else(|| ResolutionError::MissingParentScan {
                schema: hop.edge.parent_schema.clone(),
                table: hop.edge.parent_table.clone(),
            })?;
        let aliased = LogicalPlan::SubqueryAlias(
            SubqueryAlias::try_new(std::sync::Arc::new(parent_plan), hop.alias.clone())
                .map_err(|e| ResolutionError::PlanBuild(e.to_string()))?,
        );

        let left = Expr::Column(Column::new(
            Some(TableReference::bare(child_alias.clone())),
            hop.edge.child_column.clone(),
        ));
        let right = Expr::Column(Column::new(
            Some(TableReference::bare(hop.alias.clone())),
            hop.edge.parent_column.clone(),
        ));
        builder = builder
            .join_on(aliased, JoinType::Inner, vec![left.eq(right)])
            .map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;
    }

    // Rewrite filter expression so missing-column refs point at their
    // terminal aliases (FK walk) or their actual column names (alias).
    let rewritten = requalify_expr(
        filter_expr,
        target_table,
        &terminal_alias_by_col,
        &rename_by_col,
    )
    .map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;
    builder = builder
        .filter(rewritten)
        .map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;

    // Project the target scan's original columns so downstream sees exactly
    // the schema the caller fed in. Skip the projection entirely when the
    // scan is zero-column (DF 52+ `SELECT COUNT(*)` optimization) — a
    // projection over zero columns would still be non-empty (it'd leak the
    // joined parent columns) and also produces an invalid plan.
    if !target_scan_columns.is_empty() {
        let project_cols: Vec<Expr> = target_scan_columns.into_iter().map(Expr::Column).collect();
        builder = builder
            .project(project_cols)
            .map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;
    }

    let plan = builder
        .build()
        .map_err(|e| ResolutionError::PlanBuild(e.to_string()))?;
    Ok(Some(ResolutionPlan { plan }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use datafusion::datasource::empty::EmptyTable;
    use datafusion::logical_expr::{LogicalPlanBuilder, lit};
    use datafusion::prelude::col;
    use std::sync::Arc as StdArc;

    fn scan(schema_table: &str, columns: &[&str]) -> LogicalPlan {
        let fields: Vec<Field> = columns
            .iter()
            .map(|n| Field::new(*n, DataType::Utf8, true))
            .collect();
        let schema = StdArc::new(Schema::new(fields));
        let table = StdArc::new(EmptyTable::new(schema));
        let source = StdArc::new(datafusion::datasource::DefaultTableSource::new(table));
        LogicalPlanBuilder::scan(schema_table, source, None)
            .unwrap()
            .build()
            .unwrap()
    }

    fn relationship_id(
        child_schema: &str,
        child_table: &str,
        child_col: &str,
        parent_schema: &str,
        parent_table: &str,
        parent_col: &str,
    ) -> Uuid {
        let key = format!(
            "{child_schema}.{child_table}.{child_col}→{parent_schema}.{parent_table}.{parent_col}"
        );
        Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes())
    }

    fn edge(
        child_schema: &str,
        child_table: &str,
        child_col: &str,
        parent_schema: &str,
        parent_table: &str,
        parent_col: &str,
    ) -> RelationshipEdge {
        RelationshipEdge {
            id: relationship_id(
                child_schema,
                child_table,
                child_col,
                parent_schema,
                parent_table,
                parent_col,
            ),
            child_schema: child_schema.into(),
            child_table: child_table.into(),
            child_column: child_col.into(),
            parent_schema: parent_schema.into(),
            parent_table: parent_table.into(),
            parent_column: parent_col.into(),
        }
    }

    fn snap_with(
        edges: &[RelationshipEdge],
        anchors: &[(&str, &str, &str)], // (schema, table, col) → FK-walk anchor via first-match edge
        columns_by_table: &[(&str, &str, &[&str])],
    ) -> RelationshipSnapshot {
        snap_with_aliases(edges, anchors, &[], columns_by_table)
    }

    /// Variant of `snap_with` that additionally registers same-table alias
    /// anchors. Each alias entry is `(schema, table, resolved_col, actual_col)`.
    fn snap_with_aliases(
        edges: &[RelationshipEdge],
        relationship_anchors: &[(&str, &str, &str)],
        alias_anchors: &[(&str, &str, &str, &str)],
        columns_by_table: &[(&str, &str, &[&str])],
    ) -> RelationshipSnapshot {
        let mut relationships = HashMap::new();
        for e in edges {
            relationships.insert(e.id, e.clone());
        }

        let mut anchor_map: HashMap<(String, String, String), AnchorShape> = HashMap::new();
        for (schema, table, col) in relationship_anchors {
            let eid = edges
                .iter()
                .find(|e| e.child_schema == *schema && e.child_table == *table)
                .unwrap()
                .id;
            anchor_map.insert(
                (schema.to_string(), table.to_string(), col.to_string()),
                AnchorShape::Relationship(eid),
            );
        }
        for (schema, table, resolved_col, actual_col) in alias_anchors {
            anchor_map.insert(
                (
                    schema.to_string(),
                    table.to_string(),
                    resolved_col.to_string(),
                ),
                AnchorShape::Alias(actual_col.to_string()),
            );
        }

        let mut cbt: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for (schema, table, cols) in columns_by_table {
            cbt.insert(
                (schema.to_string(), table.to_string()),
                cols.iter().map(|s| s.to_string()).collect(),
            );
        }

        RelationshipSnapshot {
            relationships,
            anchors: anchor_map,
            columns_by_table: cbt,
        }
    }

    #[test]
    fn fast_path_when_snapshot_has_no_target_columns() {
        // Without catalog info for the target, the resolver hands off to the
        // caller (`apply_row_filters`), which does the scan-schema check.
        // The resolver itself returns Ok(None) — it can't classify columns
        // without the catalog, so it defers to the scan's own schema.
        let snap = RelationshipSnapshot::default();
        let target_scan = scan("public.payments", &["order_id"]);
        let filter = col("order_id").eq(lit("X"));
        let parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        let plan = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap();
        assert!(
            plan.is_none(),
            "expected fast-path hand-off to caller, got Some"
        );
    }

    #[test]
    fn fast_path_when_column_present_on_target() {
        let snap = snap_with(&[], &[], &[("public", "customers", &["id", "org"])]);
        let target_scan = scan("public.customers", &["id", "org"]);
        let filter = col("org").eq(lit("acme"));
        let parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        let plan = build_column_resolution_plan(
            "public",
            "customers",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap();
        assert!(plan.is_none(), "expected fast-path, got Some");
    }

    #[test]
    fn single_hop_resolves() {
        let e = edge("public", "payments", "order_id", "public", "orders", "id");
        let snap = snap_with(
            &[e],
            &[("public", "payments", "org")],
            &[
                ("public", "payments", &["id", "order_id"]),
                ("public", "orders", &["id", "org"]),
            ],
        );

        let target_scan = scan("public.payments", &["id", "order_id"]);
        let parent_scan = scan("public.orders", &["id", "org"]);
        let mut parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        parents.insert(("public".into(), "orders".into()), parent_scan);

        let filter = col("org").eq(lit("acme"));
        let plan = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap()
        .expect("expected Some(ResolutionPlan)");

        let rendered = format!("{}", plan.plan.display_indent());
        // Must contain the anchor alias, the filter against the parent column,
        // an inner join with the target, and a final projection back to target.*.
        assert!(
            rendered.contains("__br_anchor_orders_"),
            "missing anchor alias: {rendered}"
        );
        assert!(
            rendered.contains("Inner Join"),
            "missing inner join: {rendered}"
        );
        assert!(
            rendered.contains("Projection"),
            "missing projection: {rendered}"
        );
        assert!(rendered.contains("Filter"), "missing filter: {rendered}");
        // Projection should only reference target columns.
        assert!(
            rendered.contains("payments.id") && rendered.contains("payments.order_id"),
            "projection missing target columns: {rendered}"
        );
    }

    #[test]
    fn multi_hop_resolves() {
        let e1 = edge(
            "public",
            "order_items",
            "order_id",
            "public",
            "orders",
            "id",
        );
        let e2 = edge(
            "public",
            "orders",
            "organization_id",
            "public",
            "organizations",
            "id",
        );
        let snap = snap_with(
            &[e1, e2],
            &[
                ("public", "order_items", "org"),
                ("public", "orders", "org"),
            ],
            &[
                ("public", "order_items", &["id", "order_id"]),
                ("public", "orders", &["id", "organization_id"]), // no org here
                ("public", "organizations", &["id", "org"]),
            ],
        );

        let target_scan = scan("public.order_items", &["id", "order_id"]);
        let orders_scan = scan("public.orders", &["id", "organization_id"]);
        let orgs_scan = scan("public.organizations", &["id", "org"]);
        let mut parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        parents.insert(("public".into(), "orders".into()), orders_scan);
        parents.insert(("public".into(), "organizations".into()), orgs_scan);

        let filter = col("org").eq(lit("acme"));
        let plan = build_column_resolution_plan(
            "public",
            "order_items",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap()
        .expect("expected Some(ResolutionPlan)");

        let rendered = format!("{}", plan.plan.display_indent());
        assert!(rendered.contains("__br_anchor_orders_"));
        assert!(rendered.contains("__br_anchor_organizations_"));
        // Two inner joins for a 2-hop chain.
        let join_count = rendered.matches("Inner Join").count();
        assert!(
            join_count >= 2,
            "expected ≥2 joins, got {join_count}: {rendered}"
        );
    }

    #[test]
    fn mixed_target_and_parent_refs_handled() {
        let e = edge("public", "payments", "order_id", "public", "orders", "id");
        let snap = snap_with(
            &[e],
            &[("public", "payments", "org")],
            &[
                ("public", "payments", &["id", "order_id", "status"]),
                ("public", "orders", &["id", "org"]),
            ],
        );

        let target_scan = scan("public.payments", &["id", "order_id", "status"]);
        let parent_scan = scan("public.orders", &["id", "org"]);
        let mut parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        parents.insert(("public".into(), "orders".into()), parent_scan);

        // status is on target; org is on parent → mixed filter must work.
        let filter = col("status")
            .eq(lit("paid"))
            .and(col("org").eq(lit("acme")));
        let plan = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap()
        .expect("expected Some(ResolutionPlan)");

        let rendered = format!("{}", plan.plan.display_indent());
        assert!(rendered.contains("Inner Join"));
        assert!(rendered.contains("status"));
        assert!(rendered.contains("__br_anchor_orders_"));
    }

    #[test]
    fn no_anchor_returns_error() {
        let snap = snap_with(
            &[],
            &[],
            &[("public", "payments", &["id", "order_id"])], // no anchor for "org"
        );
        let target_scan = scan("public.payments", &["id", "order_id"]);
        let parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        let filter = col("org").eq(lit("acme"));
        let err = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .expect_err("expected error, got Ok");
        assert!(
            matches!(err, ResolutionError::NoColumnAnchor { .. }),
            "{err}"
        );
    }

    #[test]
    fn depth_limit_exceeded_when_chain_too_long() {
        // Build a chain of MAX_DEPTH + 1 hops where only the final table has the column.
        let mut edges = Vec::new();
        let mut anchors: Vec<(String, String, String)> = Vec::new();
        let mut cols: Vec<(String, String, Vec<String>)> = Vec::new();
        let mut prev = "payments".to_string();
        for i in 0..=MAX_DEPTH {
            let next = format!("hop{i}");
            edges.push(RelationshipEdge {
                id: relationship_id("public", &prev, "fk", "public", &next, "id"),
                child_schema: "public".into(),
                child_table: prev.clone(),
                child_column: "fk".into(),
                parent_schema: "public".into(),
                parent_table: next.clone(),
                parent_column: "id".into(),
            });
            anchors.push(("public".into(), prev.clone(), "org".into()));
            cols.push(("public".into(), prev.clone(), vec!["fk".into()]));
            prev = next;
        }
        // Final table has "org".
        cols.push((
            "public".into(),
            prev.clone(),
            vec!["id".into(), "org".into()],
        ));

        let mut snap = RelationshipSnapshot::default();
        for e in edges {
            snap.relationships.insert(e.id, e);
        }
        for (s, t, c) in anchors {
            let eid = snap
                .relationships
                .values()
                .find(|e| e.child_schema == s && e.child_table == t)
                .unwrap()
                .id;
            snap.anchors
                .insert((s, t, c), AnchorShape::Relationship(eid));
        }
        for (s, t, c) in cols {
            snap.columns_by_table
                .insert((s, t), c.into_iter().collect());
        }

        let target_scan = scan("public.payments", &["fk"]);
        let parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        let filter = col("org").eq(lit("acme"));
        let err = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .expect_err("expected DepthLimitExceeded");
        assert!(
            matches!(err, ResolutionError::DepthLimitExceeded { .. }),
            "{err}"
        );
    }

    #[test]
    fn cycle_detected() {
        // a → b → a cycle; neither carries "org".
        let e1 = edge("public", "a", "fk", "public", "b", "id");
        let e2 = edge("public", "b", "fk", "public", "a", "id");
        let snap = snap_with(
            &[e1, e2],
            &[("public", "a", "org"), ("public", "b", "org")],
            &[("public", "a", &["fk"]), ("public", "b", &["fk"])],
        );
        let target_scan = scan("public.a", &["fk"]);
        let parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        let filter = col("org").eq(lit("X"));
        let err =
            build_column_resolution_plan("public", "a", target_scan, &filter, &snap, &parents)
                .expect_err("expected ChainCycle");
        assert!(matches!(err, ResolutionError::ChainCycle { .. }), "{err}");
    }

    #[test]
    fn qualified_parent_ref_rejected() {
        let e = edge("public", "payments", "order_id", "public", "orders", "id");
        let snap = snap_with(
            &[e],
            &[("public", "payments", "org")],
            &[
                ("public", "payments", &["id"]),
                ("public", "orders", &["id", "org"]),
            ],
        );
        let target_scan = scan("public.payments", &["id"]);
        let parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        // Qualified reference to a parent table.
        let filter = Expr::Column(Column::new(
            Some(TableReference::bare("orders".to_string())),
            "org".to_string(),
        ))
        .eq(lit("acme"));
        let err = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .expect_err("expected QualifiedParentRefNotSupported");
        assert!(
            matches!(err, ResolutionError::QualifiedParentRefNotSupported(_)),
            "{err}"
        );
    }

    #[test]
    fn projection_preserves_target_schema_only() {
        // Single-hop: ensure the final plan's output schema exposes only the
        // target's columns — no parent columns leak through, even though
        // they're present internally for the join/filter evaluation.
        let e = edge("public", "payments", "order_id", "public", "orders", "id");
        let snap = snap_with(
            &[e],
            &[("public", "payments", "org")],
            &[
                ("public", "payments", &["id", "order_id", "amount"]),
                ("public", "orders", &["id", "org", "created_at"]),
            ],
        );
        let target_scan = scan("public.payments", &["id", "order_id", "amount"]);
        let parent_scan = scan("public.orders", &["id", "org", "created_at"]);
        let mut parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        parents.insert(("public".into(), "orders".into()), parent_scan);
        let filter = col("org").eq(lit("acme"));
        let plan = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap()
        .expect("Some");

        // Schema-level assertion: the plan's output schema is exactly the
        // target's scanned fields. Any regression that reintroduces parent
        // columns into the projection — or drops target columns — fails here
        // regardless of how the plan renders.
        let field_names: HashSet<String> = plan
            .plan
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        let expected: HashSet<String> = ["id", "order_id", "amount"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            field_names, expected,
            "plan schema must match target's scanned columns exactly"
        );

        // Qualifier-level assertion: no field in the output schema is
        // qualified with a parent-table reference. Defends against a
        // regression where the Projection builds correctly but emits
        // parent-aliased columns (e.g. via the terminal alias qualifier).
        for (qualifier, _field) in plan.plan.schema().iter() {
            if let Some(q) = qualifier {
                assert!(
                    q.table() != "orders" && !q.table().starts_with(ALIAS_PREFIX),
                    "parent qualifier leaked into output schema: {q:?}"
                );
            }
        }

        // And the parent column itself ("org") must not appear as an output
        // field — the row filter consumed it internally but the downstream
        // schema hides it.
        assert!(
            !field_names.contains("org"),
            "parent column 'org' leaked into output schema"
        );
    }

    #[test]
    fn parents_needed_for_is_empty_on_fast_path() {
        let snap = snap_with(&[], &[], &[("public", "t", &["x"])]);
        let filter = col("x").eq(lit("1"));
        let parents = snap.parents_needed_for("public", "t", &filter).unwrap();
        assert!(parents.is_empty());
    }

    #[test]
    fn partial_target_projection_emits_only_scanned_columns() {
        // Target is discovered with ["id", "order_id", "amount"] in the
        // catalog, but the caller's target_scan exposes only ["id", "amount"]
        // (equivalent to `SELECT id, amount FROM payments`). The resolver's
        // top Projection must emit exactly those two columns — projecting
        // "order_id" would error "field not found" at plan validation.
        let e = edge("public", "payments", "order_id", "public", "orders", "id");
        let snap = snap_with(
            &[e],
            &[("public", "payments", "org")],
            &[
                ("public", "payments", &["id", "order_id", "amount"]),
                ("public", "orders", &["id", "org"]),
            ],
        );
        let target_scan = scan("public.payments", &["id", "amount"]);
        let parent_scan = scan("public.orders", &["id", "org"]);
        let mut parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        parents.insert(("public".into(), "orders".into()), parent_scan);
        let filter = col("org").eq(lit("acme"));
        let plan = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap()
        .expect("Some");

        // The plan's output schema must match the scan's output: exactly
        // ["id", "amount"], no "order_id" leaking through from catalog data.
        let field_names: Vec<String> = plan
            .plan
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        assert_eq!(
            field_names,
            vec!["id".to_string(), "amount".to_string()],
            "top projection should emit only scanned columns"
        );
    }

    #[test]
    fn expr_column_names_extracts_unqualified_refs() {
        let expr = col("org")
            .eq(lit("acme"))
            .and(col("status").eq(lit("paid")));
        let names = expr_column_names(&expr).unwrap();
        let expected: HashSet<String> = ["org", "status"].iter().map(|s| s.to_string()).collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn expr_column_names_empty_for_pure_literal() {
        let expr = lit(true);
        let names = expr_column_names(&expr).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn alias_anchor_rewrites_filter_column_name() {
        // Policy says `tenant_id = $1` but the table has it under `org_id`.
        // An alias anchor translates `tenant_id` → `org_id` in the filter
        // expression; no join is built.
        let snap = snap_with_aliases(
            &[],
            &[],
            &[("public", "customers", "tenant_id", "org_id")],
            &[("public", "customers", &["id", "name", "org_id"])],
        );
        let target_scan = scan("public.customers", &["id", "name", "org_id"]);
        let parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        let filter = col("tenant_id").eq(lit("acme"));
        let plan = build_column_resolution_plan(
            "public",
            "customers",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap()
        .expect("expected Some(ResolutionPlan) for alias");
        let rendered = format!("{}", plan.plan.display_indent());
        // No join, the filter references org_id (the aliased column).
        assert!(
            !rendered.contains("Inner Join"),
            "alias rewrite should not produce a join: {rendered}"
        );
        assert!(
            rendered.contains("Filter"),
            "alias rewrite should include a Filter: {rendered}"
        );
        assert!(
            rendered.contains("org_id"),
            "filter should reference actual column org_id: {rendered}"
        );
        assert!(
            !rendered.contains("tenant_id"),
            "filter should not still reference resolved name tenant_id: {rendered}"
        );
    }

    #[test]
    fn alias_anchor_contributes_no_parent_scans() {
        let snap = snap_with_aliases(
            &[],
            &[],
            &[("public", "customers", "tenant_id", "org_id")],
            &[("public", "customers", &["id", "org_id"])],
        );
        let filter = col("tenant_id").eq(lit("acme"));
        let parents = snap
            .parents_needed_for("public", "customers", &filter)
            .unwrap();
        assert!(
            parents.is_empty(),
            "alias anchor should need no parent scans, got {parents:?}"
        );
    }

    #[test]
    fn mixed_alias_and_fk_walk_in_one_filter() {
        // `tenant_id` is an alias for `org_id` on the target.
        // `region` lives on a parent table reached via an FK anchor.
        let e = edge(
            "public",
            "customers",
            "account_id",
            "public",
            "accounts",
            "id",
        );
        let snap = snap_with_aliases(
            &[e],
            &[("public", "customers", "region")],
            &[("public", "customers", "tenant_id", "org_id")],
            &[
                ("public", "customers", &["id", "org_id", "account_id"]),
                ("public", "accounts", &["id", "region"]),
            ],
        );
        let target_scan = scan("public.customers", &["id", "org_id", "account_id"]);
        let parent_scan = scan("public.accounts", &["id", "region"]);
        let mut parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        parents.insert(("public".into(), "accounts".into()), parent_scan);

        let filter = col("tenant_id")
            .eq(lit("acme"))
            .and(col("region").eq(lit("us-east")));
        let plan = build_column_resolution_plan(
            "public",
            "customers",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap()
        .expect("expected Some(ResolutionPlan)");

        let rendered = format!("{}", plan.plan.display_indent());
        // One inner join for the FK-walk anchor (region → accounts.region).
        assert!(
            rendered.contains("Inner Join"),
            "FK-walk anchor must contribute a join: {rendered}"
        );
        // Alias substitution happened: filter uses org_id, not tenant_id.
        assert!(
            rendered.contains("org_id"),
            "filter must reference aliased column org_id: {rendered}"
        );
        assert!(
            !rendered.contains("tenant_id"),
            "filter must not reference resolved-name tenant_id: {rendered}"
        );
        // Output schema stays strictly the target's columns.
        let field_names: HashSet<String> = plan
            .plan
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        let expected: HashSet<String> = ["id", "org_id", "account_id"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            field_names, expected,
            "top projection must match target scan"
        );
    }

    #[test]
    fn alias_preferred_over_fk_walk_when_both_keys_would_match() {
        // Sanity on the enum discriminator: if an alias anchor is the one
        // stored under (schema, table, col), `resolve_column` returns the
        // alias branch regardless of what FK anchors happen to exist for
        // other columns on the same table.
        let e = edge(
            "public",
            "customers",
            "account_id",
            "public",
            "accounts",
            "id",
        );
        let snap = snap_with_aliases(
            &[e],
            &[("public", "customers", "region")], // FK anchor for a DIFFERENT column
            &[("public", "customers", "tenant_id", "org_id")], // alias for tenant_id
            &[
                ("public", "customers", &["id", "org_id", "account_id"]),
                ("public", "accounts", &["id", "region"]),
            ],
        );

        let mut counter = 0;
        let resolved =
            resolve_column("public", "customers", "tenant_id", &snap, &mut counter).unwrap();
        match resolved {
            ResolvedColumn::Alias(actual) => assert_eq!(actual, "org_id"),
            ResolvedColumn::Hops(_) => panic!("expected Alias, got Hops"),
        }
    }

    #[test]
    fn zero_column_target_scan_resolves_without_error() {
        // DF 52+ plans `SELECT COUNT(*) FROM payments WHERE org = 'acme'` as
        // Aggregate → TableScan(projection=Some([])) — a zero-column scan.
        // The resolver must not produce an invalid zero-output Projection;
        // it should fall through to Filter → Join and rely on downstream
        // aggregation to handle the empty schema.
        let e = edge("public", "payments", "order_id", "public", "orders", "id");
        let snap = snap_with(
            &[e],
            &[("public", "payments", "org")],
            &[
                ("public", "payments", &["id", "order_id"]),
                ("public", "orders", &["id", "org"]),
            ],
        );
        let target_scan = scan("public.payments", &[]);
        let parent_scan = scan("public.orders", &["id", "org"]);
        let mut parents: HashMap<(String, String), LogicalPlan> = HashMap::new();
        parents.insert(("public".into(), "orders".into()), parent_scan);
        let filter = col("org").eq(lit("acme"));
        let plan = build_column_resolution_plan(
            "public",
            "payments",
            target_scan,
            &filter,
            &snap,
            &parents,
        )
        .unwrap()
        .expect("Some");

        let rendered = format!("{}", plan.plan.display_indent());
        assert!(
            rendered.contains("Inner Join"),
            "missing inner join: {rendered}"
        );
        assert!(rendered.contains("Filter"), "missing filter: {rendered}");
    }
}
