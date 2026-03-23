import { client } from './client'
import type {
  Role,
  RoleDetail,
  EffectiveMember,
  ImpactResponse,
  CreateRolePayload,
  UpdateRolePayload,
} from '../types/role'
import type { PaginatedResponse } from '../types/user'

export async function listRoles(params?: {
  page?: number
  page_size?: number
  search?: string
}): Promise<PaginatedResponse<Role>> {
  const { data } = await client.get<PaginatedResponse<Role>>('/roles', { params })
  return data
}

export async function getRole(id: string): Promise<RoleDetail> {
  const { data } = await client.get<RoleDetail>(`/roles/${id}`)
  return data
}

export async function createRole(payload: CreateRolePayload): Promise<Role> {
  const { data } = await client.post<Role>('/roles', payload)
  return data
}

export async function updateRole(id: string, payload: UpdateRolePayload): Promise<Role> {
  const { data } = await client.put<Role>(`/roles/${id}`, payload)
  return data
}

export async function deleteRole(id: string): Promise<void> {
  await client.delete(`/roles/${id}`)
}

export async function getEffectiveMembers(id: string): Promise<EffectiveMember[]> {
  const { data } = await client.get<EffectiveMember[]>(`/roles/${id}/effective-members`)
  return data
}

export async function getRoleImpact(id: string): Promise<ImpactResponse> {
  const { data } = await client.get<ImpactResponse>(`/roles/${id}/impact`)
  return data
}

export async function addMembers(roleId: string, userIds: string[]): Promise<void> {
  await client.post(`/roles/${roleId}/members`, { user_ids: userIds })
}

export async function removeMember(roleId: string, userId: string): Promise<void> {
  await client.delete(`/roles/${roleId}/members/${userId}`)
}

export async function addParent(roleId: string, parentRoleId: string): Promise<void> {
  await client.post(`/roles/${roleId}/parents`, { parent_role_id: parentRoleId })
}

export async function removeParent(roleId: string, parentId: string): Promise<void> {
  await client.delete(`/roles/${roleId}/parents/${parentId}`)
}

export async function getDatasourceRoles(
  dsId: string,
): Promise<{ id: string; name: string; is_active: boolean }[]> {
  const { data } = await client.get<{ id: string; name: string; is_active: boolean }[]>(
    `/datasources/${dsId}/access/roles`,
  )
  return data
}

export async function setDatasourceRoleAccess(
  dsId: string,
  roleIds: string[],
): Promise<void> {
  await client.put(`/datasources/${dsId}/access/roles`, { role_ids: roleIds })
}
