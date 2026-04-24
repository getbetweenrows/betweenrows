import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, fireEvent, within } from '@testing-library/react'
import { Route, Routes } from 'react-router-dom'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { PolicyEditPage } from './PolicyEditPage'
import { makePolicy } from '../test/factories'

vi.mock('../api/policies', () => ({
  getPolicy: vi.fn(),
  updatePolicy: vi.fn(),
  deletePolicy: vi.fn(),
  assignPolicy: vi.fn(),
  removeAssignment: vi.fn(),
  getPolicyAnchorCoverage: vi.fn(),
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

vi.mock('react-hot-toast', () => ({
  default: { success: vi.fn(), error: vi.fn() },
}))

import { deletePolicy, getPolicy, getPolicyAnchorCoverage, updatePolicy } from '../api/policies'
import { listDataSources } from '../api/datasources'
import { listUsers } from '../api/users'
import { getCatalog } from '../api/catalog'
import toast from 'react-hot-toast'

const mockGetPolicy = getPolicy as ReturnType<typeof vi.fn>
const mockUpdatePolicy = updatePolicy as ReturnType<typeof vi.fn>
const mockDeletePolicy = deletePolicy as ReturnType<typeof vi.fn>
const mockListDataSources = listDataSources as ReturnType<typeof vi.fn>
const mockListUsers = listUsers as ReturnType<typeof vi.fn>
const mockGetCatalog = getCatalog as ReturnType<typeof vi.fn>
const mockGetPolicyAnchorCoverage = getPolicyAnchorCoverage as ReturnType<typeof vi.fn>
const mockToastSuccess = toast.success as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockListDataSources.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 200 })
  mockListUsers.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 100 })
  mockGetCatalog.mockResolvedValue({ schemas: [] })
  mockGetPolicyAnchorCoverage.mockResolvedValue({
    policy_id: 'p-1',
    policy_type: 'row_filter',
    coverage: [],
  })
})

function renderEditPage(path = '/policies/p-1/edit') {
  return renderWithProviders(
    <Routes>
      <Route path="/policies/:id/edit" element={<PolicyEditPage />} />
    </Routes>,
    { authenticated: true, routerEntries: [path] },
  )
}

