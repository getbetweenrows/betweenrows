import { describe, it, expect, vi, beforeEach } from 'vitest'
import { makeUser, makeLoginResponse, makePaginatedUsers } from '../test/factories'

// Mock the axios client
vi.mock('./client', () => ({
  client: {
    post: vi.fn(),
    get: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
  },
}))

import { client } from './client'
import { login, getMe, listUsers, getUser, createUser, updateUser, changePassword, deleteUser } from './users'

const mockClient = client as unknown as {
  post: ReturnType<typeof vi.fn>
  get: ReturnType<typeof vi.fn>
  put: ReturnType<typeof vi.fn>
  delete: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('login', () => {
  it('POSTs credentials and returns LoginResponse', async () => {
    const loginRes = makeLoginResponse()
    mockClient.post.mockResolvedValue({ data: loginRes })
    const result = await login('admin', 'secret')
    expect(mockClient.post).toHaveBeenCalledWith('/auth/login', { username: 'admin', password: 'secret' })
    expect(result).toEqual(loginRes)
  })
})

describe('getMe', () => {
  it('GETs /auth/me and returns User', async () => {
    const user = makeUser()
    mockClient.get.mockResolvedValue({ data: user })
    const result = await getMe()
    expect(mockClient.get).toHaveBeenCalledWith('/auth/me')
    expect(result).toEqual(user)
  })
})

describe('listUsers', () => {
  it('GETs /users with no params', async () => {
    const page = makePaginatedUsers([makeUser()])
    mockClient.get.mockResolvedValue({ data: page })
    const result = await listUsers()
    expect(mockClient.get).toHaveBeenCalledWith('/users', { params: undefined })
    expect(result).toEqual(page)
  })

  it('passes search + pagination params', async () => {
    const page = makePaginatedUsers([])
    mockClient.get.mockResolvedValue({ data: page })
    await listUsers({ page: 2, page_size: 10, search: 'alice' })
    expect(mockClient.get).toHaveBeenCalledWith('/users', {
      params: { page: 2, page_size: 10, search: 'alice' },
    })
  })
})

describe('getUser', () => {
  it('GETs /users/:id', async () => {
    const user = makeUser({ id: 'u-1' })
    mockClient.get.mockResolvedValue({ data: user })
    const result = await getUser('u-1')
    expect(mockClient.get).toHaveBeenCalledWith('/users/u-1')
    expect(result).toEqual(user)
  })
})

describe('createUser', () => {
  it('POSTs payload to /users', async () => {
    const user = makeUser()
    mockClient.post.mockResolvedValue({ data: user })
    const payload = { username: 'newuser', password: 'pw', is_admin: false }
    const result = await createUser(payload)
    expect(mockClient.post).toHaveBeenCalledWith('/users', payload)
    expect(result).toEqual(user)
  })
})

describe('updateUser', () => {
  it('PUTs payload to /users/:id', async () => {
    const user = makeUser()
    mockClient.put.mockResolvedValue({ data: user })
    const payload = { is_admin: true }
    const result = await updateUser('u-1', payload)
    expect(mockClient.put).toHaveBeenCalledWith('/users/u-1', payload)
    expect(result).toEqual(user)
  })
})

describe('changePassword', () => {
  it('PUTs new password to /users/:id/password', async () => {
    const user = makeUser()
    mockClient.put.mockResolvedValue({ data: user })
    const result = await changePassword('u-1', 'newpw')
    expect(mockClient.put).toHaveBeenCalledWith('/users/u-1/password', { password: 'newpw' })
    expect(result).toEqual(user)
  })
})

describe('deleteUser', () => {
  it('DELETEs /users/:id', async () => {
    mockClient.delete.mockResolvedValue({})
    await deleteUser('u-1')
    expect(mockClient.delete).toHaveBeenCalledWith('/users/u-1')
  })
})
