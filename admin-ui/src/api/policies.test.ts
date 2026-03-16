import { describe, it, expect, vi, beforeEach } from 'vitest'
import { makePolicy, makePolicyAssignment } from '../test/factories'

vi.mock('./client', () => ({
  client: {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
  },
}))

import { client } from './client'
import {
  listPolicies,
  getPolicy,
  createPolicy,
  updatePolicy,
  deletePolicy,
  listDatasourcePolicies,
  assignPolicy,
  removeAssignment,
} from './policies'

const mockClient = client as unknown as {
  get: ReturnType<typeof vi.fn>
  post: ReturnType<typeof vi.fn>
  put: ReturnType<typeof vi.fn>
  delete: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('listPolicies', () => {
  it('GETs /policies with no params', async () => {
    const page = { data: [makePolicy()], total: 1, page: 1, page_size: 20 }
    mockClient.get.mockResolvedValue({ data: page })
    const result = await listPolicies()
    expect(mockClient.get).toHaveBeenCalledWith('/policies', { params: undefined })
    expect(result).toEqual(page)
  })

  it('passes search + pagination params', async () => {
    const page = { data: [], total: 0, page: 2, page_size: 10 }
    mockClient.get.mockResolvedValue({ data: page })
    await listPolicies({ page: 2, page_size: 10, search: 'row' })
    expect(mockClient.get).toHaveBeenCalledWith('/policies', {
      params: { page: 2, page_size: 10, search: 'row' },
    })
  })
})

describe('getPolicy', () => {
  it('GETs /policies/:id', async () => {
    const policy = makePolicy({ id: 'p-1' })
    mockClient.get.mockResolvedValue({ data: policy })
    const result = await getPolicy('p-1')
    expect(mockClient.get).toHaveBeenCalledWith('/policies/p-1')
    expect(result).toEqual(policy)
  })
})

describe('createPolicy', () => {
  it('POSTs payload to /policies', async () => {
    const policy = makePolicy()
    mockClient.post.mockResolvedValue({ data: policy })
    const payload = {
      name: 'my-policy',
      policy_type: 'row_filter' as const,
      is_enabled: true,
      targets: [{ schemas: ['public'], tables: ['orders'] }],
      definition: null,
    }
    const result = await createPolicy(payload)
    expect(mockClient.post).toHaveBeenCalledWith('/policies', payload)
    expect(result).toEqual(policy)
  })
})

describe('updatePolicy', () => {
  it('PUTs payload to /policies/:id', async () => {
    const policy = makePolicy({ version: 2 })
    mockClient.put.mockResolvedValue({ data: policy })
    const payload = { version: 1, name: 'updated' }
    const result = await updatePolicy('p-1', payload)
    expect(mockClient.put).toHaveBeenCalledWith('/policies/p-1', payload)
    expect(result).toEqual(policy)
  })
})

describe('deletePolicy', () => {
  it('DELETEs /policies/:id', async () => {
    mockClient.delete.mockResolvedValue({})
    await deletePolicy('p-1')
    expect(mockClient.delete).toHaveBeenCalledWith('/policies/p-1')
  })
})

describe('listDatasourcePolicies', () => {
  it('GETs /datasources/:id/policies', async () => {
    const assignments = [makePolicyAssignment()]
    mockClient.get.mockResolvedValue({ data: assignments })
    const result = await listDatasourcePolicies('ds-1')
    expect(mockClient.get).toHaveBeenCalledWith('/datasources/ds-1/policies')
    expect(result).toEqual(assignments)
  })
})

describe('assignPolicy', () => {
  it('POSTs to /datasources/:id/policies', async () => {
    const assignment = makePolicyAssignment()
    mockClient.post.mockResolvedValue({ data: assignment })
    const payload = { policy_id: 'p-1', user_id: null, priority: 100 }
    const result = await assignPolicy('ds-1', payload)
    expect(mockClient.post).toHaveBeenCalledWith('/datasources/ds-1/policies', payload)
    expect(result).toEqual(assignment)
  })
})

describe('removeAssignment', () => {
  it('DELETEs /datasources/:id/policies/:assignment_id', async () => {
    mockClient.delete.mockResolvedValue({})
    await removeAssignment('ds-1', 'a-1')
    expect(mockClient.delete).toHaveBeenCalledWith('/datasources/ds-1/policies/a-1')
  })
})
