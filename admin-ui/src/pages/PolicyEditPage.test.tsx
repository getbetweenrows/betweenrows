import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, fireEvent } from '@testing-library/react'
import { Route, Routes } from 'react-router-dom'
import { renderWithProviders } from '../test/test-utils'
import { PolicyEditPage } from './PolicyEditPage'
import { makePolicy } from '../test/factories'

vi.mock('../api/policies', () => ({
  getPolicy: vi.fn(),
  updatePolicy: vi.fn(),
  assignPolicy: vi.fn(),
  removeAssignment: vi.fn(),
}))

vi.mock('../api/datasources', () => ({
  listDataSources: vi.fn(),
}))

vi.mock('../api/users', () => ({
  listUsers: vi.fn(),
}))

vi.mock('../api/catalog', () => ({
  getCatalog: vi.fn(),
}))

import { getPolicy, updatePolicy } from '../api/policies'
import { listDataSources } from '../api/datasources'
import { listUsers } from '../api/users'
import { getCatalog } from '../api/catalog'

const mockGetPolicy = getPolicy as ReturnType<typeof vi.fn>
const mockUpdatePolicy = updatePolicy as ReturnType<typeof vi.fn>
const mockListDataSources = listDataSources as ReturnType<typeof vi.fn>
const mockListUsers = listUsers as ReturnType<typeof vi.fn>
const mockGetCatalog = getCatalog as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockListDataSources.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 200 })
  mockListUsers.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 100 })
  mockGetCatalog.mockResolvedValue({ schemas: [] })
})

function renderEditPage(policyId = 'p-1') {
  return renderWithProviders(
    <Routes>
      <Route path="/policies/:id/edit" element={<PolicyEditPage />} />
    </Routes>,
    { authenticated: true, routerEntries: [`/policies/${policyId}/edit`] },
  )
}

describe('PolicyEditPage', () => {
  it('shows loading state while fetching', () => {
    mockGetPolicy.mockReturnValue(new Promise(() => {})) // never resolves
    renderEditPage()
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows error with "Go back" when policy not found', async () => {
    mockGetPolicy.mockRejectedValue(new Error('not found'))
    renderEditPage()
    await waitFor(() => expect(screen.getByText(/policy not found/i)).toBeInTheDocument())
    expect(screen.getByText(/go back/i)).toBeInTheDocument()
  })

  it('renders form pre-populated with policy data', async () => {
    const policy = makePolicy({ id: 'p-1', name: 'row-filter', policy_type: 'row_filter', version: 2 })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue('row-filter')).toBeInTheDocument())
    expect(screen.getByText(/version 2/i)).toBeInTheDocument()
  })

  it('renders the editable assignments section', async () => {
    const policy = makePolicy({ id: 'p-1', assignments: [] })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => expect(screen.getByText('Assignments')).toBeInTheDocument())
    expect(screen.getByText('No assignments yet.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /assign policy/i })).toBeInTheDocument()
  })

  it('renders the "View as code" section', async () => {
    const policy = makePolicy({ id: 'p-1' })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => expect(screen.getByText('View as code')).toBeInTheDocument())
  })

  it('calls updatePolicy with correct version on submit', async () => {
    const policy = makePolicy({ id: 'p-1', name: 'my-policy', policy_type: 'row_filter', version: 3 })
    mockGetPolicy.mockResolvedValue(policy)
    mockUpdatePolicy.mockResolvedValue({ ...policy, version: 4 })

    const { container } = renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue('my-policy')).toBeInTheDocument())

    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() => expect(mockUpdatePolicy).toHaveBeenCalled())
    expect(mockUpdatePolicy.mock.calls[0][0]).toBe('p-1')
    expect(mockUpdatePolicy.mock.calls[0][1].version).toBe(3)
    expect(mockUpdatePolicy.mock.calls[0][1].policy_type).toBe('row_filter')
  })

  it('shows conflict message on 409 response', async () => {
    const policy = makePolicy({ id: 'p-1', version: 1 })
    mockGetPolicy.mockResolvedValue(policy)
    mockUpdatePolicy.mockRejectedValue({ response: { status: 409, data: { error: 'conflict' } } })

    const { container } = renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue(policy.name)).toBeInTheDocument())

    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() =>
      expect(screen.getByText(/modified by someone else/i)).toBeInTheDocument(),
    )
  })

  it('fetches catalog and shows hint chips when policy has an assignment', async () => {
    // Use targets with no schemas so that all catalog schemas appear as hint chips
    const policy = makePolicy({
      id: 'p-1',
      targets: [{ schemas: [], tables: [] }],
      assignments: [
        {
          id: 'a-1',
          policy_id: 'p-1',
          policy_name: 'test-policy',
          data_source_id: 'ds-42',
          datasource_name: 'prod-db',
          user_id: null,
          username: null,
          priority: 100,
          created_at: '2024-01-01T00:00:00Z',
        },
      ],
    })
    mockGetPolicy.mockResolvedValue(policy)
    mockGetCatalog.mockResolvedValue({
      schemas: [
        {
          id: 's-1',
          schema_name: 'analytics',
          schema_alias: null,
          is_selected: true,
          tables: [
            {
              id: 't-1',
              table_name: 'events',
              table_type: 'TABLE',
              is_selected: true,
              columns: [
                {
                  id: 'c-1',
                  column_name: 'id',
                  ordinal_position: 1,
                  data_type: 'integer',
                  is_nullable: false,
                  column_default: null,
                  arrow_type: 'Int64',
                  is_selected: true,
                },
              ],
            },
          ],
        },
      ],
    })

    renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue(policy.name)).toBeInTheDocument())

    expect(mockGetCatalog).toHaveBeenCalledWith('ds-42')
    // The schema hint chip "analytics" should appear in the target editor
    await waitFor(() => expect(screen.getByText('analytics')).toBeInTheDocument())
  })

  it('does not fetch catalog when policy has no assignments', async () => {
    const policy = makePolicy({ id: 'p-1', assignments: [] })
    mockGetPolicy.mockResolvedValue(policy)

    renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue(policy.name)).toBeInTheDocument())

    expect(mockGetCatalog).not.toHaveBeenCalled()
  })
})
