import { client } from './client'
import type {
  AttributeDefinition,
  CreateAttributeDefinitionPayload,
  UpdateAttributeDefinitionPayload,
} from '../types/attributeDefinition'
import type { PaginatedResponse } from '../types/user'

export async function listAttributeDefinitions(params?: {
  entity_type?: string
  page?: number
  page_size?: number
}): Promise<PaginatedResponse<AttributeDefinition>> {
  const { data } = await client.get<PaginatedResponse<AttributeDefinition>>(
    '/attribute-definitions',
    { params },
  )
  return data
}

export async function getAttributeDefinition(id: string): Promise<AttributeDefinition> {
  const { data } = await client.get<AttributeDefinition>(`/attribute-definitions/${id}`)
  return data
}

export async function createAttributeDefinition(
  payload: CreateAttributeDefinitionPayload,
): Promise<AttributeDefinition> {
  const { data } = await client.post<AttributeDefinition>('/attribute-definitions', payload)
  return data
}

export async function updateAttributeDefinition(
  id: string,
  payload: UpdateAttributeDefinitionPayload,
): Promise<AttributeDefinition> {
  const { data } = await client.put<AttributeDefinition>(`/attribute-definitions/${id}`, payload)
  return data
}

export async function deleteAttributeDefinition(
  id: string,
  force = false,
): Promise<void> {
  await client.delete(`/attribute-definitions/${id}`, { params: force ? { force: true } : {} })
}
