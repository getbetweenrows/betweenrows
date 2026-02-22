import { client } from './client'
import type {
  CreateDataSourcePayload,
  DataSource,
  DataSourceType,
  TestConnectionResponse,
  UpdateDataSourcePayload,
} from '../types/datasource'
import type { PaginatedResponse, User } from '../types/user'

export async function getDataSourceTypes(): Promise<DataSourceType[]> {
  const { data } = await client.get<DataSourceType[]>('/datasource-types')
  return data
}

export async function listDataSources(params?: {
  page?: number
  page_size?: number
  search?: string
}): Promise<PaginatedResponse<DataSource>> {
  const { data } = await client.get<PaginatedResponse<DataSource>>('/datasources', { params })
  return data
}

export async function getDataSource(id: string): Promise<DataSource> {
  const { data } = await client.get<DataSource>(`/datasources/${id}`)
  return data
}

export async function createDataSource(payload: CreateDataSourcePayload): Promise<DataSource> {
  const { data } = await client.post<DataSource>('/datasources', payload)
  return data
}

export async function updateDataSource(
  id: string,
  payload: UpdateDataSourcePayload,
): Promise<DataSource> {
  const { data } = await client.put<DataSource>(`/datasources/${id}`, payload)
  return data
}

export async function deleteDataSource(id: string): Promise<void> {
  await client.delete(`/datasources/${id}`)
}

export async function testDataSource(id: string): Promise<TestConnectionResponse> {
  const { data } = await client.post<TestConnectionResponse>(`/datasources/${id}/test`)
  return data
}

export async function getDataSourceUsers(id: string): Promise<User[]> {
  const { data } = await client.get<User[]>(`/datasources/${id}/users`)
  return data
}

export async function setDataSourceUsers(id: string, user_ids: string[]): Promise<void> {
  await client.put(`/datasources/${id}/users`, { user_ids })
}
