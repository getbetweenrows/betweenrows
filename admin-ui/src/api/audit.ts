import { client } from './client'
import type { PaginatedResponse } from '../types/user'

export interface AuditLogEntry {
  id: string
  user_id: string
  username: string
  data_source_id: string
  datasource_name: string
  original_query: string
  rewritten_query: string | null
  policies_applied: Array<{
    policy_id: string
    version: number
    name: string
    decision?: {
      result?: { fire: boolean; fuel_consumed?: number; time_us?: number }
      logs?: string[]
      fuel_consumed?: number
      time_us?: number
      error?: string | null
    }
  }>
  execution_time_ms: number | null
  client_ip: string | null
  client_info: string | null
  created_at: string
  status: 'success' | 'error' | 'denied'
  error_message: string | null
}

export async function listAuditLogs(params?: {
  page?: number
  page_size?: number
  user_id?: string
  datasource_id?: string
  from?: string
  to?: string
  status?: string
}): Promise<PaginatedResponse<AuditLogEntry>> {
  const { data } = await client.get<PaginatedResponse<AuditLogEntry>>('/audit/queries', { params })
  return data
}
