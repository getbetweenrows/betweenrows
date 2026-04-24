import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { Route, Routes } from 'react-router-dom'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { DataSourceEditPage } from './DataSourceEditPage'
import { makeDataSource, makeDataSourceType } from '../test/factories'

vi.mock('../api/datasources', () => ({
  getDataSource: vi.fn(),
  updateDataSource: vi.fn(),
  getDataSourceTypes: vi.fn(),
  testDataSource: vi.fn(),
  getDataSourceUsers: vi.fn(),
  setDataSourceUsers: vi.fn(),
}))

vi.mock('../api/users', () => ({
  listUsers: vi.fn(),
}))

vi.mock('../api/policies', () => ({
  listDatasourcePolicies: vi.fn(),
}))

vi.mock('../api/catalog', () => ({
  getCatalog: vi.fn().mockResolvedValue({ schemas: [] }),
  submitAndStream: vi.fn(),
  cancelDiscovery: vi.fn(),
  listRelationships: vi.fn().mockResolvedValue([]),
  listColumnAnchors: vi.fn().mockResolvedValue([]),
  listFkSuggestions: vi.fn().mockResolvedValue([]),
  createRelationship: vi.fn(),
  deleteRelationship: vi.fn(),
  createColumnAnchor: vi.fn(),
  deleteColumnAnchor: vi.fn(),
}))

import {
  getDataSource,
  updateDataSource,
  getDataSourceTypes,
  getDataSourceUsers,
} from '../api/datasources'
import { listUsers } from '../api/users'
import { listDatasourcePolicies } from '../api/policies'

const mockGetDataSource = getDataSource as ReturnType<typeof vi.fn>
const mockUpdateDataSource = updateDataSource as ReturnType<typeof vi.fn>
const mockGetTypes = getDataSourceTypes as ReturnType<typeof vi.fn>
const mockGetDsUsers = getDataSourceUsers as ReturnType<typeof vi.fn>
const mockListUsers = listUsers as ReturnType<typeof vi.fn>
const mockListDsPolicies = listDatasourcePolicies as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockGetTypes.mockResolvedValue([makeDataSourceType()])
  mockGetDsUsers.mockResolvedValue([])
  mockListUsers.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 100 })
  mockListDsPolicies.mockResolvedValue([])
})

// Wrap DataSourceEditPage in a Route so useParams works correctly
function renderEditPage() {
  return renderWithProviders(
    <Routes>
      <Route path="/datasources/:id/edit" element={<DataSourceEditPage />} />
    </Routes>,
    { authenticated: true, routerEntries: ['/datasources/ds-1/edit'] },
  )
}

describe('DataSourceEditPage', () => {
  it('shows loading state initially', () => {
    mockGetDataSource.mockReturnValue(new Promise(() => {}))
    renderEditPage()
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows not found when data source is null', async () => {
    mockGetDataSource.mockResolvedValue(null)
    renderEditPage()
    await waitFor(() =>
      expect(screen.getByText(/data source not found/i)).toBeInTheDocument(),
    )
  })

  it('renders the page header with ds name and a disabled type selector in the form', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderEditPage()

    // Name is in the page header (and breadcrumb) — no longer a form field.
    await waitFor(() =>
      expect(screen.getByRole('heading', { name: 'prod-db' })).toBeInTheDocument(),
    )
    // Type selector is still part of the Details form but read-only in edit mode.
    await waitFor(() => expect(screen.getByDisplayValue('PostgreSQL')).toBeDisabled())
  })

  it('renders UserAssignmentPanel under the Users section', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderEditPage()

    await waitFor(() => screen.getByRole('button', { name: /save changes/i }))
    await user.click(screen.getByRole('button', { name: /^Users$/ }))

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /save assignments/i })).toBeInTheDocument(),
    )
    expect(screen.getByText(/user access/i)).toBeInTheDocument()
  })

  it('opens the Catalog section when its nav item is clicked', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderEditPage()

    await waitFor(() => screen.getByRole('button', { name: /save changes/i }))
    const catalogNav = screen.getByRole('button', { name: /^Catalog$/ })
    await user.click(catalogNav)

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^Catalog$/ })).toHaveAttribute(
        'aria-current',
        'page',
      ),
    )
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: /discover schemas/i }),
      ).toBeInTheDocument(),
    )
  })

  it('renders read-only Policy Assignments under the Policies section', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderEditPage()

    await waitFor(() => screen.getByRole('button', { name: /save changes/i }))
    await user.click(screen.getByRole('button', { name: /^Policies$/ }))

    await waitFor(() => expect(screen.getByText('Policy Assignments')).toBeInTheDocument())
    expect(screen.getByText(/manage assignments from the policy edit page/i)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /assign policy/i })).toBeNull()
  })

  it('honors ?section=anchors on load', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderWithProviders(
      <Routes>
        <Route path="/datasources/:id/edit" element={<DataSourceEditPage />} />
      </Routes>,
      { authenticated: true, routerEntries: ['/datasources/ds-1/edit?section=anchors'] },
    )

    await waitFor(() => {
      const navBtn = screen.getByRole('button', { name: /^Column anchors$/ })
      expect(navBtn).toHaveAttribute('aria-current', 'page')
    })
  })

  it('submits update on save', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)
    mockUpdateDataSource.mockResolvedValue(ds)

    renderEditPage()

    await waitFor(() => screen.getByRole('button', { name: /save changes/i }))
    await user.click(screen.getByRole('button', { name: /save changes/i }))

    await waitFor(() => expect(mockUpdateDataSource).toHaveBeenCalled())
    expect(mockUpdateDataSource.mock.calls[0][0]).toBe('ds-1')
  })
})
