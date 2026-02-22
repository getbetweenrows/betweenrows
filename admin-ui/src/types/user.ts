export interface User {
  id: string
  username: string
  tenant: string
  is_admin: boolean
  is_active: boolean
  email: string | null
  display_name: string | null
  last_login_at: string | null
  created_at: string
  updated_at: string
}

export interface PaginatedResponse<T> {
  data: T[]
  total: number
  page: number
  page_size: number
}

export interface LoginResponse {
  token: string
  user: User
}

export interface CreateUserPayload {
  username: string
  password: string
  tenant: string
  is_admin: boolean
  email?: string
  display_name?: string
}

export interface UpdateUserPayload {
  tenant?: string
  is_admin?: boolean
  is_active?: boolean
  email?: string
  display_name?: string
}
