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
  const [selectedColumns, setSelectedColumns] = useState<Set<string>>(new Set())
  const [activeTable, setActiveTable] = useState<string | null>(null)
  const [schemaSearch, setSchemaSearch] = useState('')
  const [tableSearch, setTableSearch] = useState('')
  const [columnSearch, setColumnSearch] = useState('')
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
      setSchemaSearch('')
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
      setTableSearch('')
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
      setActiveTable(
        data.length > 0 ? `${data[0].schema_name}.${data[0].table_name}` : null,
      )

      // Pre-select columns based on is_already_selected.
      // For first-time discovery (none are already selected), pre-select supported ones.
      const anyAlreadySelected = data.some((c) => c.is_already_selected)
      const initialSelected = new Set(
        data
          .filter((c) =>
            anyAlreadySelected ? c.is_already_selected : c.arrow_type !== null,
          )
          .map((c) => `${c.schema_name}.${c.table_name}.${c.column_name}`),
      )
      setSelectedColumns(initialSelected)
      setColumnSearch('')
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
        .map((t) => {
          const tableKey = `${t.schema_name}.${t.table_name}`
          const tableCols = discoveredColumns.filter(
            (c) => c.schema_name === t.schema_name && c.table_name === t.table_name,
          )
          return {
            table_name: t.table_name,
            table_type: t.table_type,
            is_selected: selectedTables.has(tableKey),
            columns: tableCols.map((c) => ({
              column_name: c.column_name,
              is_selected: selectedColumns.has(`${tableKey}.${c.column_name}`),
            })),
          }
        }),
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
      setSelectedColumns(new Set())
      setActiveTable(null)
      setSchemaSearch('')
      setTableSearch('')
      setColumnSearch('')
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

  // ---------- toggle helpers ----------

  function toggleSchema(name: string) {
    setSelectedSchemas((prev) => {
      const next = new Set(prev)
      if (next.has(name)) next.delete(name)
      else next.add(name)
      return next
    })
  }

  function toggleAllSchemas(visible: DiscoveredSchemaResponse[]) {
    const visibleNames = visible.map((s) => s.schema_name)
    const allSelected = visibleNames.every((n) => selectedSchemas.has(n))
    setSelectedSchemas((prev) => {
      const next = new Set(prev)
      if (allSelected) {
        visibleNames.forEach((n) => next.delete(n))
      } else {
        visibleNames.forEach((n) => next.add(n))
      }
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

  function toggleAllTablesForSchema(_schemaName: string, visibleTables: DiscoveredTableResponse[]) {
    const visibleKeys = visibleTables.map((t) => `${t.schema_name}.${t.table_name}`)
    const allSelected = visibleKeys.every((k) => selectedTables.has(k))
    setSelectedTables((prev) => {
      const next = new Set(prev)
      if (allSelected) {
        visibleKeys.forEach((k) => next.delete(k))
      } else {
        visibleKeys.forEach((k) => next.add(k))
      }
      return next
    })
  }

  function toggleColumn(key: string) {
    setSelectedColumns((prev) => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }

  function toggleAllColumnsForTable(tableKey: string, visibleCols: DiscoveredColumnResponse[]) {
    // Only toggle supported columns
    const supportedKeys = visibleCols
      .filter((c) => c.arrow_type !== null)
      .map((c) => `${tableKey}.${c.column_name}`)
    const allSelected = supportedKeys.every((k) => selectedColumns.has(k))
    setSelectedColumns((prev) => {
      const next = new Set(prev)
      if (allSelected) {
        supportedKeys.forEach((k) => next.delete(k))
      } else {
        supportedKeys.forEach((k) => next.add(k))
      }
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

  function getTableSelectionStats(tableKey: string) {
    const cols = columnsByTable[tableKey] ?? []
    const total = cols.length
    const supported = cols.filter((c) => c.arrow_type !== null).length
    const selected = cols.filter((c) =>
      c.arrow_type !== null && selectedColumns.has(`${tableKey}.${c.column_name}`),
    ).length
    return { selected, supported, total }
  }

  function getTableCheckState(tableKey: string): 'all' | 'some' | 'none' {
    const cols = columnsByTable[tableKey] ?? []
    const supportedCols = cols.filter((c) => c.arrow_type !== null)
    if (supportedCols.length === 0) return 'none'
    const selectedCount = supportedCols.filter((c) =>
      selectedColumns.has(`${tableKey}.${c.column_name}`),
    ).length
    if (selectedCount === 0) return 'none'
    if (selectedCount === supportedCols.length) return 'all'
    return 'some'
  }

  function selectAllColumns() {
    const allKeys = discoveredColumns
      .filter((c) => c.arrow_type !== null)
      .map((c) => `${c.schema_name}.${c.table_name}.${c.column_name}`)
    setSelectedColumns(new Set(allKeys))
  }

  function deselectAllColumns() {
    setSelectedColumns(new Set())
  }

  if (catalogLoading) {
    return <div className="text-sm text-gray-400">Loading catalog…</div>
  }

  // Filtered lists
  const filteredSchemas = discoveredSchemas.filter((s) =>
    s.schema_name.toLowerCase().includes(schemaSearch.toLowerCase()),
  )

  const allVisibleSchemasSelected =
    filteredSchemas.length > 0 && filteredSchemas.every((s) => selectedSchemas.has(s.schema_name))

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

          {/* Search */}
          <input
            type="text"
            value={schemaSearch}
            onChange={(e) => setSchemaSearch(e.target.value)}
            placeholder="Search schemas…"
            className="w-full mb-2 px-3 py-1.5 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500"
          />

          {/* Select-all */}
          <label className="flex items-center gap-2 px-3 py-1.5 mb-1 text-xs font-medium text-gray-500 cursor-pointer hover:bg-gray-50 rounded-lg">
            <input
              type="checkbox"
              checked={allVisibleSchemasSelected}
              onChange={() => toggleAllSchemas(filteredSchemas)}
              className="rounded border-gray-300 text-indigo-600"
            />
            Select all ({filteredSchemas.length})
          </label>

          <div className="space-y-1 mb-4">
            {filteredSchemas.map((schema) => (
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
            {filteredSchemas.length === 0 && (
              <p className="text-xs text-gray-400 px-3">No schemas match your search.</p>
            )}
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

          {/* Search */}
          <input
            type="text"
            value={tableSearch}
            onChange={(e) => setTableSearch(e.target.value)}
            placeholder="Search tables…"
            className="w-full mb-3 px-3 py-1.5 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500"
          />

          {Array.from(selectedSchemas).map((schemaName) => {
            const allTablesForSchema = discoveredTables.filter((t) => t.schema_name === schemaName)
            const visibleTables = allTablesForSchema.filter((t) =>
              t.table_name.toLowerCase().includes(tableSearch.toLowerCase()),
            )
            const allVisibleSelected =
              visibleTables.length > 0 &&
              visibleTables.every((t) => selectedTables.has(`${t.schema_name}.${t.table_name}`))

            return (
              <div key={schemaName} className="mb-4">
                {/* Schema group header with select-all */}
                <div className="flex items-center gap-2 px-1 mb-1">
                  <input
                    type="checkbox"
                    checked={allVisibleSelected}
                    onChange={() => toggleAllTablesForSchema(schemaName, visibleTables)}
                    disabled={visibleTables.length === 0}
                    className="rounded border-gray-300 text-indigo-600 disabled:opacity-40"
                  />
                  <p className="text-xs font-medium text-gray-500 uppercase tracking-wide">
                    {schemaName}
                  </p>
                </div>
                <div className="space-y-1">
                  {visibleTables.map((table) => {
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
                  {visibleTables.length === 0 && (
                    <p className="text-xs text-gray-400 px-3">
                      {allTablesForSchema.length === 0
                        ? 'No tables found.'
                        : 'No tables match your search.'}
                    </p>
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

      {/* Step 3: Column selection & save */}
      {step === 'columns' && (() => {
        const totalSupportedColumns = discoveredColumns.filter((c) => c.arrow_type !== null).length
        const selectedColumnsCount = discoveredColumns.filter(
          (c) =>
            c.arrow_type !== null &&
            selectedColumns.has(`${c.schema_name}.${c.table_name}.${c.column_name}`),
        ).length
        const tableCount = Object.keys(columnsByTable).length

        return (
          <div>
            {/* Global toolbar */}
            <div className="flex items-center justify-between mb-3">
              <div className="flex gap-3">
                <button
                  onClick={selectAllColumns}
                  className="text-xs text-indigo-600 hover:text-indigo-800 underline"
                >
                  Select All
                </button>
                <button
                  onClick={deselectAllColumns}
                  className="text-xs text-indigo-600 hover:text-indigo-800 underline"
                >
                  Deselect All
                </button>
              </div>
              <span className="text-xs text-gray-500">
                {selectedColumnsCount}/{totalSupportedColumns} cols selected across {tableCount}{' '}
                table{tableCount !== 1 ? 's' : ''}
              </span>
            </div>

            {/* Two-panel layout */}
            <div className="flex border border-gray-200 rounded-lg overflow-hidden min-h-[300px] max-h-[70vh] mb-4">
              {/* Left panel: table list */}
              <div className="w-60 flex-shrink-0 border-r border-gray-200 flex flex-col">
                <div className="p-2 border-b border-gray-100">
                  <input
                    type="text"
                    value={tableSearch}
                    onChange={(e) => setTableSearch(e.target.value)}
                    placeholder="Search tables…"
                    className="w-full px-2 py-1 text-xs border border-gray-300 rounded focus:outline-none focus:ring-1 focus:ring-indigo-500"
                  />
                </div>
                <div className="overflow-y-auto flex-1">
                  {Object.entries(columnsByTable)
                    .filter(([tableKey]) =>
                      tableKey.toLowerCase().includes(tableSearch.toLowerCase()),
                    )
                    .map(([tableKey]) => {
                      const checkState = getTableCheckState(tableKey)
                      const stats = getTableSelectionStats(tableKey)
                      const isActive = tableKey === activeTable
                      const badgeColor =
                        stats.selected > 0 && stats.selected === stats.supported
                          ? 'text-green-600'
                          : stats.selected > 0
                            ? 'text-yellow-600'
                            : 'text-gray-400'
                      return (
                        <div
                          key={tableKey}
                          onClick={() => setActiveTable(tableKey)}
                          className={`flex items-center gap-2 px-2 py-1.5 cursor-pointer border-l-2 ${
                            isActive
                              ? 'bg-indigo-50 border-indigo-500'
                              : 'border-transparent hover:bg-gray-50'
                          }`}
                        >
                          <input
                            type="checkbox"
                            ref={(el) => {
                              if (el) el.indeterminate = checkState === 'some'
                            }}
                            checked={checkState === 'all'}
                            onChange={() =>
                              toggleAllColumnsForTable(tableKey, columnsByTable[tableKey] ?? [])
                            }
                            onClick={(e) => e.stopPropagation()}
                            className="rounded border-gray-300 text-indigo-600 flex-shrink-0"
                          />
                          <span className="text-xs font-mono text-gray-800 truncate flex-1">
                            {tableKey}
                          </span>
                          <span className={`text-xs font-medium flex-shrink-0 ${badgeColor}`}>
                            {stats.selected}/{stats.supported}
                          </span>
                        </div>
                      )
                    })}
                  {Object.keys(columnsByTable).filter((k) =>
                    k.toLowerCase().includes(tableSearch.toLowerCase()),
                  ).length === 0 && (
                    <p className="text-xs text-gray-400 p-3">No tables found.</p>
                  )}
                </div>
              </div>

              {/* Right panel: column detail */}
              <div className="flex-1 flex flex-col overflow-hidden">
                {activeTable ? (
                  <>
                    {/* Right panel header */}
                    <div className="px-3 py-2 border-b border-gray-100 flex items-center justify-between">
                      <span className="text-xs font-semibold font-mono text-gray-800">
                        {activeTable}
                      </span>
                      {(() => {
                        const stats = getTableSelectionStats(activeTable)
                        return (
                          <span className="text-xs text-gray-500">
                            {stats.selected}/{stats.supported} selected
                          </span>
                        )
                      })()}
                    </div>

                    {/* Select all + column search row */}
                    <div className="px-3 py-1.5 border-b border-gray-100 flex items-center gap-3">
                      {(() => {
                        const checkState = getTableCheckState(activeTable)
                        return (
                          <label className="flex items-center gap-2 cursor-pointer">
                            <input
                              type="checkbox"
                              ref={(el) => {
                                if (el) el.indeterminate = checkState === 'some'
                              }}
                              checked={checkState === 'all'}
                              onChange={() =>
                                toggleAllColumnsForTable(
                                  activeTable,
                                  columnsByTable[activeTable] ?? [],
                                )
                              }
                              className="rounded border-gray-300 text-indigo-600"
                            />
                            <span className="text-xs text-gray-600">Select all supported</span>
                          </label>
                        )
                      })()}
                      <input
                        type="text"
                        value={columnSearch}
                        onChange={(e) => setColumnSearch(e.target.value)}
                        placeholder="Search columns…"
                        className="ml-auto px-2 py-0.5 text-xs border border-gray-300 rounded focus:outline-none focus:ring-1 focus:ring-indigo-500"
                      />
                    </div>

                    {/* Column table */}
                    <div className="overflow-y-auto flex-1">
                      <table className="w-full text-xs border-collapse">
                        <thead className="sticky top-0 bg-white">
                          <tr className="text-gray-500 text-left border-b border-gray-100">
                            <th className="py-1 pr-2 pl-3 font-medium w-6"></th>
                            <th className="py-1 pr-3 font-medium">Column</th>
                            <th className="py-1 pr-3 font-medium">Type</th>
                            <th className="py-1 pr-3 font-medium">Arrow</th>
                            <th className="py-1 pr-3 font-medium">Nullable</th>
                          </tr>
                        </thead>
                        <tbody>
                          {(columnsByTable[activeTable] ?? [])
                            .slice()
                            .sort((a, b) => a.ordinal_position - b.ordinal_position)
                            .filter((c) =>
                              c.column_name.toLowerCase().includes(columnSearch.toLowerCase()),
                            )
                            .map((col) => {
                              const colKey = `${activeTable}.${col.column_name}`
                              const unsupported = col.arrow_type === null
                              return (
                                <tr
                                  key={col.column_name}
                                  className={`border-b border-gray-50 ${unsupported ? 'opacity-40' : ''}`}
                                >
                                  <td className="py-0.5 pr-2 pl-3">
                                    <input
                                      type="checkbox"
                                      checked={!unsupported && selectedColumns.has(colKey)}
                                      disabled={unsupported}
                                      onChange={() => !unsupported && toggleColumn(colKey)}
                                      className="rounded border-gray-300 text-indigo-600 disabled:opacity-40"
                                    />
                                  </td>
                                  <td className="py-0.5 pr-3 font-mono text-gray-900">
                                    {col.column_name}
                                  </td>
                                  <td className="py-0.5 pr-3 text-gray-600">{col.data_type}</td>
                                  <td className="py-0.5 pr-3 text-gray-500">
                                    {col.arrow_type ?? (
                                      <span className="text-yellow-600">⚠ unsup</span>
                                    )}
                                  </td>
                                  <td className="py-0.5 pr-3 text-gray-500">
                                    {col.is_nullable ? 'yes' : 'no'}
                                  </td>
                                </tr>
                              )
                            })}
                          {(columnsByTable[activeTable] ?? []).filter((c) =>
                            c.column_name.toLowerCase().includes(columnSearch.toLowerCase()),
                          ).length === 0 && (
                            <tr>
                              <td colSpan={5} className="py-2 pl-3 text-gray-400 italic">
                                No columns match your search.
                              </td>
                            </tr>
                          )}
                        </tbody>
                      </table>
                    </div>
                  </>
                ) : (
                  <div className="flex-1 flex items-center justify-center">
                    <p className="text-sm text-gray-400">Select a table from the list</p>
                  </div>
                )}
              </div>
            </div>

            {/* Action buttons */}
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
        )
      })()}

      {error && (
        <div className="mt-3 p-2 bg-red-50 border border-red-200 rounded text-sm text-red-700">
          {error}
        </div>
      )}
    </div>
  )
}
