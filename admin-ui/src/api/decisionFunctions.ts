import { client } from './client'
import type {
  DecisionFunctionResponse,
  CreateDecisionFunctionPayload,
  UpdateDecisionFunctionPayload,
  TestDecisionFnPayload,
  TestDecisionFnResponse,
} from '../types/decisionFunction'

export async function createDecisionFunction(
  payload: CreateDecisionFunctionPayload,
): Promise<DecisionFunctionResponse> {
  const { data } = await client.post<DecisionFunctionResponse>('/decision-functions', payload)
  return data
}

export async function getDecisionFunction(id: string): Promise<DecisionFunctionResponse> {
  const { data } = await client.get<DecisionFunctionResponse>(`/decision-functions/${id}`)
  return data
}

export async function updateDecisionFunction(
  id: string,
  payload: UpdateDecisionFunctionPayload,
): Promise<DecisionFunctionResponse> {
  const { data } = await client.put<DecisionFunctionResponse>(`/decision-functions/${id}`, payload)
  return data
}

export async function deleteDecisionFunction(id: string): Promise<void> {
  await client.delete(`/decision-functions/${id}`)
}

export async function testDecisionFn(
  payload: TestDecisionFnPayload,
): Promise<TestDecisionFnResponse> {
  const { data } = await client.post<TestDecisionFnResponse>('/decision-functions/test', payload)
  return data
}