describe('PolicyEditPage', () => {
  it('shows loading state while fetching', () => {
    mockGetPolicy.mockReturnValue(new Promise(() => {}))
    renderEditPage()
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows error with "Go back" when policy not found', async () => {
    mockGetPolicy.mockRejectedValue(new Error('not found'))
    renderEditPage()
    await waitFor(() => expect(screen.getByText(/policy not found/i)).toBeInTheDocument())
    expect(screen.getByText(/go back/i)).toBeInTheDocument()
  })

  it('renders the page header with breadcrumb, title, and version metadata', async () => {
    const policy = makePolicy({ id: 'p-1', name: 'row-filter', policy_type: 'row_filter', version: 2 })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() =>
      expect(screen.getByRole('heading', { name: 'row-filter' })).toBeInTheDocument(),
    )
    expect(screen.getByText(/version 2/i)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Policies' })).toHaveAttribute('href', '/policies')
  })

  it('renders the form in the default Details section', async () => {
    const policy = makePolicy({ id: 'p-1', name: 'row-filter', policy_type: 'row_filter', version: 2 })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue('row-filter')).toBeInTheDocument())
  })

  it('shows Assignments section content after clicking its nav item', async () => {
    const user = userEvent.setup()
    const policy = makePolicy({ id: 'p-1', assignments: [] })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))

    await user.click(screen.getByRole('button', { name: /^Assignments$/ }))
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /assign policy/i })).toBeInTheDocument(),
    )
  })

  it('shows View as code section after clicking its nav item', async () => {
    const user = userEvent.setup()
    const policy = makePolicy({ id: 'p-1' })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))

    await user.click(screen.getByRole('button', { name: /^View as code$/ }))
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^View as code$/ })).toHaveAttribute(
        'aria-current',
        'page',
      ),
    )
  })

  it('hides the Anchor coverage section for non-row-filter policies', async () => {
    const policy = makePolicy({ id: 'p-1', policy_type: 'column_mask' })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))
    expect(screen.queryByRole('button', { name: /^Anchor coverage$/ })).toBeNull()
  })

  it('includes the Anchor coverage section for row_filter policies', async () => {
    const policy = makePolicy({ id: 'p-1', policy_type: 'row_filter' })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^Anchor coverage$/ })).toBeInTheDocument(),
    )
  })

  it('shows no nav indicator when anchor coverage is clean', async () => {
    const policy = makePolicy({ id: 'p-1', policy_type: 'row_filter' })
    mockGetPolicy.mockResolvedValue(policy)
    mockGetPolicyAnchorCoverage.mockResolvedValue({
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'orders',
          verdicts: [{ kind: 'on_table', column: 'tenant' }],
        },
      ],
    })
    renderEditPage()
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^Anchor coverage$/ })).toBeInTheDocument(),
    )
    expect(screen.queryByTestId('section-indicator-coverage')).toBeNull()
  })

  it('shows a top banner when coverage is broken on a non-coverage section', async () => {
    const policy = makePolicy({ id: 'p-1', policy_type: 'row_filter' })
    mockGetPolicy.mockResolvedValue(policy)
    mockGetPolicyAnchorCoverage.mockResolvedValue({
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'invoices',
          verdicts: [{ kind: 'missing_anchor', column: 'tenant' }],
        },
      ],
    })
    renderEditPage()
    await waitFor(() =>
      expect(screen.getByTestId('anchor-coverage-banner')).toBeInTheDocument(),
    )
    expect(document.body.textContent).toMatch(/silently deny on 1 table/i)
  })

  it('hides the banner once the user is on the Anchor coverage section', async () => {
    const user = userEvent.setup()
    const policy = makePolicy({ id: 'p-1', policy_type: 'row_filter' })
    mockGetPolicy.mockResolvedValue(policy)
    mockGetPolicyAnchorCoverage.mockResolvedValue({
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'invoices',
          verdicts: [{ kind: 'missing_anchor', column: 'tenant' }],
        },
      ],
    })
    renderEditPage()
    await waitFor(() =>
      expect(screen.getByTestId('anchor-coverage-banner')).toBeInTheDocument(),
    )
    await user.click(screen.getByRole('button', { name: /review/i }))
    await waitFor(() =>
      expect(screen.queryByTestId('anchor-coverage-banner')).toBeNull(),
    )
    // The indicator pill is part of the button's accessible name when broken,
    // so match by leading label only.
    expect(
      screen.getByRole('button', { name: /^Anchor coverage/ }),
    ).toHaveAttribute('aria-current', 'page')
  })

  it('does not show the banner when coverage is clean', async () => {
    const policy = makePolicy({ id: 'p-1', policy_type: 'row_filter' })
    mockGetPolicy.mockResolvedValue(policy)
    mockGetPolicyAnchorCoverage.mockResolvedValue({
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'orders',
          verdicts: [{ kind: 'on_table', column: 'tenant' }],
        },
      ],
    })
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))
    expect(screen.queryByTestId('anchor-coverage-banner')).toBeNull()
  })

  it('flags the Anchor coverage section with a red count pill when broken', async () => {
    const policy = makePolicy({ id: 'p-1', policy_type: 'row_filter' })
    mockGetPolicy.mockResolvedValue(policy)
    mockGetPolicyAnchorCoverage.mockResolvedValue({
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'invoices',
          verdicts: [{ kind: 'missing_anchor', column: 'tenant' }],
        },
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'payments',
          verdicts: [{ kind: 'missing_anchor', column: 'tenant' }],
        },
      ],
    })
    renderEditPage()
    await waitFor(() => {
      const indicator = screen.getByTestId('section-indicator-coverage')
      expect(indicator).toBeInTheDocument()
      expect(indicator.textContent).toBe('2')
    })
  })

  it('honors ?section=assignments on load', async () => {
    const policy = makePolicy({ id: 'p-1' })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage('/policies/p-1/edit?section=assignments')
    await waitFor(() => {
      const navBtn = screen.getByRole('button', { name: /^Assignments$/ })
      expect(navBtn).toHaveAttribute('aria-current', 'page')
    })
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

  it('shows a success toast and stays on the page after save (no auto-navigate)', async () => {
    const policy = makePolicy({ id: 'p-1', version: 1 })
    mockGetPolicy.mockResolvedValue(policy)
    mockUpdatePolicy.mockResolvedValue({ ...policy, version: 2 })

    const { container } = renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue(policy.name)).toBeInTheDocument())

    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() => expect(mockUpdatePolicy).toHaveBeenCalled())
    await waitFor(() => expect(mockToastSuccess).toHaveBeenCalledWith('Saved'))
    // Page stays on edit route: the Details form should still be rendered.
    expect(screen.getByDisplayValue(policy.name)).toBeInTheDocument()
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
          role_id: null,
          role_name: null,
          assignment_scope: 'all',
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

describe('PolicyEditPage — danger zone', () => {
  it('opens a typed-name delete modal from the Details section', async () => {
    const user = userEvent.setup()
    const policy = makePolicy({ id: 'p-1', name: 'my-policy' })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    expect(screen.getByRole('dialog', { name: /delete my-policy\?/i })).toBeInTheDocument()
  })

  it('offers "Disable instead" when the policy is enabled', async () => {
    const user = userEvent.setup()
    const policy = makePolicy({ id: 'p-1', name: 'my-policy', is_enabled: true })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    expect(
      screen.getByRole('button', { name: /disable instead/i }),
    ).toBeInTheDocument()
  })

  it('hides "Disable instead" when the policy is already disabled', async () => {
    const user = userEvent.setup()
    const policy = makePolicy({ id: 'p-1', name: 'my-policy', is_enabled: false })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    expect(screen.queryByRole('button', { name: /disable instead/i })).toBeNull()
  })

  it('Disable-instead path calls updatePolicy with is_enabled: false and does not delete', async () => {
    const user = userEvent.setup()
    const policy = makePolicy({ id: 'p-1', name: 'my-policy', is_enabled: true, version: 5 })
    mockGetPolicy.mockResolvedValue(policy)
    mockUpdatePolicy.mockResolvedValue(policy)

    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    await user.click(screen.getByRole('button', { name: /disable instead/i }))

    await waitFor(() => expect(mockUpdatePolicy).toHaveBeenCalled())
    expect(mockUpdatePolicy.mock.calls[0][0]).toBe('p-1')
    expect(mockUpdatePolicy.mock.calls[0][1]).toMatchObject({ is_enabled: false, version: 5 })
    expect(mockDeletePolicy).not.toHaveBeenCalled()
  })

  it('Delete button is disabled until the typed name matches exactly', async () => {
    const user = userEvent.setup()
    const policy = makePolicy({ id: 'p-1', name: 'my-policy', is_enabled: false })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue(policy.name))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    const dialog = screen.getByRole('dialog')
    const submit = within(dialog).getByRole('button', { name: /^delete$/i })
    expect(submit).toBeDisabled()

    await user.type(within(dialog).getByRole('textbox'), 'my-policy')
    expect(submit).not.toBeDisabled()
  })
})
