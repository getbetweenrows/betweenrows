import { describe, it, expect, vi, beforeEach } from 'vitest'
import { makeDataSource, makeDataSourceType, makeUser, makePaginatedUsers } from '../test/factories'

vi.mock('./client', () => ({
  client: {
    post: vi.fn(),
    get: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
  },
}))

import { client } from './client'
import {
  getDataSourceTypes,
  listDataSources,
  getDataSource,
  createDataSource,
  updateDataSource,
  deleteDataSource,
  testDataSource,
  getDataSourceUsers,
  setDataSourceUsers,
} from './datasources'

const mockClient = client as {
  post: ReturnType<typeof vi.fn>
  get: ReturnType<typeof vi.fn>
  put: ReturnType<typeof vi.fn>
  delete: ReturnType<typeof vi.fn>
}

beforeEach(() => vi.clearAllMocks())

describe('getDataSourceTypes', () => {
  it('GETs /datasource-types', async () => {
    const types = [makeDataSourceType()]
    mockClient.get.mockResolvedValue({ data: types })
    const result = await getDataSourceTypes()
    expect(mockClient.get).toHaveBeenCalledWith('/datasource-types')
    expect(result).toEqual(types)
  })
})

describe('listDataSources', () => {
  it('GETs /datasources with params', async () => {
    const page = { data: [makeDataSource()], total: 1, page: 1, page_size: 20 }
    mockClient.get.mockResolvedValue({ data: page })
    const result = await listDataSources({ page: 1, page_size: 20, search: 'prod' })
    expect(mockClient.get).toHaveBeenCalledWith('/datasources', {
      params: { page: 1, page_size: 20, search: 'prod' },
    })
    expect(result).toEqual(page)
  })
})

describe('getDataSource', () => {
  it('GETs /datasources/:id', async () => {
    const ds = makeDataSource({ id: 'ds-1' })
    mockClient.get.mockResolvedValue({ data: ds })
    const result = await getDataSource('ds-1')
    expect(mockClient.get).toHaveBeenCalledWith('/datasources/ds-1')
    expect(result).toEqual(ds)
  })
})

describe('createDataSource', () => {
  it('POSTs to /datasources', async () => {
    const ds = makeDataSource()
    mockClient.post.mockResolvedValue({ data: ds })
    const payload = { name: 'prod', ds_type: 'postgres', config: { host: 'localhost' } }
    const result = await createDataSource(payload)
    expect(mockClient.post).toHaveBeenCalledWith('/datasources', payload)
    expect(result).toEqual(ds)
  })
})

describe('updateDataSource', () => {
  it('PUTs to /datasources/:id', async () => {
    const ds = makeDataSource()
    mockClient.put.mockResolvedValue({ data: ds })
    const payload = { name: 'prod-renamed', is_active: false }
    const result = await updateDataSource('ds-1', payload)
    expect(mockClient.put).toHaveBeenCalledWith('/datasources/ds-1', payload)
    expect(result).toEqual(ds)
  })
})

describe('deleteDataSource', () => {
  it('DELETEs /datasources/:id', async () => {
    mockClient.delete.mockResolvedValue({})
    await deleteDataSource('ds-1')
    expect(mockClient.delete).toHaveBeenCalledWith('/datasources/ds-1')
  })
})

describe('testDataSource', () => {
  it('POSTs to /datasources/:id/test', async () => {
    const testRes = { success: true, message: 'Connected' }
    mockClient.post.mockResolvedValue({ data: testRes })
    const result = await testDataSource('ds-1')
    expect(mockClient.post).toHaveBeenCalledWith('/datasources/ds-1/test')
    expect(result).toEqual(testRes)
  })
})

describe('getDataSourceUsers', () => {
  it('GETs /datasources/:id/users', async () => {
    const users = [makeUser()]
    mockClient.get.mockResolvedValue({ data: users })
    const result = await getDataSourceUsers('ds-1')
    expect(mockClient.get).toHaveBeenCalledWith('/datasources/ds-1/users')
    expect(result).toEqual(users)
  })
})

describe('setDataSourceUsers', () => {
  it('PUTs user_ids to /datasources/:id/users', async () => {
    mockClient.put.mockResolvedValue({})
    await setDataSourceUsers('ds-1', ['u-1', 'u-2'])
    expect(mockClient.put).toHaveBeenCalledWith('/datasources/ds-1/users', {
      user_ids: ['u-1', 'u-2'],
    })
  })
})

// suppress unused import warning
void makePaginatedUsers
