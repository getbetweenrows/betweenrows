import { Navigate, useParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { getDataSource } from '../api/datasources'
import { CatalogDiscoveryWizard } from '../components/CatalogDiscoveryWizard'
import type { DataSource } from '../types/datasource'
import type { DriftReport } from '../types/catalog'
import { formatRelativeTime } from '../utils/relativeTime'

function DriftReportPanel({ report }: { report: DriftReport }) {
  const hasChanges = report.schemas.some(
    (s) => s.status !== 'unchanged' || s.tables.some((t) => t.status !== 'unchanged'),
  )

  if (!hasChanges) {
    return (
      <div className="mb-4 p-3 bg-green-50 border border-green-200 rounded-lg text-sm text-green-800">
        Catalog is up to date — no schema drift detected.
      </div>
    )
  }

  const newTables = report.schemas.flatMap((s) =>
    s.tables.filter((t) => t.status === 'new').map((t) => `${s.schema_name}.${t.table_name}`),
  )
  const deletedTables = report.schemas.flatMap((s) =>
    s.tables.filter((t) => t.status === 'deleted').map((t) => `${s.schema_name}.${t.table_name}`),
  )

  if (report.has_breaking_changes) {
    return (
      <div className="mb-4 p-3 bg-amber-50 border border-amber-200 rounded-lg text-sm">
        <p className="font-medium text-amber-900 mb-2">Breaking schema changes detected</p>
        {deletedTables.length > 0 && (
          <p className="text-red-700 line-through">Removed: {deletedTables.join(', ')}</p>
        )}
        {newTables.length > 0 && (
          <p className="text-blue-700">New tables available: {newTables.join(', ')}</p>
        )}
        <p className="text-amber-700 mt-2">Re-run the discovery wizard to update the catalog.</p>
      </div>
    )
  }

  return (
    <div className="mb-4 p-3 bg-blue-50 border border-blue-200 rounded-lg text-sm">
      <p className="font-medium text-blue-900 mb-1">Additive changes available</p>
      {newTables.length > 0 && (
        <p className="text-blue-700">{newTables.length} new table(s): {newTables.join(', ')}</p>
      )}
      <p className="text-blue-600 mt-1 text-xs">Re-run discovery wizard to add these tables.</p>
    </div>
  )
}

/// In-page catalog section rendered from `DataSourceEditPage`. Shows the drift
/// summary (if any) plus the discovery wizard for selecting schemas/tables/columns.
export function CatalogSection({ ds }: { ds: DataSource }) {
  const driftReport = ds.last_sync_result
    ? (() => {
        try {
          return JSON.parse(ds.last_sync_result) as DriftReport
        } catch {
          return null
        }
      })()
    : null

  return (
    <div>
      {ds.last_sync_at && (
        <p className="mb-3 text-xs text-gray-400">
          Last synced: {formatRelativeTime(ds.last_sync_at)}
        </p>
      )}
      {driftReport && <DriftReportPanel report={driftReport} />}
      <CatalogDiscoveryWizard datasourceId={ds.id} />
    </div>
  )
}

/// Redirect legacy bookmarks to the edit page's Catalog tab.
export function DataSourceCatalogPage() {
  const { id } = useParams<{ id: string }>()
  const dsId = id ?? ''

  const { data: ds, isLoading, isError } = useQuery({
    queryKey: ['datasource', dsId],
    queryFn: () => getDataSource(dsId),
    enabled: !!dsId,
  })

  if (isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading…</div>
  }
  if (isError || !ds) {
    return <Navigate to="/datasources" replace />
  }
  return <Navigate to={`/datasources/${dsId}/edit?section=catalog`} replace />
}
