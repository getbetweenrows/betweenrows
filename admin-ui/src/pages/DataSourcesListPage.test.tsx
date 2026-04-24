import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { DataSourcesListPage } from './DataSourcesListPage'
import { makeDataSource } from '../test/factories'

vi.mock('../api/datasources', () => ({
  listDataSources: vi.fn(),
  testDataSource: vi.fn(),
}))

import { listDataSources, testDataSource } from '../api/datasources'
const mockListDataSources = listDataSources as ReturnType<typeof vi.fn>
const mockTestDataSource = testDataSource as ReturnType<typeof vi.fn>

function makePaginatedDs(items: ReturnType<typeof makeDataSource>[]) {
  return { data: items, total: items.length, page: 1, page_size: 20 }
}

beforeEach(() => vi.clearAllMocks())

describe('DataSourcesListPage', () => {
  it('shows loading state', () => {
    mockListDataSources.mockReturnValue(new Promise(() => {}))
    renderWithProviders(<DataSourcesListPage />, { authenticated: true })
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows empty state with create link', async () => {
    mockListDataSources.mockResolvedValue(makePaginatedDs([]))
    renderWithProviders(<DataSourcesListPage />, { authenticated: true })
    await waitFor(() =>
      expect(screen.getByText(/no data sources yet/i)).toBeInTheDocument(),
    )
    expect(screen.getByRole('link', { name: /create one/i })).toBeInTheDocument()
  })

  it('renders data source rows', async () => {
    const ds = makeDataSource({ name: 'prod-db', ds_type: 'postgres', is_active: true })
    mockListDataSources.mockResolvedValue(makePaginatedDs([ds]))
    renderWithProviders(<DataSourcesListPage />, { authenticated: true })
    await waitFor(() => expect(screen.getByText('prod-db')).toBeInTheDocument())
    expect(screen.getByText('postgres')).toBeInTheDocument()
    expect(screen.getByText('Active')).toBeInTheDocument()
  })

  it('does not expose a Delete row action (deletion lives in the edit page Danger Zone)', async () => {
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db' })
    mockListDataSources.mockResolvedValue(makePaginatedDs([ds]))

    renderWithProviders(<DataSourcesListPage />, { authenticated: true })
    await waitFor(() => screen.getByText('prod-db'))

    expect(screen.queryByRole('button', { name: /^delete$/i })).toBeNull()
  })

  it('test connection calls testDataSource and shows result', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db' })
    mockListDataSources.mockResolvedValue(makePaginatedDs([ds]))
    mockTestDataSource.mockResolvedValue({ success: true })

    renderWithProviders(<DataSourcesListPage />, { authenticated: true })
    await waitFor(() => screen.getByText('prod-db'))

    await user.click(screen.getByRole('button', { name: /^test$/i }))

    await waitFor(() => expect(screen.getByText('✓')).toBeInTheDocument())
    expect(mockTestDataSource).toHaveBeenCalledWith('ds-1')
  })

  it('shows pagination controls for multiple pages', async () => {
    const items = [makeDataSource()]
    mockListDataSources.mockResolvedValue({ data: items, total: 45, page: 1, page_size: 20 })
    renderWithProviders(<DataSourcesListPage />, { authenticated: true })
    await waitFor(() => expect(screen.getByText(/page 1 of 3/i)).toBeInTheDocument())
  })
})
