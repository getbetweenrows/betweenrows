import { client } from './client'
import type {
  CreateUserPayload,
  LoginResponse,
  PaginatedResponse,
  UpdateUserPayload,
  User,
} from '../types/user'

export async function login(username: string, password: string): Promise<LoginResponse> {
  const { data } = await client.post<LoginResponse>('/auth/login', { username, password })
  return data
}

export async function getMe(): Promise<User> {
  const { data } = await client.get<User>('/auth/me')
  return data
}

export async function listUsers(params?: {
  page?: number
  page_size?: number
  search?: string
}): Promise<PaginatedResponse<User>> {
  const { data } = await client.get<PaginatedResponse<User>>('/users', { params })
  return data
}

export async function getUser(id: string): Promise<User> {
  const { data } = await client.get<User>(`/users/${id}`)
  return data
}

export async function createUser(payload: CreateUserPayload): Promise<User> {
  const { data } = await client.post<User>('/users', payload)
  return data
}

export async function updateUser(id: string, payload: UpdateUserPayload): Promise<User> {
  const { data } = await client.put<User>(`/users/${id}`, payload)
  return data
}

export async function changePassword(id: string, password: string): Promise<User> {
  const { data } = await client.put<User>(`/users/${id}/password`, { password })
  return data
}

export async function deleteUser(id: string): Promise<void> {
  await client.delete(`/users/${id}`)
}
