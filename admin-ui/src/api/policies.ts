import { client } from './client'
import type {
  PolicyResponse,
  PolicyAssignmentResponse,
  CreatePolicyPayload,
  UpdatePolicyPayload,
  AssignPolicyPayload,
  PolicyAnchorCoverageResponse,
} from '../types/policy'
import type { PaginatedResponse } from '../types/user'

export async function listPolicies(params?: {
  page?: number
  page_size?: number
  search?: string
}): Promise<PaginatedResponse<PolicyResponse>> {
  const { data } = await client.get<PaginatedResponse<PolicyResponse>>('/policies', { params })
  return data
}

export async function getPolicy(id: string): Promise<PolicyResponse> {
  const { data } = await client.get<PolicyResponse>(`/policies/${id}`)
  return data
}

export async function createPolicy(payload: CreatePolicyPayload): Promise<PolicyResponse> {
  const { data } = await client.post<PolicyResponse>('/policies', payload)
  return data
}

export async function updatePolicy(
  id: string,
  payload: UpdatePolicyPayload,
): Promise<PolicyResponse> {
  const { data } = await client.put<PolicyResponse>(`/policies/${id}`, payload)
  return data
}

export async function deletePolicy(id: string): Promise<void> {
  await client.delete(`/policies/${id}`)
}

export async function listDatasourcePolicies(
  datasourceId: string,
): Promise<PolicyAssignmentResponse[]> {
  const { data } = await client.get<PolicyAssignmentResponse[]>(
    `/datasources/${datasourceId}/policies`,
  )
  return data
}

export async function assignPolicy(
  datasourceId: string,
  payload: AssignPolicyPayload,
): Promise<PolicyAssignmentResponse> {
  const { data } = await client.post<PolicyAssignmentResponse>(
    `/datasources/${datasourceId}/policies`,
    payload,
  )
  return data
}

export async function removeAssignment(
  datasourceId: string,
  assignmentId: string,
): Promise<void> {
  await client.delete(`/datasources/${datasourceId}/policies/${assignmentId}`)
}

export async function validateExpression(
  expression: string,
  isMask: boolean,
): Promise<{ valid: boolean; error?: string }> {
  const { data } = await client.post<{ valid: boolean; error?: string }>(
    '/policies/validate-expression',
    { expression, is_mask: isMask },
  )
  return data
}

export async function getPolicyAnchorCoverage(
  id: string,
): Promise<PolicyAnchorCoverageResponse> {
  const { data } = await client.get<PolicyAnchorCoverageResponse>(
    `/policies/${id}/anchor-coverage`,
  )
  return data
}
