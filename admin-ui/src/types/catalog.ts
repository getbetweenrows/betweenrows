// ---------- Discovery response types ----------

export interface DiscoveredSchemaResponse {
  schema_name: string
  schema_alias: string | null
  is_already_selected: boolean
}

export interface DiscoveredTableResponse {
  schema_name: string
  table_name: string
  table_type: string
  is_already_selected: boolean
}

export interface DiscoveredColumnResponse {
  schema_name: string
  table_name: string
  column_name: string
  ordinal_position: number
  data_type: string
  is_nullable: boolean
  column_default: string | null
  arrow_type: string | null
  is_already_selected: boolean
}

// ---------- Stored catalog types ----------

export interface CatalogColumnResponse {
  id: string
  column_name: string
  ordinal_position: number
  data_type: string
  is_nullable: boolean
  column_default: string | null
  arrow_type: string | null
  is_selected: boolean
}

export interface CatalogTableResponse {
  id: string
  table_name: string
  table_type: string
  is_selected: boolean
  columns: CatalogColumnResponse[]
}

export interface CatalogSchemaResponse {
  id: string
  schema_name: string
  schema_alias: string | null
  is_selected: boolean
  tables: CatalogTableResponse[]
}

export interface CatalogResponse {
  schemas: CatalogSchemaResponse[]
}

// ---------- Save catalog request types ----------

export interface SaveCatalogColumnSelection {
  column_name: string
  is_selected: boolean
}

export interface SaveCatalogTableSelection {
  table_name: string
  table_type: string
  is_selected: boolean
  columns?: SaveCatalogColumnSelection[]
}

export interface SaveCatalogSchemaSelection {
  schema_name: string
  is_selected: boolean
  tables: SaveCatalogTableSelection[]
}

// ---------- Discovery job types ----------

export type DiscoveryAction =
  | 'discover_schemas'
  | 'discover_tables'
  | 'discover_columns'
  | 'save_catalog'
  | 'sync_catalog'

export interface TableRef {
  schema: string
  table: string
}

export interface CatalogColumnSelection {
  column_name: string
  is_selected: boolean
}

export interface CatalogTableSelection {
  table_name: string
  table_type: string
  is_selected: boolean
  columns?: CatalogColumnSelection[]
}

export interface CatalogSchemaSelection {
  schema_name: string
  schema_alias?: string | null
  is_selected: boolean
  tables: CatalogTableSelection[]
}

export type DiscoveryRequest =
  | { action: 'discover_schemas' }
  | { action: 'discover_tables'; schemas: string[] }
  | { action: 'discover_columns'; tables: TableRef[] }
  | { action: 'save_catalog'; schemas: CatalogSchemaSelection[] }
  | { action: 'sync_catalog' }

export interface SubmitDiscoveryResponse {
  job_id: string
}

export interface JobStatusResponse {
  job_id: string
  action: string
  status: 'running' | 'completed' | 'failed'
  result: unknown | null
  error: string | null
}

export type DiscoveryEventType =
  | { type: 'progress'; phase: string; detail: string }
  | { type: 'result'; data: unknown }
  | { type: 'error'; message: string }
  | { type: 'cancelled' }
  | { type: 'done' }

// ---------- Drift report types ----------

export type DriftStatus = 'unchanged' | 'new' | 'deleted' | 'modified'

export interface ColumnChanges {
  old_type?: string
  new_type?: string
}

export interface ColumnDrift {
  column_name: string
  status: DriftStatus
  changes: ColumnChanges | null
}

export interface TableDrift {
  table_name: string
  status: DriftStatus
  columns: ColumnDrift[]
}

export interface SchemaDrift {
  schema_name: string
  status: DriftStatus
  tables: TableDrift[]
}

export interface DriftReport {
  schemas: SchemaDrift[]
  has_breaking_changes: boolean
}

// ---------- Relationships + column anchors ----------

export interface TableRelationship {
  id: string
  data_source_id: string
  child_table_id: string
  child_table_name: string
  child_schema_name: string
  child_column_name: string
  parent_table_id: string
  parent_table_name: string
  parent_schema_name: string
  parent_column_name: string
  created_at: string
  created_by: string | null
}

export interface CreateTableRelationshipRequest {
  child_table_id: string
  child_column_name: string
  parent_table_id: string
  parent_column_name: string
}

/// Column anchors come in two shapes (XOR):
///   - FK walk: `relationship_id` is set, `actual_column_name` is null
///   - Same-table alias: `actual_column_name` is set, `relationship_id` is null
export interface ColumnAnchor {
  id: string
  data_source_id: string
  child_table_id: string
  child_table_name: string
  resolved_column_name: string
  relationship_id: string | null
  actual_column_name: string | null
  designated_at: string
  designated_by: string
}

/// Exactly one of `relationship_id` or `actual_column_name` must be set.
/// The server validates this with a 422.
export interface CreateColumnAnchorRequest {
  child_table_id: string
  resolved_column_name: string
  relationship_id?: string
  actual_column_name?: string
}

/// Suggestions returned by live `pg_constraint` introspection. The
/// `child_table_id` / `parent_table_id` have already been resolved to the
/// admin's discovered catalog so the UI can promote them in one POST.
export interface FkSuggestion {
  child_table_id: string
  child_schema_name: string
  child_table_name: string
  child_column_name: string
  parent_table_id: string
  parent_schema_name: string
  parent_table_name: string
  parent_column_name: string
  fk_constraint_name: string
  already_added: boolean
}
