import { listUsers } from '../api/users'
import { listDataSources } from '../api/datasources'
import { listRoles } from '../api/roles'
import { listPolicies } from '../api/policies'

export interface EntityOption {
  id: string
  label: string
}

export async function searchUsers(query: string): Promise<EntityOption[]> {
  const res = await listUsers({ search: query || undefined, page: 1, page_size: 20 })
  return res.data.map((u) => ({ id: u.id, label: u.username }))
}

export async function searchDataSources(query: string): Promise<EntityOption[]> {
  const res = await listDataSources({ search: query || undefined, page: 1, page_size: 20 })
  return res.data.map((ds) => ({ id: ds.id, label: ds.name }))
}

export async function searchRoles(query: string): Promise<EntityOption[]> {
  const res = await listRoles({ search: query || undefined, page: 1, page_size: 20 })
  return res.data.map((r) => ({ id: r.id, label: r.name }))
}

export async function searchPolicies(query: string): Promise<EntityOption[]> {
  const res = await listPolicies({ search: query || undefined, page: 1, page_size: 20 })
  return res.data.map((p) => ({ id: p.id, label: p.name }))
}
