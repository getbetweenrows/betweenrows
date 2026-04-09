import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryAuditPage } from './QueryAuditPage'
import { renderWithProviders } from '../test/test-utils'

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver

vi.mock('../api/audit', () => ({
  listAuditLogs: vi.fn().mockResolvedValue({
    data: [],
    total: 0,
    page: 1,
    page_size: 20,
  }),
}))

vi.mock('../utils/entitySearchFns', () => ({
  searchUsers: vi.fn().mockResolvedValue([
    { id: 'user-uuid-1', label: 'alice' },
    { id: 'user-uuid-2', label: 'bob' },
  ]),
  searchDataSources: vi.fn().mockResolvedValue([
    { id: 'ds-uuid-1', label: 'prod-db' },
  ]),
}))

describe('QueryAuditPage', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('renders EntitySelect filters for User and Data Source', async () => {
    renderWithProviders(<QueryAuditPage />, { authenticated: true })

    expect(screen.getByText('User')).toBeInTheDocument()
    expect(screen.getByText('Data Source')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Search users…')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Search data sources…')).toBeInTheDocument()
  })

  it('shows user options in typeahead dropdown', async () => {
    renderWithProviders(<QueryAuditPage />, { authenticated: true })

    const userInput = screen.getByPlaceholderText('Search users…')
    await userEvent.type(userInput, 'ali')
    await vi.advanceTimersByTimeAsync(350)

    await waitFor(() => {
      expect(screen.getByText('alice')).toBeInTheDocument()
    })
  })
})
