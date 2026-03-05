import { describe, it, expect, vi, beforeEach } from 'vitest'
import { makeEmptyCatalog, makeDiscoveredSchema } from '../test/factories'

vi.mock('./client', () => ({
  client: {
    post: vi.fn(),
    get: vi.fn(),
    delete: vi.fn(),
  },
}))

// Mock @microsoft/fetch-event-source
vi.mock('@microsoft/fetch-event-source', () => ({
  fetchEventSource: vi.fn(),
}))

import { client } from './client'
import { fetchEventSource } from '@microsoft/fetch-event-source'
import { submitAndStream, cancelDiscovery, getDiscoveryStatus, getCatalog } from './catalog'

const mockClient = client as unknown as {
  post: ReturnType<typeof vi.fn>
  get: ReturnType<typeof vi.fn>
  delete: ReturnType<typeof vi.fn>
}
const mockFetchEventSource = fetchEventSource as ReturnType<typeof vi.fn>

beforeEach(() => vi.clearAllMocks())

describe('submitAndStream', () => {
  it('submits job then resolves on result event', async () => {
    mockClient.post.mockResolvedValue({ data: { job_id: 'job-1' } })
    const schemas = [makeDiscoveredSchema()]

    mockFetchEventSource.mockImplementation((_url: string, opts: {
      onmessage: (ev: { data: string }) => void
      onerror: (err: unknown) => void
    }) => {
      // Simulate a progress event then result event
      opts.onmessage({ data: JSON.stringify({ type: 'progress', phase: 'discover', detail: 'scanning' }) })
      opts.onmessage({ data: JSON.stringify({ type: 'result', data: schemas }) })
    })

    const onProgress = vi.fn()
    const result = await submitAndStream<typeof schemas>('ds-1', { action: 'discover_schemas' }, onProgress)

    expect(mockClient.post).toHaveBeenCalledWith('/datasources/ds-1/discover', { action: 'discover_schemas' })
    expect(mockFetchEventSource).toHaveBeenCalledWith(
      expect.stringContaining('ds-1/discover/job-1/events'),
      expect.objectContaining({ headers: expect.objectContaining({ Authorization: 'Bearer ' }) }),
    )
    expect(onProgress).toHaveBeenCalledWith('discover', 'scanning')
    expect(result).toEqual(schemas)
  })

  it('rejects on error event', async () => {
    mockClient.post.mockResolvedValue({ data: { job_id: 'job-2' } })

    mockFetchEventSource.mockImplementation((_url: string, opts: {
      onmessage: (ev: { data: string }) => void
      onerror: (err: unknown) => void
    }) => {
      opts.onmessage({ data: JSON.stringify({ type: 'error', message: 'Connection refused' }) })
    })

    await expect(
      submitAndStream('ds-1', { action: 'discover_schemas' }, vi.fn()),
    ).rejects.toThrow('Connection refused')
  })

  it('rejects on cancelled event', async () => {
    mockClient.post.mockResolvedValue({ data: { job_id: 'job-3' } })

    mockFetchEventSource.mockImplementation((_url: string, opts: {
      onmessage: (ev: { data: string }) => void
      onerror: (err: unknown) => void
    }) => {
      opts.onmessage({ data: JSON.stringify({ type: 'cancelled' }) })
    })

    await expect(
      submitAndStream('ds-1', { action: 'discover_schemas' }, vi.fn()),
    ).rejects.toThrow('Discovery cancelled')
  })

  it('rejects via onerror', async () => {
    mockClient.post.mockResolvedValue({ data: { job_id: 'job-4' } })

    mockFetchEventSource.mockImplementation((_url: string, opts: {
      onmessage: (ev: { data: string }) => void
      onerror: (err: unknown) => void
    }) => {
      opts.onerror(new Error('SSE failed'))
    })

    await expect(
      submitAndStream('ds-1', { action: 'discover_schemas' }, vi.fn()),
    ).rejects.toThrow('SSE failed')
  })
})

describe('cancelDiscovery', () => {
  it('DELETEs the job', async () => {
    mockClient.delete.mockResolvedValue({})
    await cancelDiscovery('ds-1', 'job-1')
    expect(mockClient.delete).toHaveBeenCalledWith('/datasources/ds-1/discover/job-1')
  })
})

describe('getDiscoveryStatus', () => {
  it('GETs job status', async () => {
    const status = { job_id: 'job-1', action: 'discover_schemas', status: 'completed', result: null, error: null }
    mockClient.get.mockResolvedValue({ data: status })
    const result = await getDiscoveryStatus('ds-1', 'job-1')
    expect(mockClient.get).toHaveBeenCalledWith('/datasources/ds-1/discover/job-1')
    expect(result).toEqual(status)
  })
})

describe('getCatalog', () => {
  it('GETs the stored catalog', async () => {
    const catalog = makeEmptyCatalog()
    mockClient.get.mockResolvedValue({ data: catalog })
    const result = await getCatalog('ds-1')
    expect(mockClient.get).toHaveBeenCalledWith('/datasources/ds-1/catalog')
    expect(result).toEqual(catalog)
  })
})
