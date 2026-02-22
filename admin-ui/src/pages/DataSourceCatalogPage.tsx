import { useNavigate, useParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { getDataSource } from '../api/datasources'
import { CatalogDiscoveryWizard } from '../components/CatalogDiscoveryWizard'
import type { DriftReport } from '../types/catalog'

function formatRelativeTime(isoString: string): string {
  const diff = Date.now() - new Date(isoString).getTime()
  const mins = Math.floor(diff / 60000)
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins}m ago`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

function DriftReportPanel({ report }: { report: DriftReport }) {
  const hasChanges = report.schemas.some(
    (s) => s.status !== 'unchanged' || s.tables.some((t) => t.status !== 'unchanged'),
  )

  if (!hasChanges) {
    return (
      <div className="mt-4 p-3 bg-green-50 border border-green-200 rounded-lg text-sm text-green-800">
        Catalog is up to date — no schema drift detected.
      </div>
    )
  }

  const newTables = report.schemas
    .flatMap((s) => s.tables.filter((t) => t.status === 'new').map((t) => `${s.schema_name}.${t.table_name}`))
  const deletedTables = report.schemas
    .flatMap((s) => s.tables.filter((t) => t.status === 'deleted').map((t) => `${s.schema_name}.${t.table_name}`))

  if (report.has_breaking_changes) {
    return (
      <div className="mt-4 p-3 bg-amber-50 border border-amber-200 rounded-lg text-sm">
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
    <div className="mt-4 p-3 bg-blue-50 border border-blue-200 rounded-lg text-sm">
      <p className="font-medium text-blue-900 mb-1">Additive changes available</p>
      {newTables.length > 0 && (
        <p className="text-blue-700">{newTables.length} new table(s): {newTables.join(', ')}</p>
      )}
      <p className="text-blue-600 mt-1 text-xs">Re-run discovery wizard to add these tables.</p>
    </div>
  )
}

export function DataSourceCatalogPage() {
  const { id } = useParams<{ id: string }>()
  const dsId = id ?? ''
  const navigate = useNavigate()

  const { data: ds, isLoading, isError } = useQuery({
    queryKey: ['datasource', dsId],
    queryFn: () => getDataSource(dsId),
    enabled: !!dsId,
  })

  if (isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading…</div>
  }

  if (isError || !ds) {
    return (
      <div className="p-6 text-sm text-red-500">
        Data source not found.{' '}
        <button onClick={() => navigate('/datasources')} className="underline">
          Go back
        </button>
      </div>
    )
  }

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
    <div className="p-6 max-w-3xl">
      <div className="mb-6">
        <button
          onClick={() => navigate(`/datasources/${dsId}/edit`)}
          className="text-sm text-gray-500 hover:text-gray-700 mb-2"
        >
          ← Back to {ds.name}
        </button>
        <h1 className="text-xl font-bold text-gray-900">
          Catalog: <span className="font-mono text-lg">{ds.name}</span>
        </h1>
        <div className="flex items-center gap-3 mt-1">
          <p className="text-sm text-gray-500">Type: {ds.ds_type}</p>
          {ds.last_sync_at && (
            <p className="text-xs text-gray-400">
              Last synced: {formatRelativeTime(ds.last_sync_at)}
            </p>
          )}
        </div>
      </div>

      {driftReport && <DriftReportPanel report={driftReport} />}

      <div className="bg-white rounded-xl border border-gray-200 p-6 mt-6">
        <CatalogDiscoveryWizard datasourceId={dsId} />
      </div>
    </div>
  )
}
