import type { DecisionFunctionSummary } from './decisionFunction'

export type PolicyType = 'row_filter' | 'column_mask' | 'column_allow' | 'column_deny' | 'table_deny'

export type AssignmentScope = 'all' | 'user' | 'role'

export interface TargetEntry {
  schemas: string[]
  tables: string[]
  columns?: string[]
}

export interface PolicyAssignmentResponse {
  id: string
  policy_id: string
  policy_name: string
  data_source_id: string
  datasource_name: string
  user_id: string | null
  username: string | null
  role_id: string | null
  role_name: string | null
  assignment_scope: AssignmentScope
  priority: number
  created_at: string
}

export interface PolicyResponse {
  id: string
  name: string
  description: string | null
  policy_type: string
  targets: TargetEntry[]
  definition: Record<string, string> | null
  is_enabled: boolean
  version: number
  decision_function_id?: string | null
  decision_function?: DecisionFunctionSummary | null
  assignment_count: number
  created_by: string
  updated_by: string
  created_at: string
  updated_at: string
  assignments?: PolicyAssignmentResponse[]
}

export interface CreatePolicyPayload {
  name: string
  description?: string
  policy_type: PolicyType
  is_enabled: boolean
  targets: TargetEntry[]
  definition?: Record<string, string> | null
  decision_function_id?: string | null
}

export interface UpdatePolicyPayload {
  name?: string
  description?: string
  policy_type?: PolicyType
  is_enabled?: boolean
  targets?: TargetEntry[]
  definition?: Record<string, string> | null
  decision_function_id?: string | null
  version: number
}

export interface AssignPolicyPayload {
  policy_id: string
  user_id?: string | null
  role_id?: string | null
  scope?: AssignmentScope
  priority: number
}
