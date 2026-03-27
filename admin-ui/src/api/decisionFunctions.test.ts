import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  listDecisionFunctions,
  getDecisionFunction,
  createDecisionFunction,
  updateDecisionFunction,
  deleteDecisionFunction,
  testDecisionFn,
} from './decisionFunctions'
import { client } from './client'

vi.mock('./client', () => ({
  client: {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
  },
}))

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const mockClient = client as any as {
  get: ReturnType<typeof vi.fn>
  post: ReturnType<typeof vi.fn>
  put: ReturnType<typeof vi.fn>
  delete: ReturnType<typeof vi.fn>
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('listDecisionFunctions', () => {
  it('sends GET to /decision-functions with page_size', async () => {
    const items = [{ id: 'fn-1', name: 'func-1' }]
    mockClient.get.mockResolvedValue({ data: { data: items, total: 1, page: 1, page_size: 200 } })

    const result = await listDecisionFunctions()

    expect(mockClient.get).toHaveBeenCalledWith('/decision-functions', { params: { page_size: 200 } })
    expect(result).toEqual(items)
  })
})

describe('getDecisionFunction', () => {
  it('sends GET to /decision-functions/:id', async () => {
    const data = { id: 'fn-1', name: 'func-1' }
    mockClient.get.mockResolvedValue({ data })

    const result = await getDecisionFunction('fn-1')

    expect(mockClient.get).toHaveBeenCalledWith('/decision-functions/fn-1')
    expect(result).toEqual(data)
  })
})

describe('createDecisionFunction', () => {
  it('sends POST to /decision-functions', async () => {
    const payload = {
      name: 'new-fn',
      decision_fn: 'function evaluate() { return { fire: true }; }',
      evaluate_context: 'session' as const,
    }
    const data = { id: 'fn-new', ...payload }
    mockClient.post.mockResolvedValue({ data })

    const result = await createDecisionFunction(payload)

    expect(mockClient.post).toHaveBeenCalledWith('/decision-functions', payload)
    expect(result).toEqual(data)
  })
})

describe('updateDecisionFunction', () => {
  it('sends PUT to /decision-functions/:id', async () => {
    const payload = { decision_fn: 'updated code', version: 2 }
    const data = { id: 'fn-1', ...payload, version: 3 }
    mockClient.put.mockResolvedValue({ data })

    const result = await updateDecisionFunction('fn-1', payload)

    expect(mockClient.put).toHaveBeenCalledWith('/decision-functions/fn-1', payload)
    expect(result).toEqual(data)
  })
})

describe('deleteDecisionFunction', () => {
  it('sends DELETE to /decision-functions/:id', async () => {
    mockClient.delete.mockResolvedValue({})

    await deleteDecisionFunction('fn-1')

    expect(mockClient.delete).toHaveBeenCalledWith('/decision-functions/fn-1')
  })
})

describe('testDecisionFn', () => {
  it('sends POST to /decision-functions/test', async () => {
    const payload = {
      decision_fn: 'function evaluate() { return { fire: true }; }',
      context: { session: {} },
      config: {},
    }
    const data = { success: true, result: { fire: true } }
    mockClient.post.mockResolvedValue({ data })

    const result = await testDecisionFn(payload)

    expect(mockClient.post).toHaveBeenCalledWith('/decision-functions/test', payload)
    expect(result).toEqual(data)
  })
})
