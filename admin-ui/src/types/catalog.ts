// ---------- Discovery response types ----------

export interface DiscoveredSchemaResponse {
  schema_name: string
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
  is_selected: boolean
  tables: CatalogTableResponse[]
}

export interface CatalogResponse {
  schemas: CatalogSchemaResponse[]
}

// ---------- Save catalog request types ----------

export interface SaveCatalogTableSelection {
  table_name: string
  table_type: string
  is_selected: boolean
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

export interface CatalogTableSelection {
  table_name: string
  table_type: string
  is_selected: boolean
}

export interface CatalogSchemaSelection {
  schema_name: string
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
