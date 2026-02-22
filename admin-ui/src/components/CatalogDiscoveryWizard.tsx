import { useState, useEffect, useRef } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  submitAndStream,
  cancelDiscovery,
  getCatalog,
} from '../api/catalog'
import type {
  CatalogSchemaSelection,
  DiscoveredColumnResponse,
  DiscoveredSchemaResponse,
  DiscoveredTableResponse,
} from '../types/catalog'

interface Props {
  datasourceId: string
}

type WizardStep = 'idle' | 'schemas' | 'tables' | 'columns'

export function CatalogDiscoveryWizard({ datasourceId }: Props) {
  const queryClient = useQueryClient()

  const [step, setStep] = useState<WizardStep>('idle')
  const [selectedSchemas, setSelectedSchemas] = useState<Set<string>>(new Set())
  const [discoveredSchemas, setDiscoveredSchemas] = useState<DiscoveredSchemaResponse[]>([])
  const [discoveredTables, setDiscoveredTables] = useState<DiscoveredTableResponse[]>([])
  const [selectedTables, setSelectedTables] = useState<Set<string>>(new Set())
  const [discoveredColumns, setDiscoveredColumns] = useState<DiscoveredColumnResponse[]>([])
  const [error, setError] = useState<string | null>(null)
  const [progress, setProgress] = useState<string | null>(null)
  const [activeJobId, setActiveJobId] = useState<string | null>(null)
  const [isWorking, setIsWorking] = useState(false)

  // AbortController ref for cancelling in-flight SSE streams
  const abortRef = useRef<AbortController | null>(null)

  // Cancel on unmount
  useEffect(() => {
    return () => {
      abortRef.current?.abort()
    }
  }, [])

  const { data: catalog, isLoading: catalogLoading } = useQuery({
    queryKey: ['catalog', datasourceId],
    queryFn: () => getCatalog(datasourceId),
  })

  const catalogSummary = catalog
    ? {
        schemas: catalog.schemas.filter((s) => s.is_selected).length,
        tables: catalog.schemas.flatMap((s) => s.tables).filter((t) => t.is_selected).length,
      }
    : null

  function onProgress(phase: string, detail: string) {
    setProgress(`[${phase}] ${detail}`)
  }

  async function handleCancel() {
    abortRef.current?.abort()
    abortRef.current = null
    if (activeJobId) {
      try {
        await cancelDiscovery(datasourceId, activeJobId)
      } catch {
        // best-effort
      }
      setActiveJobId(null)
    }
    setIsWorking(false)
    setProgress(null)
    setStep('idle')
  }

  async function runDiscoverSchemas() {
    setError(null)
    setProgress(null)
    setIsWorking(true)
    const abort = new AbortController()
    abortRef.current = abort
    try {
      const data = await submitAndStream<DiscoveredSchemaResponse[]>(
        datasourceId,
        { action: 'discover_schemas' },
        onProgress,
        abort.signal,
      )
      setDiscoveredSchemas(data)
      setSelectedSchemas(new Set(data.filter((s) => s.is_already_selected).map((s) => s.schema_name)))
      setStep('schemas')
    } catch (e: unknown) {
      if (!abort.signal.aborted) {
        setError((e as Error).message ?? 'Discovery failed')
      }
    } finally {
      setIsWorking(false)
      setProgress(null)
      abortRef.current = null
    }
  }

  async function runDiscoverTables() {
    setError(null)
    setProgress(null)
    setIsWorking(true)
    const abort = new AbortController()
    abortRef.current = abort
    try {
      const data = await submitAndStream<DiscoveredTableResponse[]>(
        datasourceId,
        { action: 'discover_tables', schemas: Array.from(selectedSchemas) },
        onProgress,
        abort.signal,
      )
      setDiscoveredTables(data)
      setSelectedTables(
        new Set(
          data
            .filter((t) => t.is_already_selected)
            .map((t) => `${t.schema_name}.${t.table_name}`),
        ),
      )
      setStep('tables')
    } catch (e: unknown) {
      if (!abort.signal.aborted) {
        setError((e as Error).message ?? 'Discovery failed')
      }
    } finally {
      setIsWorking(false)
      setProgress(null)
      abortRef.current = null
    }
  }

  async function runDiscoverColumns() {
    setError(null)
    setProgress(null)
    setIsWorking(true)
    const abort = new AbortController()
    abortRef.current = abort
    try {
      const tables = Array.from(selectedTables).map((key) => {
        const [schema, ...rest] = key.split('.')
        return { schema, table: rest.join('.') }
      })
      const data = await submitAndStream<DiscoveredColumnResponse[]>(
        datasourceId,
        { action: 'discover_columns', tables },
        onProgress,
        abort.signal,
      )
      setDiscoveredColumns(data)
      setStep('columns')
    } catch (e: unknown) {
      if (!abort.signal.aborted) {
        setError((e as Error).message ?? 'Discovery failed')
      }
    } finally {
      setIsWorking(false)
      setProgress(null)
      abortRef.current = null
    }
  }

  async function runSaveCatalog() {
    setError(null)
    setProgress(null)
    setIsWorking(true)
    const abort = new AbortController()
    abortRef.current = abort

    const schemas: CatalogSchemaSelection[] = discoveredSchemas.map((schema) => ({
      schema_name: schema.schema_name,
      is_selected: selectedSchemas.has(schema.schema_name),
      tables: discoveredTables
        .filter((t) => t.schema_name === schema.schema_name)
        .map((t) => ({
          table_name: t.table_name,
          table_type: t.table_type,
          is_selected: selectedTables.has(`${t.schema_name}.${t.table_name}`),
        })),
    }))

    try {
      await submitAndStream(
        datasourceId,
        { action: 'save_catalog', schemas },
        onProgress,
        abort.signal,
      )
      queryClient.invalidateQueries({ queryKey: ['catalog', datasourceId] })
      queryClient.invalidateQueries({ queryKey: ['datasource', datasourceId] })
      setStep('idle')
      setDiscoveredSchemas([])
      setDiscoveredTables([])
      setDiscoveredColumns([])
      setSelectedSchemas(new Set())
      setSelectedTables(new Set())
    } catch (e: unknown) {
      if (!abort.signal.aborted) {
        setError((e as Error).message ?? 'Save failed')
      }
    } finally {
      setIsWorking(false)
      setProgress(null)
      abortRef.current = null
    }
  }

  async function runSyncCatalog() {
    setError(null)
    setProgress(null)
    setIsWorking(true)
    const abort = new AbortController()
    abortRef.current = abort
    try {
      await submitAndStream(
        datasourceId,
        { action: 'sync_catalog' },
        onProgress,
        abort.signal,
      )
      queryClient.invalidateQueries({ queryKey: ['catalog', datasourceId] })
      queryClient.invalidateQueries({ queryKey: ['datasource', datasourceId] })
    } catch (e: unknown) {
      if (!abort.signal.aborted) {
        setError((e as Error).message ?? 'Sync failed')
      }
    } finally {
      setIsWorking(false)
      setProgress(null)
      abortRef.current = null
    }
  }

  function toggleSchema(name: string) {
    setSelectedSchemas((prev) => {
      const next = new Set(prev)
      if (next.has(name)) next.delete(name)
      else next.add(name)
      return next
    })
  }

  function toggleTable(key: string) {
    setSelectedTables((prev) => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }

  const tableTypeLabel = (t: string) => (t === 'MATERIALIZED VIEW' ? 'MVIEW' : t)

  const tableTypeBadgeColor = (t: string) => {
    if (t === 'VIEW') return 'bg-blue-100 text-blue-700'
    if (t === 'MATERIALIZED VIEW') return 'bg-purple-100 text-purple-700'
    return 'bg-gray-100 text-gray-600'
  }

  const columnsByTable = discoveredColumns.reduce<Record<string, DiscoveredColumnResponse[]>>(
    (acc, col) => {
      const key = `${col.schema_name}.${col.table_name}`
      if (!acc[key]) acc[key] = []
      acc[key].push(col)
      return acc
    },
    {},
  )

  if (catalogLoading) {
    return <div className="text-sm text-gray-400">Loading catalog…</div>
  }

  return (
    <div>
      <h2 className="text-base font-semibold text-gray-900 mb-4">Schema Catalog</h2>

      {/* Progress / cancel bar */}
      {isWorking && (
        <div className="mb-4 flex items-center gap-3 p-3 bg-indigo-50 border border-indigo-200 rounded-lg">
          <svg
            className="animate-spin h-4 w-4 text-indigo-600 shrink-0"
            viewBox="0 0 24 24"
            fill="none"
          >
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path
              className="opacity-75"
              fill="currentColor"
              d="M4 12a8 8 0 018-8v8H4z"
            />
          </svg>
          <p className="text-sm text-indigo-800 flex-1 truncate">
            {progress ?? 'Working…'}
          </p>
          <button
            onClick={handleCancel}
            className="text-xs text-indigo-600 hover:text-indigo-800 shrink-0 underline"
          >
            Cancel
          </button>
        </div>
      )}

      {/* Idle / summary state */}
      {step === 'idle' && !isWorking && (
        <div>
          {catalogSummary && (catalogSummary.schemas > 0 || catalogSummary.tables > 0) ? (
            <div className="mb-4 p-3 bg-green-50 border border-green-200 rounded-lg text-sm text-green-800">
              Catalog: <strong>{catalogSummary.schemas}</strong> schema
              {catalogSummary.schemas !== 1 ? 's' : ''},{' '}
              <strong>{catalogSummary.tables}</strong> table
              {catalogSummary.tables !== 1 ? 's' : ''} selected
            </div>
          ) : (
            <p className="text-sm text-gray-500 mb-4">
              No catalog configured. Discover schemas to start.
            </p>
          )}

          <div className="flex gap-2">
            <button
              onClick={runDiscoverSchemas}
              disabled={isWorking}
              className="px-3 py-1.5 text-sm bg-indigo-600 text-white rounded-lg hover:bg-indigo-700 disabled:opacity-50"
            >
              Discover Schemas
            </button>

            {catalogSummary && catalogSummary.schemas > 0 && (
              <button
                onClick={runSyncCatalog}
                disabled={isWorking}
                className="px-3 py-1.5 text-sm border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 disabled:opacity-50"
              >
                Re-sync
              </button>
            )}
          </div>
        </div>
      )}

      {/* Step 1: Schema selection */}
      {step === 'schemas' && (
        <div>
          <p className="text-sm text-gray-600 mb-3">
            Select the schemas to include in the catalog:
          </p>
          <div className="space-y-1 mb-4">
            {discoveredSchemas.map((schema) => (
              <label
                key={schema.schema_name}
                className="flex items-center gap-2 px-3 py-2 rounded-lg hover:bg-gray-50 cursor-pointer"
              >
                <input
                  type="checkbox"
                  checked={selectedSchemas.has(schema.schema_name)}
                  onChange={() => toggleSchema(schema.schema_name)}
                  className="rounded border-gray-300 text-indigo-600"
                />
                <span className="text-sm font-mono text-gray-900">{schema.schema_name}</span>
                {schema.is_already_selected && (
                  <span className="text-xs text-green-600 bg-green-50 px-1.5 py-0.5 rounded">
                    selected
                  </span>
                )}
              </label>
            ))}
          </div>
          <div className="flex gap-2">
            <button
              onClick={runDiscoverTables}
              disabled={selectedSchemas.size === 0 || isWorking}
              className="px-3 py-1.5 text-sm bg-indigo-600 text-white rounded-lg hover:bg-indigo-700 disabled:opacity-50"
            >
              Next: Discover Tables
            </button>
            <button
              onClick={() => setStep('idle')}
              disabled={isWorking}
              className="px-3 py-1.5 text-sm text-gray-600 hover:text-gray-800 disabled:opacity-50"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Step 2: Table selection */}
      {step === 'tables' && (
        <div>
          <p className="text-sm text-gray-600 mb-3">Select the tables to expose to query clients:</p>
          {Array.from(selectedSchemas).map((schemaName) => {
            const tables = discoveredTables.filter((t) => t.schema_name === schemaName)
            return (
              <div key={schemaName} className="mb-4">
                <p className="text-xs font-medium text-gray-500 uppercase tracking-wide mb-1 px-1">
                  {schemaName}
                </p>
                <div className="space-y-1">
                  {tables.map((table) => {
                    const key = `${table.schema_name}.${table.table_name}`
                    return (
                      <label
                        key={key}
                        className="flex items-center gap-2 px-3 py-2 rounded-lg hover:bg-gray-50 cursor-pointer"
                      >
                        <input
                          type="checkbox"
                          checked={selectedTables.has(key)}
                          onChange={() => toggleTable(key)}
                          className="rounded border-gray-300 text-indigo-600"
                        />
                        <span className="text-sm font-mono text-gray-900">{table.table_name}</span>
                        <span
                          className={`text-xs px-1.5 py-0.5 rounded font-medium ${tableTypeBadgeColor(table.table_type)}`}
                        >
                          {tableTypeLabel(table.table_type)}
                        </span>
                        {table.is_already_selected && (
                          <span className="text-xs text-green-600 bg-green-50 px-1.5 py-0.5 rounded">
                            selected
                          </span>
                        )}
                      </label>
                    )
                  })}
                  {tables.length === 0 && (
                    <p className="text-xs text-gray-400 px-3">No tables found.</p>
                  )}
                </div>
              </div>
            )
          })}
          <div className="flex gap-2 mt-2">
            <button
              onClick={runDiscoverColumns}
              disabled={selectedTables.size === 0 || isWorking}
              className="px-3 py-1.5 text-sm bg-indigo-600 text-white rounded-lg hover:bg-indigo-700 disabled:opacity-50"
            >
              Next: Discover Columns
            </button>
            <button
              onClick={() => setStep('schemas')}
              disabled={isWorking}
              className="px-3 py-1.5 text-sm text-gray-600 hover:text-gray-800 disabled:opacity-50"
            >
              Back
            </button>
          </div>
        </div>
      )}

      {/* Step 3: Column preview & save */}
      {step === 'columns' && (
        <div>
          <p className="text-sm text-gray-600 mb-3">
            Review columns for the selected tables, then save the catalog:
          </p>
          <div className="space-y-4 mb-4 max-h-96 overflow-y-auto">
            {Object.entries(columnsByTable).map(([tableKey, cols]) => (
              <div key={tableKey}>
                <p className="text-xs font-semibold text-gray-700 font-mono mb-1">{tableKey}</p>
                <table className="w-full text-xs border-collapse">
                  <thead>
                    <tr className="text-gray-500 text-left border-b border-gray-100">
                      <th className="pb-1 pr-3 font-medium">Column</th>
                      <th className="pb-1 pr-3 font-medium">Type</th>
                      <th className="pb-1 pr-3 font-medium">Arrow</th>
                      <th className="pb-1 font-medium">Nullable</th>
                    </tr>
                  </thead>
                  <tbody>
                    {cols
                      .sort((a, b) => a.ordinal_position - b.ordinal_position)
                      .map((col) => (
                        <tr key={col.column_name} className="border-b border-gray-50">
                          <td className="py-0.5 pr-3 font-mono text-gray-900">{col.column_name}</td>
                          <td className="py-0.5 pr-3 text-gray-600">{col.data_type}</td>
                          <td className="py-0.5 pr-3 text-gray-500">
                            {col.arrow_type ?? (
                              <span className="text-yellow-600">unsupported</span>
                            )}
                          </td>
                          <td className="py-0.5 text-gray-500">{col.is_nullable ? 'yes' : 'no'}</td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </div>
            ))}
            {Object.keys(columnsByTable).length === 0 && (
              <p className="text-sm text-gray-400">No columns found for selected tables.</p>
            )}
          </div>
          <div className="flex gap-2">
            <button
              onClick={runSaveCatalog}
              disabled={isWorking}
              className="px-3 py-1.5 text-sm bg-green-600 text-white rounded-lg hover:bg-green-700 disabled:opacity-50"
            >
              Save Catalog
            </button>
            <button
              onClick={() => setStep('tables')}
              disabled={isWorking}
              className="px-3 py-1.5 text-sm text-gray-600 hover:text-gray-800 disabled:opacity-50"
            >
              Back
            </button>
          </div>
        </div>
      )}

      {error && (
        <div className="mt-3 p-2 bg-red-50 border border-red-200 rounded text-sm text-red-700">
          {error}
        </div>
      )}
    </div>
  )
}
