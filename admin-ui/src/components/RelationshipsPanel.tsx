import { useEffect, useMemo, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useSearchParams } from 'react-router-dom'
import toast from 'react-hot-toast'
import {
  createColumnAnchor,
  createRelationship,
  deleteColumnAnchor,
  deleteRelationship,
  getCatalog,
  listColumnAnchors,
  listFkSuggestions,
  listRelationships,
} from '../api/catalog'
import type {
  CatalogResponse,
  CatalogTableResponse,
  ColumnAnchor,
  FkSuggestion,
  TableRelationship,
} from '../types/catalog'
import { ConfirmDialog } from './ConfirmDialog'

/// Format a flat "schema.table" label derived from the catalog.
function tableLabel(schemaName: string, tableName: string): string {
  return `${schemaName}.${tableName}`
}

/// Flatten all selected (schema, table, table_id) tuples from the catalog, sorted by label.
function flattenTables(catalog?: CatalogResponse) {
  if (!catalog) return []
  const out: Array<{ id: string; schema: string; table: string; columns: CatalogTableResponse['columns'] }> = []
  for (const s of catalog.schemas) {
    for (const t of s.tables) {
      out.push({ id: t.id, schema: s.schema_name, table: t.table_name, columns: t.columns })
    }
  }
  return out.sort((a, b) => tableLabel(a.schema, a.table).localeCompare(tableLabel(b.schema, b.table)))
}

type CatalogTables = ReturnType<typeof flattenTables>

function errorMessage(err: unknown, fallback: string): string {
  return (err as { response?: { data?: { error?: string } } })?.response?.data?.error ?? fallback
}

// ---------- Public: Relationships section ----------

