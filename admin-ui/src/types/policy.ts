export interface ObligationRequest {
  obligation_type: string
  definition: Record<string, unknown>
}

export interface ObligationResponse {
  id: string
  obligation_type: string
  definition: Record<string, unknown>
  created_at: string
  updated_at: string
}

export interface PolicyAssignmentResponse {
  id: string
  policy_id: string
  policy_name: string
  data_source_id: string
  datasource_name: string
  user_id: string | null
  username: string | null
  priority: number
  created_at: string
}

export interface PolicyResponse {
  id: string
  name: string
  description: string | null
  effect: 'permit' | 'deny'
  is_enabled: boolean
  version: number
  obligation_count: number
  assignment_count: number
  obligations?: ObligationResponse[]
  assignments?: PolicyAssignmentResponse[]
  created_at: string
  updated_at: string
}

export interface CreatePolicyPayload {
  name: string
  description?: string
  effect: 'permit' | 'deny'
  is_enabled: boolean
  obligations: ObligationRequest[]
}

export interface UpdatePolicyPayload {
  name?: string
  description?: string
  effect?: 'permit' | 'deny'
  is_enabled?: boolean
  version: number
  obligations?: ObligationRequest[]
}

export interface AssignPolicyPayload {
  policy_id: string
  user_id?: string | null
  priority: number
}
