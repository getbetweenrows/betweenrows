import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { Route, Routes } from 'react-router-dom'
import { renderWithProviders } from '../test/test-utils'
import { DataSourceCatalogPage } from './DataSourceCatalogPage'
import { makeDataSource } from '../test/factories'
import type { DriftReport } from '../types/catalog'

vi.mock('../api/datasources', () => ({
  getDataSource: vi.fn(),
}))

vi.mock('../api/catalog', () => ({
  getCatalog: vi.fn(),
  submitAndStream: vi.fn(),
  cancelDiscovery: vi.fn(),
}))

import { getDataSource } from '../api/datasources'
import { getCatalog } from '../api/catalog'

const mockGetDataSource = getDataSource as ReturnType<typeof vi.fn>
const mockGetCatalog = getCatalog as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockGetCatalog.mockResolvedValue({ schemas: [] })
})

// Wrap DataSourceCatalogPage in a Route so useParams works correctly
function renderCatalogPage(ds: ReturnType<typeof makeDataSource> | null = null) {
  if (ds) mockGetDataSource.mockResolvedValue(ds)
  return renderWithProviders(
    <Routes>
      <Route path="/datasources/:id/catalog" element={<DataSourceCatalogPage />} />
    </Routes>,
    { authenticated: true, routerEntries: ['/datasources/ds-1/catalog'] },
  )
}

function makeBreakingDrift(): DriftReport {
  return {
    schemas: [
      {
        schema_name: 'public',
        status: 'unchanged',
        tables: [{ table_name: 'users', status: 'deleted', columns: [] }],
      },
    ],
    has_breaking_changes: true,
  }
}

function makeAdditiveDrift(): DriftReport {
  return {
    schemas: [
      {
        schema_name: 'public',
        status: 'unchanged',
        tables: [{ table_name: 'orders', status: 'new', columns: [] }],
      },
    ],
    has_breaking_changes: false,
  }
}

function makeUptodateDrift(): DriftReport {
  return { schemas: [], has_breaking_changes: false }
}

describe('DataSourceCatalogPage', () => {
  it('shows loading state', () => {
    mockGetDataSource.mockReturnValue(new Promise(() => {}))
    renderCatalogPage()
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows not found when data source is null', async () => {
    mockGetDataSource.mockResolvedValue(null)
    renderCatalogPage()
    await waitFor(() =>
      expect(screen.getByText(/data source not found/i)).toBeInTheDocument(),
    )
  })

  it('renders datasource name and wizard', async () => {
    renderCatalogPage(makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' }))
    await waitFor(() => expect(screen.getByText('prod-db')).toBeInTheDocument())
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /discover schemas/i })).toBeInTheDocument(),
    )
  })

  it('shows "up to date" drift panel when no drift', async () => {
    renderCatalogPage(
      makeDataSource({
        id: 'ds-1', name: 'prod-db',
        last_sync_result: JSON.stringify(makeUptodateDrift()),
      }),
    )
    await waitFor(() =>
      expect(screen.getByText(/catalog is up to date/i)).toBeInTheDocument(),
    )
  })

  it('shows breaking changes drift panel', async () => {
    renderCatalogPage(
      makeDataSource({
        id: 'ds-1', name: 'prod-db',
        last_sync_result: JSON.stringify(makeBreakingDrift()),
      }),
    )
    await waitFor(() =>
      expect(screen.getByText(/breaking schema changes detected/i)).toBeInTheDocument(),
    )
    expect(screen.getByText(/re-run the discovery wizard/i)).toBeInTheDocument()
  })

  it('shows additive changes drift panel', async () => {
    renderCatalogPage(
      makeDataSource({
        id: 'ds-1', name: 'prod-db',
        last_sync_result: JSON.stringify(makeAdditiveDrift()),
      }),
    )
    await waitFor(() =>
      expect(screen.getByText(/additive changes available/i)).toBeInTheDocument(),
    )
  })

  it('shows last synced time when last_sync_at is set', async () => {
    const fiveMinsAgo = new Date(Date.now() - 5 * 60 * 1000).toISOString()
    renderCatalogPage(
      makeDataSource({ id: 'ds-1', name: 'prod-db', last_sync_at: fiveMinsAgo }),
    )
    await waitFor(() => expect(screen.getByText(/last synced/i)).toBeInTheDocument())
    expect(screen.getByText(/5m ago/i)).toBeInTheDocument()
  })
})
