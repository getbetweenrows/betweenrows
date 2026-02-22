export interface FieldDef {
  key: string
  label: string
  field_type: 'text' | 'number' | 'select' | 'textarea'
  options?: string[]
  required: boolean
  is_secret: boolean
  default_value?: string
}

export interface DataSourceType {
  ds_type: string
  label: string
  fields: FieldDef[]
}

export interface DataSource {
  id: string
  last_sync_at?: string | null
  last_sync_result?: string | null
  name: string
  ds_type: string
  /** Non-secret config only. Secret fields (e.g. password) are never returned. */
  config: Record<string, unknown>
  is_active: boolean
  created_at: string
  updated_at: string
}

export interface CreateDataSourcePayload {
  name: string
  ds_type: string
  /** Flat object containing all fields (secret + non-secret). Backend splits them. */
  config: Record<string, unknown>
}

export interface UpdateDataSourcePayload {
  name?: string
  is_active?: boolean
  /** Partial config update. Absent fields preserved. Empty string = keep secret. */
  config?: Record<string, unknown>
}

export interface TestConnectionResponse {
  success: boolean
  message?: string
}
