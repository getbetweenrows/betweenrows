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

  it('renders DataSourceForm with ds name and disabled type selector', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderEditPage()

    await waitFor(() => expect(screen.getByDisplayValue('prod-db')).toBeInTheDocument())
    await waitFor(() => expect(screen.getByDisplayValue('PostgreSQL')).toBeDisabled())
  })

  it('renders UserAssignmentPanel', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderEditPage()

    await waitFor(() => screen.getByText(/user access/i))
    expect(screen.getByRole('button', { name: /save assignments/i })).toBeInTheDocument()
  })

  it('renders Manage Catalog button', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderEditPage()

    await waitFor(() => screen.getByRole('button', { name: /manage catalog/i }))
  })

  it('renders read-only Policy Assignments section', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' })
    mockGetDataSource.mockResolvedValue(ds)

    renderEditPage()

    await waitFor(() => expect(screen.getByText('Policy Assignments')).toBeInTheDocument())
    expect(screen.getByText(/manage assignments from the policy edit page/i)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /assign policy/i })).toBeNull()
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
