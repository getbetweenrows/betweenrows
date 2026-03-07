import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { renderWithProviders } from '../test/test-utils'
import { makePolicy, makePolicyAssignment } from '../test/factories'
import { makeUser } from '../test/factories'

vi.mock('../api/policies', () => ({
  listDatasourcePolicies: vi.fn(),
  listPolicies: vi.fn(),
  assignPolicy: vi.fn(),
  removeAssignment: vi.fn(),
}))

vi.mock('../api/users', () => ({
  listUsers: vi.fn(),
}))

import { listDatasourcePolicies, listPolicies } from '../api/policies'
import { listUsers } from '../api/users'
import { PolicyAssignmentsReadonly, PolicyAssignmentPanel } from './PolicyAssignmentPanel'

const mockListDsPolicies = listDatasourcePolicies as ReturnType<typeof vi.fn>
const mockListPolicies = listPolicies as ReturnType<typeof vi.fn>
const mockListUsers = listUsers as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockListDsPolicies.mockResolvedValue([])
  mockListPolicies.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 100 })
  mockListUsers.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 100 })
})

// ===== PolicyAssignmentsReadonly =====

describe('PolicyAssignmentsReadonly', () => {
  it('renders "No assignments yet" when empty', () => {
    renderWithProviders(<PolicyAssignmentsReadonly assignments={[]} />)
    expect(screen.getByText('No assignments yet.')).toBeInTheDocument()
  })

  it('renders table rows with datasource name, username, and priority', () => {
    const a = makePolicyAssignment({
      data_source_id: 'ds-1',
      datasource_name: 'prod-db',
      username: 'alice',
      priority: 50,
    })
    renderWithProviders(<PolicyAssignmentsReadonly assignments={[a]} />)
    expect(screen.getByText('prod-db')).toBeInTheDocument()
    expect(screen.getByText('alice')).toBeInTheDocument()
    expect(screen.getByText('50')).toBeInTheDocument()
  })

  it('renders "all users" italic text when username is null', () => {
    const a = makePolicyAssignment({ username: null, user_id: null })
    renderWithProviders(<PolicyAssignmentsReadonly assignments={[a]} />)
    expect(screen.getByText('all users')).toBeInTheDocument()
  })

  it('datasource name links to /datasources/:id/edit', () => {
    const a = makePolicyAssignment({
      data_source_id: 'ds-42',
      datasource_name: 'my-db',
    })
    renderWithProviders(<PolicyAssignmentsReadonly assignments={[a]} />)
    const link = screen.getByRole('link', { name: 'my-db' })
    expect(link).toHaveAttribute('href', '/datasources/ds-42/edit')
  })
})

// ===== PolicyAssignmentPanel =====

describe('PolicyAssignmentPanel', () => {
  it('shows "No policies assigned yet" when empty', async () => {
    mockListDsPolicies.mockResolvedValue([])
    renderWithProviders(<PolicyAssignmentPanel datasourceId="ds-1" />, { authenticated: true })
    await waitFor(() =>
      expect(screen.getByText('No policies assigned yet.')).toBeInTheDocument(),
    )
  })

  it('renders assignments with policy name linked to /policies/:id/edit', async () => {
    const a = makePolicyAssignment({
      policy_id: 'p-99',
      policy_name: 'row-filter',
      priority: 100,
    })
    mockListDsPolicies.mockResolvedValue([a])
    renderWithProviders(<PolicyAssignmentPanel datasourceId="ds-1" />, { authenticated: true })
    await waitFor(() => expect(screen.getByText('row-filter')).toBeInTheDocument())
    const link = screen.getByRole('link', { name: 'row-filter' })
    expect(link).toHaveAttribute('href', '/policies/p-99/edit')
  })

  it('shows the add assignment form with policy, user, and priority fields', async () => {
    renderWithProviders(<PolicyAssignmentPanel datasourceId="ds-1" />, { authenticated: true })
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /assign policy/i })).toBeInTheDocument(),
    )
    expect(screen.getByRole('spinbutton')).toBeInTheDocument() // priority number input
  })

  it('populates policy dropdown from listPolicies', async () => {
    const policy = makePolicy({ id: 'p-1', name: 'deny-cols', effect: 'deny' })
    mockListPolicies.mockResolvedValue({ data: [policy], total: 1, page: 1, page_size: 100 })
    renderWithProviders(<PolicyAssignmentPanel datasourceId="ds-1" />, { authenticated: true })
    await waitFor(() => expect(screen.getByText(/deny-cols.*deny/)).toBeInTheDocument())
  })

  it('populates user dropdown from listUsers', async () => {
    const user = makeUser({ id: 'u-1', username: 'bob' })
    mockListUsers.mockResolvedValue({ data: [user], total: 1, page: 1, page_size: 100 })
    renderWithProviders(<PolicyAssignmentPanel datasourceId="ds-1" />, { authenticated: true })
    await waitFor(() => expect(screen.getByText('bob')).toBeInTheDocument())
  })

  it('renders a Remove button for each assignment', async () => {
    const assignments = [
      makePolicyAssignment({ id: 'a-1', policy_name: 'p1' }),
      makePolicyAssignment({ id: 'a-2', policy_name: 'p2' }),
    ]
    mockListDsPolicies.mockResolvedValue(assignments)
    renderWithProviders(<PolicyAssignmentPanel datasourceId="ds-1" />, { authenticated: true })
    await waitFor(() => expect(screen.getAllByRole('button', { name: /remove/i })).toHaveLength(2))
  })
})