/// Datasource edit page section for admin-curated `table_relationship` CRUD.
/// Surfaces live FK candidates and a manual add form. Self-contained — does its
/// own catalog + relationships fetches.
export function RelationshipsSection({ datasourceId }: { datasourceId: string }) {
  const queryClient = useQueryClient()
  const [showSuggestions, setShowSuggestions] = useState(false)
  const [showManualForm, setShowManualForm] = useState(false)
  const [relToDelete, setRelToDelete] = useState<TableRelationship | null>(null)

  const { data: catalog } = useQuery({
    queryKey: ['catalog', datasourceId],
    queryFn: () => getCatalog(datasourceId),
  })
  const { data: relationships = [], isLoading } = useQuery({
    queryKey: ['relationships', datasourceId],
    queryFn: () => listRelationships(datasourceId),
  })

  const catalogTables = useMemo(() => flattenTables(catalog), [catalog])

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['relationships', datasourceId] })
    queryClient.invalidateQueries({ queryKey: ['column-anchors', datasourceId] })
    queryClient.invalidateQueries({ queryKey: ['fk-suggestions', datasourceId] })
  }

  const deleteRelMutation = useMutation({
    mutationFn: (relId: string) => deleteRelationship(datasourceId, relId),
    onSuccess: () => {
      toast.success('Relationship deleted')
      invalidate()
      setRelToDelete(null)
    },
    onError: (err) => toast.error(errorMessage(err, 'Failed to delete relationship')),
  })

  return (
    <div>
      <div className="mb-3">
        <h2 className="text-base font-semibold text-gray-900">Relationships</h2>
        <p className="text-xs text-gray-400 mt-0.5">
          Admin-curated join paths for row-filter policies. Pick from live FK suggestions or add
          manually.
        </p>
      </div>

      <div className="flex items-center justify-end gap-2 mb-2">
        <button
          type="button"
          onClick={() => setShowSuggestions((v) => !v)}
          className="text-xs px-2 py-1 rounded border border-gray-200 hover:bg-gray-50"
        >
          {showSuggestions ? 'Hide suggestions' : 'Suggest from database FKs'}
        </button>
        <button
          type="button"
          onClick={() => setShowManualForm((v) => !v)}
          className="text-xs px-2 py-1 rounded bg-blue-600 text-white hover:bg-blue-700"
        >
          {showManualForm ? 'Cancel' : '+ Add manual relationship'}
        </button>
      </div>

      {showManualForm && (
        <ManualRelationshipForm
          datasourceId={datasourceId}
          catalogTables={catalogTables}
          onDone={() => {
            setShowManualForm(false)
            invalidate()
          }}
        />
      )}

      {showSuggestions && (
        <FkSuggestionsList datasourceId={datasourceId} onMutated={invalidate} />
      )}

      {isLoading ? (
        <div className="text-xs text-gray-400">Loading…</div>
      ) : relationships.length === 0 ? (
        <div className="text-xs text-gray-400 italic">No relationships configured.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 text-xs text-gray-500">
              <tr>
                <th className="text-left px-3 py-1.5 font-medium">Child → Parent</th>
                <th className="text-right px-3 py-1.5 font-medium"></th>
              </tr>
            </thead>
            <tbody>
              {relationships.map((r) => (
                <tr key={r.id} className="border-t border-gray-100 align-top">
                  <td className="px-3 py-1.5 text-gray-800 font-mono text-xs">
                    <div>
                      {tableLabel(r.child_schema_name, r.child_table_name)}.{r.child_column_name}
                    </div>
                    <div className="text-gray-400">
                      → {tableLabel(r.parent_schema_name, r.parent_table_name)}.{r.parent_column_name}
                    </div>
                  </td>
                  <td className="px-3 py-1.5 text-right whitespace-nowrap">
                    <button
                      type="button"
                      onClick={() => setRelToDelete(r)}
                      disabled={deleteRelMutation.isPending}
                      className="text-xs text-red-600 hover:underline disabled:opacity-50"
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {relToDelete && (
        <ConfirmDialog
          title="Delete this relationship?"
          message={
            <>
              Removes the join path{' '}
              <code className="font-mono text-xs">
                {tableLabel(relToDelete.child_schema_name, relToDelete.child_table_name)}.
                {relToDelete.child_column_name}
              </code>
              {' → '}
              <code className="font-mono text-xs">
                {tableLabel(relToDelete.parent_schema_name, relToDelete.parent_table_name)}.
                {relToDelete.parent_column_name}
              </code>
              .
            </>
          }
          confirmLabel="Delete"
          confirmPendingLabel="Deleting…"
          pending={deleteRelMutation.isPending}
          onConfirm={() => deleteRelMutation.mutate(relToDelete.id)}
          onCancel={() => setRelToDelete(null)}
        />
      )}
    </div>
  )
}

// ---------- Public: Column anchors section ----------

interface FocusTarget {
  schema: string
  table: string
  column: string
}

function parseFocus(raw: string | null): FocusTarget | null {
  if (!raw) return null
  const parts = raw.split('.')
  if (parts.length !== 3) return null
  const [schema, table, column] = parts
  if (!schema || !table || !column) return null
  return { schema, table, column }
}

/// Datasource edit page section for `column_anchor` CRUD. Reads `?focus=schema.table.column`
/// from the URL; when present, scrolls + highlights the matching anchor row, or pre-opens
/// the Add form with those values if no anchor exists yet. The URL param is stripped after
/// first consumption so refresh doesn't re-trigger.
export function ColumnAnchorsSection({ datasourceId }: { datasourceId: string }) {
  const queryClient = useQueryClient()
  const [searchParams, setSearchParams] = useSearchParams()
  const [showForm, setShowForm] = useState(false)
  const [prefill, setPrefill] = useState<{
    childTableId: string
    resolvedColumn: string
  } | null>(null)
  const [highlightedId, setHighlightedId] = useState<string | null>(null)
  const rowRefs = useRef<Map<string, HTMLTableRowElement | null>>(new Map())
  const focusHandledRef = useRef<string | null>(null)

  const focusRaw = searchParams.get('focus')
  const focus = useMemo(() => parseFocus(focusRaw), [focusRaw])

  const { data: catalog } = useQuery({
    queryKey: ['catalog', datasourceId],
    queryFn: () => getCatalog(datasourceId),
  })
  const { data: relationships = [] } = useQuery({
    queryKey: ['relationships', datasourceId],
    queryFn: () => listRelationships(datasourceId),
  })
  const { data: anchors = [], isSuccess: anchorsLoaded } = useQuery({
    queryKey: ['column-anchors', datasourceId],
    queryFn: () => listColumnAnchors(datasourceId),
  })

  const catalogTables = useMemo(() => flattenTables(catalog), [catalog])

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['relationships', datasourceId] })
    queryClient.invalidateQueries({ queryKey: ['column-anchors', datasourceId] })
  }

  const clearFocusParam = () => {
    const next = new URLSearchParams(searchParams)
    next.delete('focus')
    setSearchParams(next, { replace: true })
  }

  // Handle ?focus= once per unique value, after queries resolve.
  useEffect(() => {
    if (!focus || !focusRaw) return
    if (!anchorsLoaded || !catalog) return
    if (focusHandledRef.current === focusRaw) return

    const table = catalogTables.find(
      (t) => t.schema === focus.schema && t.table === focus.table,
    )
    const match = table
      ? anchors.find(
          (a) =>
            a.child_table_id === table.id && a.resolved_column_name === focus.column,
        )
      : undefined

    if (match) {
      const row = rowRefs.current.get(match.id)
      row?.scrollIntoView({ block: 'center', behavior: 'smooth' })
      setHighlightedId(match.id)
      const t = setTimeout(() => setHighlightedId(null), 1500)
      focusHandledRef.current = focusRaw
      clearFocusParam()
      return () => clearTimeout(t)
    }

    if (table) {
      setPrefill({ childTableId: table.id, resolvedColumn: focus.column })
      setShowForm(true)
    } else {
      toast.error(
        `No selected table named ${focus.schema}.${focus.table} — check the catalog first.`,
      )
    }
    focusHandledRef.current = focusRaw
    clearFocusParam()
    // `searchParams` and `setSearchParams` deliberately omitted from deps to
    // avoid re-firing when we strip the param ourselves.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, focusRaw, anchorsLoaded, catalog, anchors, catalogTables])

  const [anchorToDelete, setAnchorToDelete] = useState<ColumnAnchor | null>(null)

  const deleteAnchorMutation = useMutation({
    mutationFn: (id: string) => deleteColumnAnchor(datasourceId, id),
    onSuccess: () => {
      toast.success('Anchor deleted')
      invalidate()
      setAnchorToDelete(null)
    },
    onError: (err) => toast.error(errorMessage(err, 'Failed to delete anchor')),
  })

  const relationshipsByChild = useMemo(() => {
    const map = new Map<string, TableRelationship[]>()
    for (const r of relationships) {
      const arr = map.get(r.child_table_id) ?? []
      arr.push(r)
      map.set(r.child_table_id, arr)
    }
    return map
  }, [relationships])

  return (
    <div>
      <div className="flex items-center justify-between mb-2">
        <div>
          <h2 className="text-base font-semibold text-gray-900">Column anchors</h2>
          <p className="text-xs text-gray-400 mt-0.5">
            For each <code className="bg-gray-100 px-1 rounded">(child_table, resolved_column)</code>,
            pick which relationship resolves it during row-filter rewriting.
          </p>
        </div>
        <button
          type="button"
          onClick={() => {
            setShowForm((v) => !v)
            if (showForm) setPrefill(null)
          }}
          className="text-xs px-2 py-1 rounded bg-blue-600 text-white hover:bg-blue-700"
        >
          {showForm ? 'Cancel' : '+ Add column anchor'}
        </button>
      </div>

      {showForm && (
        <ColumnAnchorForm
          datasourceId={datasourceId}
          relationshipsByChild={relationshipsByChild}
          catalogTables={catalogTables}
          prefill={prefill}
          onDone={() => {
            setShowForm(false)
            setPrefill(null)
            invalidate()
          }}
        />
      )}

      {anchors.length === 0 ? (
        <div className="text-xs text-gray-400 italic">No column anchors configured.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 text-xs text-gray-500">
              <tr>
                <th className="text-left px-3 py-1.5 font-medium">Anchor</th>
                <th className="text-left px-3 py-1.5 font-medium">Via relationship</th>
                <th className="text-right px-3 py-1.5 font-medium"></th>
              </tr>
            </thead>
            <tbody>
              {anchors.map((a) => (
                <AnchorRow
                  key={a.id}
                  anchor={a}
                  relationships={relationships}
                  highlighted={highlightedId === a.id}
                  onDelete={() => setAnchorToDelete(a)}
                  deleting={deleteAnchorMutation.isPending}
                  rowRef={(el) => rowRefs.current.set(a.id, el)}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {anchorToDelete && (
        <ConfirmDialog
          title="Delete this anchor?"
          message="Removes the column anchor. Row-filter policies that referenced this column may no longer resolve."
          confirmLabel="Delete"
          confirmPendingLabel="Deleting…"
          pending={deleteAnchorMutation.isPending}
          onConfirm={() => deleteAnchorMutation.mutate(anchorToDelete.id)}
          onCancel={() => setAnchorToDelete(null)}
        />
      )}
    </div>
  )
}

// ---------- Private helpers ----------

function AnchorRow({
  anchor,
  relationships,
  highlighted,
  onDelete,
  deleting,
  rowRef,
}: {
  anchor: ColumnAnchor
  relationships: TableRelationship[]
  highlighted: boolean
  onDelete: () => void
  deleting: boolean
  rowRef: (el: HTMLTableRowElement | null) => void
}) {
  const rel = anchor.relationship_id
    ? relationships.find((r) => r.id === anchor.relationship_id)
    : undefined
  const highlightClass = highlighted
    ? 'ring-2 ring-amber-400 ring-offset-1 transition-shadow'
    : 'transition-shadow'
  return (
    <tr
      ref={rowRef}
      data-testid={`anchor-row-${anchor.id}`}
      className={`border-t border-gray-100 align-top ${highlightClass}`}
    >
      <td className="px-3 py-1.5 text-gray-800 font-mono text-xs">
        <div>{anchor.child_table_name}</div>
        <div className="text-gray-500">.{anchor.resolved_column_name}</div>
      </td>
      <td className="px-3 py-1.5 text-gray-800 font-mono text-xs">
        {anchor.actual_column_name ? (
          <>
            <div className="inline-block text-[10px] uppercase tracking-wide text-indigo-500 bg-indigo-50 border border-indigo-100 rounded px-1 mb-0.5">
              alias
            </div>
            <div className="text-gray-400">→ .{anchor.actual_column_name}</div>
          </>
        ) : rel ? (
          <>
            <div>{rel.child_column_name}</div>
            <div className="text-gray-400">
              → {tableLabel(rel.parent_schema_name, rel.parent_table_name)}.{rel.parent_column_name}
            </div>
          </>
        ) : (
          <span className="text-gray-400 italic">(deleted)</span>
        )}
      </td>
      <td className="px-3 py-1.5 text-right whitespace-nowrap">
        <button
          type="button"
          onClick={onDelete}
          disabled={deleting}
          className="text-xs text-red-600 hover:underline disabled:opacity-50"
        >
          Delete
        </button>
      </td>
    </tr>
  )
}

function ManualRelationshipForm({
  datasourceId,
  catalogTables,
  onDone,
}: {
  datasourceId: string
  catalogTables: CatalogTables
  onDone: () => void
}) {
  const [childTableId, setChildTableId] = useState('')
  const [childColumnName, setChildColumnName] = useState('')
  const [parentTableId, setParentTableId] = useState('')
  const [parentColumnName, setParentColumnName] = useState('')

  const childTable = catalogTables.find((t) => t.id === childTableId)
  const parentTable = catalogTables.find((t) => t.id === parentTableId)

  const createMutation = useMutation({
    mutationFn: () =>
      createRelationship(datasourceId, {
        child_table_id: childTableId,
        child_column_name: childColumnName,
        parent_table_id: parentTableId,
        parent_column_name: parentColumnName,
      }),
    onSuccess: () => {
      toast.success('Relationship added')
      setChildTableId('')
      setChildColumnName('')
      setParentTableId('')
      setParentColumnName('')
      onDone()
    },
    onError: (err) => toast.error(errorMessage(err, 'Failed to add relationship')),
  })

  const canSubmit =
    !!childTableId && !!childColumnName && !!parentTableId && !!parentColumnName

  return (
    <div className="mb-3 border border-gray-200 rounded-lg p-3 bg-gray-50">
      <div className="grid grid-cols-2 gap-3 text-sm">
        <div>
          <label className="block text-xs text-gray-500 mb-1">Child table</label>
          <select
            value={childTableId}
            onChange={(e) => {
              setChildTableId(e.target.value)
              setChildColumnName('')
            }}
            className="w-full border border-gray-300 rounded px-2 py-1"
          >
            <option value="">Select…</option>
            {catalogTables.map((t) => (
              <option key={t.id} value={t.id}>
                {tableLabel(t.schema, t.table)}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="block text-xs text-gray-500 mb-1">Child column (FK)</label>
          <select
            value={childColumnName}
            onChange={(e) => setChildColumnName(e.target.value)}
            disabled={!childTable}
            className="w-full border border-gray-300 rounded px-2 py-1"
          >
            <option value="">Select…</option>
            {childTable?.columns.map((c) => (
              <option key={c.id} value={c.column_name}>
                {c.column_name}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="block text-xs text-gray-500 mb-1">Parent table</label>
          <select
            value={parentTableId}
            onChange={(e) => {
              setParentTableId(e.target.value)
              setParentColumnName('')
            }}
            className="w-full border border-gray-300 rounded px-2 py-1"
          >
            <option value="">Select…</option>
            {catalogTables.map((t) => (
              <option key={t.id} value={t.id}>
                {tableLabel(t.schema, t.table)}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="block text-xs text-gray-500 mb-1">
            Parent column (PK or single-column unique)
          </label>
          <select
            value={parentColumnName}
            onChange={(e) => setParentColumnName(e.target.value)}
            disabled={!parentTable}
            className="w-full border border-gray-300 rounded px-2 py-1"
          >
            <option value="">Select…</option>
            {parentTable?.columns.map((c) => (
              <option key={c.id} value={c.column_name}>
                {c.column_name}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="flex justify-end gap-2 mt-3">
        <button
          type="button"
          onClick={() => createMutation.mutate()}
          disabled={!canSubmit || createMutation.isPending}
          className="text-xs bg-blue-600 text-white px-3 py-1.5 rounded hover:bg-blue-700 disabled:opacity-50"
        >
          {createMutation.isPending ? 'Saving…' : 'Save relationship'}
        </button>
      </div>
    </div>
  )
}

function FkSuggestionsList({
  datasourceId,
  onMutated,
}: {
  datasourceId: string
  onMutated: () => void
}) {
  const { data: suggestions, isLoading } = useQuery({
    queryKey: ['fk-suggestions', datasourceId],
    queryFn: () => listFkSuggestions(datasourceId),
  })

  const addMutation = useMutation({
    mutationFn: (s: FkSuggestion) =>
      createRelationship(datasourceId, {
        child_table_id: s.child_table_id,
        child_column_name: s.child_column_name,
        parent_table_id: s.parent_table_id,
        parent_column_name: s.parent_column_name,
      }),
    onSuccess: () => {
      toast.success('Relationship added')
      onMutated()
    },
    onError: (err) => toast.error(errorMessage(err, 'Failed to add relationship from suggestion')),
  })

  if (isLoading) {
    return <div className="text-xs text-gray-400 mb-3">Loading suggestions…</div>
  }

  const list = suggestions ?? []
  const pending = list.filter((s) => !s.already_added)
  if (pending.length === 0) {
    return (
      <div className="text-xs text-gray-400 mb-3 italic">
        No new FK suggestions — all discovered foreign keys are already in your relationships list.
      </div>
    )
  }

  return (
    <div className="mb-3 border border-gray-200 rounded-lg overflow-x-auto">
      <table className="w-full text-sm">
        <thead className="bg-gray-50 text-xs text-gray-500">
          <tr>
            <th className="text-left px-3 py-1.5 font-medium">FK suggestion</th>
            <th className="text-right px-3 py-1.5 font-medium"></th>
          </tr>
        </thead>
        <tbody>
          {pending.map((s) => (
            <tr
              key={`${s.child_table_id}.${s.child_column_name}→${s.parent_table_id}.${s.parent_column_name}`}
              className="border-t border-gray-100 align-top"
            >
              <td className="px-3 py-1.5 text-gray-800 font-mono text-xs">
                <div>
                  {tableLabel(s.child_schema_name, s.child_table_name)}.{s.child_column_name}
                </div>
                <div className="text-gray-400">
                  → {tableLabel(s.parent_schema_name, s.parent_table_name)}.{s.parent_column_name}
                </div>
                <div className="text-[10px] text-gray-400 mt-0.5">{s.fk_constraint_name}</div>
              </td>
              <td className="px-3 py-1.5 text-right whitespace-nowrap">
                <button
                  type="button"
                  onClick={() => addMutation.mutate(s)}
                  disabled={addMutation.isPending}
                  className="text-xs text-blue-600 hover:underline disabled:opacity-50"
                >
                  Add
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

type AnchorMode = 'relationship' | 'alias'

function ColumnAnchorForm({
  datasourceId,
  relationshipsByChild,
  catalogTables,
  prefill,
  onDone,
}: {
  datasourceId: string
  relationshipsByChild: Map<string, TableRelationship[]>
  catalogTables: CatalogTables
  prefill: { childTableId: string; resolvedColumn: string } | null
  onDone: () => void
}) {
  const [mode, setMode] = useState<AnchorMode>('relationship')
  const [childTableId, setChildTableId] = useState(prefill?.childTableId ?? '')
  const [resolvedColumn, setResolvedColumn] = useState(prefill?.resolvedColumn ?? '')
  const [relationshipId, setRelationshipId] = useState('')
  const [actualColumn, setActualColumn] = useState('')

  const candidateRelationships = childTableId
    ? (relationshipsByChild.get(childTableId) ?? [])
    : []
  const childTable = catalogTables.find((t) => t.id === childTableId)

  const createMutation = useMutation({
    mutationFn: () =>
      createColumnAnchor(
        datasourceId,
        mode === 'relationship'
          ? {
              child_table_id: childTableId,
              resolved_column_name: resolvedColumn,
              relationship_id: relationshipId,
            }
          : {
              child_table_id: childTableId,
              resolved_column_name: resolvedColumn,
              actual_column_name: actualColumn,
            },
      ),
    onSuccess: () => {
      toast.success('Anchor added')
      setChildTableId('')
      setResolvedColumn('')
      setRelationshipId('')
      setActualColumn('')
      onDone()
    },
    onError: (err) => toast.error(errorMessage(err, 'Failed to add anchor')),
  })

  const canSubmit =
    !!childTableId &&
    !!resolvedColumn &&
    (mode === 'relationship' ? !!relationshipId : !!actualColumn.trim())

  return (
    <div className="mb-3 border border-gray-200 rounded-lg p-3 bg-gray-50">
      <div className="mb-3 flex items-center gap-3 text-xs text-gray-600">
        <span className="font-medium text-gray-500">Resolve via</span>
        <label className="flex items-center gap-1 cursor-pointer">
          <input
            type="radio"
            name="anchor-mode"
            value="relationship"
            checked={mode === 'relationship'}
            onChange={() => setMode('relationship')}
          />
          <span>Relationship (FK walk)</span>
        </label>
        <label className="flex items-center gap-1 cursor-pointer">
          <input
            type="radio"
            name="anchor-mode"
            value="alias"
            checked={mode === 'alias'}
            onChange={() => setMode('alias')}
          />
          <span>Same-table alias</span>
        </label>
      </div>

      <div className="grid grid-cols-3 gap-3 text-sm">
        <div>
          <label className="block text-xs text-gray-500 mb-1">Child table</label>
          <select
            value={childTableId}
            onChange={(e) => {
              setChildTableId(e.target.value)
              setRelationshipId('')
              setActualColumn('')
            }}
            className="w-full border border-gray-300 rounded px-2 py-1"
          >
            <option value="">Select…</option>
            {catalogTables.map((t) => (
              <option key={t.id} value={t.id}>
                {tableLabel(t.schema, t.table)}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="block text-xs text-gray-500 mb-1">Resolved column name</label>
          <input
            type="text"
            value={resolvedColumn}
            onChange={(e) => setResolvedColumn(e.target.value)}
            placeholder="e.g. tenant_id"
            className="w-full border border-gray-300 rounded px-2 py-1"
          />
        </div>
        {mode === 'relationship' ? (
          <div>
            <label className="block text-xs text-gray-500 mb-1">Via relationship</label>
            <select
              value={relationshipId}
              onChange={(e) => setRelationshipId(e.target.value)}
              disabled={!childTableId}
              className="w-full border border-gray-300 rounded px-2 py-1"
            >
              <option value="">Select…</option>
              {candidateRelationships.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.child_column_name} → {tableLabel(r.parent_schema_name, r.parent_table_name)}.
                  {r.parent_column_name}
                </option>
              ))}
            </select>
            {childTableId && candidateRelationships.length === 0 && (
              <div className="mt-1 text-xs text-amber-600">
                No relationships from this child table yet. Add one in the Relationships section
                first.
              </div>
            )}
          </div>
        ) : (
          <div>
            <label className="block text-xs text-gray-500 mb-1">
              Actual column on this table
            </label>
            <input
              type="text"
              list={`actual-col-options-${childTableId}`}
              value={actualColumn}
              onChange={(e) => setActualColumn(e.target.value)}
              disabled={!childTableId}
              placeholder="e.g. org_id"
              className="w-full border border-gray-300 rounded px-2 py-1 disabled:bg-gray-100"
            />
            {childTable && (
              <datalist id={`actual-col-options-${childTableId}`}>
                {childTable.columns.map((c) => (
                  <option key={c.id} value={c.column_name} />
                ))}
              </datalist>
            )}
            <div className="mt-1 text-[11px] text-gray-500">
              Row filters referencing <code>{resolvedColumn || 'resolved_column'}</code> will be
              rewritten to use this column on this table — no join.
            </div>
          </div>
        )}
      </div>
      <div className="flex justify-end gap-2 mt-3">
        <button
          type="button"
          onClick={() => createMutation.mutate()}
          disabled={!canSubmit || createMutation.isPending}
          className="text-xs bg-blue-600 text-white px-3 py-1.5 rounded hover:bg-blue-700 disabled:opacity-50"
        >
          {createMutation.isPending ? 'Saving…' : 'Save anchor'}
        </button>
      </div>
    </div>
  )
}
