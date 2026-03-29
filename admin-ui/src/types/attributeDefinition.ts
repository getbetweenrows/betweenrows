export type ValueType = 'string' | 'integer' | 'boolean' | 'list'
export type EntityType = 'user' | 'table' | 'column'

export interface AttributeDefinition {
  id: string
  key: string
  entity_type: EntityType
  display_name: string
  value_type: ValueType
  default_value: string | null
  allowed_values: string[] | null
  description: string | null
  created_by: string
  updated_by: string
  created_at: string
  updated_at: string
}

export interface CreateAttributeDefinitionPayload {
  key: string
  entity_type: EntityType
  display_name: string
  value_type: ValueType
  default_value?: string
  allowed_values?: string[]
  description?: string
}

export interface UpdateAttributeDefinitionPayload {
  display_name?: string
  value_type?: ValueType
  default_value?: string | null
  allowed_values?: string[] | null
  description?: string | null
}
