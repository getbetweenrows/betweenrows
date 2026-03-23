import { client } from './client'
import type { AdminAuditEntry } from '../types/role'
import type { PaginatedResponse } from '../types/user'

export async function listAdminAuditLogs(params?: {
  page?: number
  page_size?: number
  resource_type?: string
  resource_id?: string
  actor_id?: string
  from?: string
  to?: string
}): Promise<PaginatedResponse<AdminAuditEntry>> {
  const { data } = await client.get<PaginatedResponse<AdminAuditEntry>>('/audit/admin', {
    params,
  })
  return data
}
