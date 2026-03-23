export interface Role {
  id: string
  name: string
  description: string | null
  is_active: boolean
  direct_member_count: number
  created_at: string
  updated_at: string
}

export interface RoleDatasourceAccess {
  datasource_id: string
  datasource_name: string
  source: string
}

export interface RoleDetail extends Role {
  effective_member_count: number
  members: RoleMember[]
  parent_roles: RoleRef[]
  child_roles: RoleRef[]
  policy_assignments: RolePolicyAssignment[]
  datasource_access: RoleDatasourceAccess[]
}

export interface RoleMember {
  id: string
  username: string
}

export interface RoleRef {
  id: string
  name: string
}

export interface RolePolicyAssignment {
  policy_name: string
  datasource_name: string
  source: string
  priority: number
}

export interface EffectiveMember {
  user_id: string
  username: string
  source: string
}

export interface ImpactResponse {
  affected_users: number
  affected_assignments: number
}

export interface CreateRolePayload {
  name: string
  description?: string
}

export interface UpdateRolePayload {
  name?: string
  description?: string
  is_active?: boolean
}

export interface AdminAuditEntry {
  id: string
  resource_type: string
  resource_id: string
  action: string
  actor_id: string
  changes: Record<string, unknown> | null
  created_at: string
}
