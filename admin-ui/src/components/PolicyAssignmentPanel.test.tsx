import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, fireEvent } from '@testing-library/react'
import { renderWithProviders } from '../test/test-utils'
import { makePolicyAssignment, makeDataSource } from '../test/factories'
import { makeUser } from '../test/factories'

vi.mock('../api/policies', () => ({
  listDatasourcePolicies: vi.fn(),
  assignPolicy: vi.fn(),
  removeAssignment: vi.fn(),
}))

vi.mock('../api/datasources', () => ({
  listDataSources: vi.fn(),
}))

vi.mock('../api/users', () => ({
  listUsers: vi.fn(),
}))

import { listDatasourcePolicies, assignPolicy } from '../api/policies'
import { listDataSources } from '../api/datasources'
import { listUsers } from '../api/users'
import {
  PolicyAssignmentsReadonly,
  PolicyAssignmentEditPanel,
  DatasourceAssignmentsReadonly,
} from './PolicyAssignmentPanel'

const mockListDsPolicies = listDatasourcePolicies as ReturnType<typeof vi.fn>
const mockAssignPolicy = assignPolicy as ReturnType<typeof vi.fn>
const mockListDataSources = listDataSources as ReturnType<typeof vi.fn>
const mockListUsers = listUsers as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockListDsPolicies.mockResolvedValue([])
  mockAssignPolicy.mockResolvedValue({})
  mockListDataSources.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 200 })
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

// ===== PolicyAssignmentEditPanel =====

describe('PolicyAssignmentEditPanel', () => {
  it('shows "No assignments yet" when assignments prop is empty', () => {
    renderWithProviders(
      <PolicyAssignmentEditPanel
        policyId="p-1"
        assignments={[]}
        onAssignmentChange={vi.fn()}
      />,
      { authenticated: true },
    )
    expect(screen.getByText('No assignments yet.')).toBeInTheDocument()
  })

  it('renders assignments table with datasource link and Remove button', () => {
    const a = makePolicyAssignment({
      id: 'a-1',
      data_source_id: 'ds-99',
      datasource_name: 'staging-db',
      username: 'alice',
      priority: 50,
    })
    renderWithProviders(
      <PolicyAssignmentEditPanel
        policyId="p-1"
        assignments={[a]}
        onAssignmentChange={vi.fn()}
      />,
      { authenticated: true },
    )
    const link = screen.getByRole('link', { name: 'staging-db' })
    expect(link).toHaveAttribute('href', '/datasources/ds-99/edit')
    expect(screen.getByRole('button', { name: /remove/i })).toBeInTheDocument()
  })

  it('shows the Add Assignment form with datasource, user, and priority fields', () => {
    renderWithProviders(
      <PolicyAssignmentEditPanel
        policyId="p-1"
        assignments={[]}
        onAssignmentChange={vi.fn()}
      />,
      { authenticated: true },
    )
    expect(screen.getByRole('button', { name: /assign policy/i })).toBeInTheDocument()
    expect(screen.getByRole('spinbutton')).toBeInTheDocument() // priority number input
  })

  it('populates datasource dropdown from listDataSources', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'my-db', is_active: true })
    mockListDataSources.mockResolvedValue({ data: [ds], total: 1, page: 1, page_size: 200 })
    renderWithProviders(
      <PolicyAssignmentEditPanel
        policyId="p-1"
        assignments={[]}
        onAssignmentChange={vi.fn()}
      />,
      { authenticated: true },
    )
    await waitFor(() => expect(screen.getByText('my-db')).toBeInTheDocument())
  })

  it('populates user dropdown from listUsers', async () => {
    const user = makeUser({ id: 'u-1', username: 'bob' })
    mockListUsers.mockResolvedValue({ data: [user], total: 1, page: 1, page_size: 100 })
    renderWithProviders(
      <PolicyAssignmentEditPanel
        policyId="p-1"
        assignments={[]}
        onAssignmentChange={vi.fn()}
      />,
      { authenticated: true },
    )
    await waitFor(() => expect(screen.getByText('bob')).toBeInTheDocument())
  })

  it('renders a Remove button for each assignment', () => {
    const assignments = [
      makePolicyAssignment({ id: 'a-1', datasource_name: 'db1' }),
      makePolicyAssignment({ id: 'a-2', datasource_name: 'db2' }),
    ]
    renderWithProviders(
      <PolicyAssignmentEditPanel
        policyId="p-1"
        assignments={assignments}
        onAssignmentChange={vi.fn()}
      />,
      { authenticated: true },
    )
    expect(screen.getAllByRole('button', { name: /remove/i })).toHaveLength(2)
  })

  it('shows error message when duplicate assignment returns 409', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', is_active: true })
    mockListDataSources.mockResolvedValue({ data: [ds], total: 1, page: 1, page_size: 200 })
    mockAssignPolicy.mockRejectedValue({
      response: { data: { error: 'This policy is already assigned to this datasource for all users' } },
    })

    const { container } = renderWithProviders(
      <PolicyAssignmentEditPanel
        policyId="p-1"
        assignments={[]}
        onAssignmentChange={vi.fn()}
      />,
      { authenticated: true },
    )

    await waitFor(() => expect(screen.getByText('prod-db')).toBeInTheDocument())

    const dsSelect = container.querySelectorAll('select')[0]
    fireEvent.change(dsSelect, { target: { value: 'ds-1' } })

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /assign policy/i })).not.toBeDisabled(),
    )
    screen.getByRole('button', { name: /assign policy/i }).click()

    await waitFor(() =>
      expect(
        screen.getByText(/already assigned to this datasource for all users/i),
      ).toBeInTheDocument(),
    )
  })
})

// ===== DatasourceAssignmentsReadonly =====

describe('DatasourceAssignmentsReadonly', () => {
  it('shows "No policies assigned yet" when empty', async () => {
    mockListDsPolicies.mockResolvedValue([])
    renderWithProviders(<DatasourceAssignmentsReadonly datasourceId="ds-1" />, {
      authenticated: true,
    })
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
    renderWithProviders(<DatasourceAssignmentsReadonly datasourceId="ds-1" />, {
      authenticated: true,
    })
    await waitFor(() => expect(screen.getByText('row-filter')).toBeInTheDocument())
    const link = screen.getByRole('link', { name: 'row-filter' })
    expect(link).toHaveAttribute('href', '/policies/p-99/edit')
  })

  it('shows the "Manage assignments from the policy edit page" note', async () => {
    renderWithProviders(<DatasourceAssignmentsReadonly datasourceId="ds-1" />, {
      authenticated: true,
    })
    await waitFor(() =>
      expect(
        screen.getByText(/manage assignments from the policy edit page/i),
      ).toBeInTheDocument(),
    )
  })

  it('does not render a Remove button', async () => {
    const a = makePolicyAssignment({ policy_name: 'deny-cols' })
    mockListDsPolicies.mockResolvedValue([a])
    renderWithProviders(<DatasourceAssignmentsReadonly datasourceId="ds-1" />, {
      authenticated: true,
    })
    await waitFor(() => expect(screen.getByText('deny-cols')).toBeInTheDocument())
    expect(screen.queryByRole('button', { name: /remove/i })).toBeNull()
  })
})
