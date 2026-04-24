import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { Route, Routes, useLocation } from 'react-router-dom'
import { renderWithProviders } from '../test/test-utils'
import { CatalogSection, DataSourceCatalogPage } from './DataSourceCatalogPage'
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

/// Renders CatalogSection directly — the new in-page section used by
/// DataSourceEditPage's Catalog tab. These tests exercise the shared content
/// (drift panel, wizard, last-synced indicator) independently of which page
/// hosts it.
function renderSection(ds: ReturnType<typeof makeDataSource>) {
  return renderWithProviders(<CatalogSection ds={ds} />, { authenticated: true })
}

describe('CatalogSection', () => {
  it('renders the discovery wizard', async () => {
    renderSection(makeDataSource({ id: 'ds-1', name: 'prod-db', ds_type: 'postgres' }))
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /discover schemas/i })).toBeInTheDocument(),
    )
  })

  it('shows "up to date" drift panel when no drift', async () => {
    renderSection(
      makeDataSource({
        id: 'ds-1',
        name: 'prod-db',
        last_sync_result: JSON.stringify(makeUptodateDrift()),
      }),
    )
    await waitFor(() =>
      expect(screen.getByText(/catalog is up to date/i)).toBeInTheDocument(),
    )
  })

  it('shows breaking changes drift panel', async () => {
    renderSection(
      makeDataSource({
        id: 'ds-1',
        name: 'prod-db',
        last_sync_result: JSON.stringify(makeBreakingDrift()),
      }),
    )
    await waitFor(() =>
      expect(screen.getByText(/breaking schema changes detected/i)).toBeInTheDocument(),
    )
    expect(screen.getByText(/re-run the discovery wizard/i)).toBeInTheDocument()
  })

  it('shows additive changes drift panel', async () => {
    renderSection(
      makeDataSource({
        id: 'ds-1',
        name: 'prod-db',
        last_sync_result: JSON.stringify(makeAdditiveDrift()),
      }),
    )
    await waitFor(() =>
      expect(screen.getByText(/additive changes available/i)).toBeInTheDocument(),
    )
  })

  it('shows last synced time when last_sync_at is set', async () => {
    const fiveMinsAgo = new Date(Date.now() - 5 * 60 * 1000).toISOString()
    renderSection(
      makeDataSource({ id: 'ds-1', name: 'prod-db', last_sync_at: fiveMinsAgo }),
    )
    await waitFor(() => expect(screen.getByText(/last synced/i)).toBeInTheDocument())
    expect(screen.getByText(/5m ago/i)).toBeInTheDocument()
  })
})

/// Helper that surfaces the current location so we can assert redirects.
function LocationEcho() {
  const loc = useLocation()
  return <div data-testid="loc">{loc.pathname + loc.search}</div>
}

describe('DataSourceCatalogPage (legacy route redirect)', () => {
  it('redirects existing datasources to the edit page Catalog tab', async () => {
    mockGetDataSource.mockResolvedValue(makeDataSource({ id: 'ds-1', name: 'prod-db' }))

    renderWithProviders(
      <Routes>
        <Route path="/datasources/:id/catalog" element={<DataSourceCatalogPage />} />
        <Route path="/datasources/:id/edit" element={<LocationEcho />} />
        <Route path="/datasources" element={<LocationEcho />} />
      </Routes>,
      { authenticated: true, routerEntries: ['/datasources/ds-1/catalog'] },
    )

    await waitFor(() =>
      expect(screen.getByTestId('loc').textContent).toBe(
        '/datasources/ds-1/edit?section=catalog',
      ),
    )
  })

  it('redirects to the list when the datasource is missing', async () => {
    mockGetDataSource.mockResolvedValue(null)

    renderWithProviders(
      <Routes>
        <Route path="/datasources/:id/catalog" element={<DataSourceCatalogPage />} />
        <Route path="/datasources" element={<LocationEcho />} />
      </Routes>,
      { authenticated: true, routerEntries: ['/datasources/ds-1/catalog'] },
    )

    await waitFor(() =>
      expect(screen.getByTestId('loc').textContent).toBe('/datasources'),
    )
  })
})
