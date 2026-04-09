import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { AdminAuditPage } from './AdminAuditPage'
import { renderWithProviders } from '../test/test-utils'

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver

vi.mock('../api/adminAudit', () => ({
  listAdminAuditLogs: vi.fn().mockResolvedValue({
    data: [],
    total: 0,
    page: 1,
    page_size: 20,
  }),
}))

vi.mock('../utils/entitySearchFns', () => ({
  searchUsers: vi.fn().mockResolvedValue([
    { id: 'user-uuid-1', label: 'alice' },
  ]),
  searchRoles: vi.fn().mockResolvedValue([
    { id: 'role-uuid-1', label: 'admin-role' },
  ]),
  searchDataSources: vi.fn().mockResolvedValue([
    { id: 'ds-uuid-1', label: 'prod-db' },
  ]),
  searchPolicies: vi.fn().mockResolvedValue([
    { id: 'policy-uuid-1', label: 'deny-all' },
  ]),
}))

describe('AdminAuditPage', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('renders EntitySelect for Actor filter', () => {
    renderWithProviders(<AdminAuditPage />, { authenticated: true })
    expect(screen.getByText('Actor')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Search users…')).toBeInTheDocument()
  })

  it('shows Resource EntitySelect when resource type is selected', async () => {
    renderWithProviders(<AdminAuditPage />, { authenticated: true })

    // No Resource select initially
    expect(screen.queryByText('Resource')).not.toBeInTheDocument()

    // Select resource type
    const select = screen.getByDisplayValue('All')
    await userEvent.selectOptions(select, 'role')

    expect(screen.getByText('Resource')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Search roles…')).toBeInTheDocument()
  })

  it('clears resource selection when resource type changes', async () => {
    renderWithProviders(<AdminAuditPage />, { authenticated: true })

    const select = screen.getByDisplayValue('All')

    // Select role type
    await userEvent.selectOptions(select, 'role')
    expect(screen.getByPlaceholderText('Search roles…')).toBeInTheDocument()

    // Change to policy type — resource field should reset to new placeholder
    await userEvent.selectOptions(select, 'policy')
    expect(screen.queryByPlaceholderText('Search roles…')).not.toBeInTheDocument()
    expect(screen.getByPlaceholderText('Search policies…')).toBeInTheDocument()
  })

  it('hides Resource EntitySelect when resource type is cleared', async () => {
    renderWithProviders(<AdminAuditPage />, { authenticated: true })

    const select = screen.getByDisplayValue('All')
    await userEvent.selectOptions(select, 'role')
    expect(screen.getByText('Resource')).toBeInTheDocument()

    await userEvent.selectOptions(select, '')
    expect(screen.queryByText('Resource')).not.toBeInTheDocument()
  })
})
