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
import { effectiveSchemaName } from '../utils/schemaLabel'

/// Format a flat "schema.table" label. `schema` is expected to already be the
/// effective name (alias if set, else raw upstream).
function tableLabel(schema: string, tableName: string): string {
  return `${schema}.${tableName}`
}

interface FlatTable {
  id: string
  schema: string
  schema_upstream: string
  table: string
  columns: CatalogTableResponse['columns']
}

/// Flatten all selected (schema, table, table_id) tuples from the catalog, sorted
/// by effective label. `schema` is the user-facing name (alias if set);
/// `schema_upstream` is the raw upstream name carried for parenthetical display.
function flattenTables(catalog?: CatalogResponse): FlatTable[] {
  if (!catalog) return []
  const out: FlatTable[] = []
  for (const s of catalog.schemas) {
    const schema = effectiveSchemaName(s.schema_name, s.schema_alias)
    for (const t of s.tables) {
      out.push({
        id: t.id,
        schema,
        schema_upstream: s.schema_name,
        table: t.table_name,
        columns: t.columns,
      })
    }
  }
  return out.sort((a, b) =>
    tableLabel(a.schema, a.table).localeCompare(tableLabel(b.schema, b.table)),
  )
}

type CatalogTables = FlatTable[]

/// Build a raw-schema-name → alias lookup so we can resolve effective names
/// for relationship/FK-suggestion rows, which carry only the upstream name.
function buildAliasMap(catalog?: CatalogResponse): Map<string, string | null> {
  const map = new Map<string, string | null>()
  if (!catalog) return map
  for (const s of catalog.schemas) {
    map.set(s.schema_name, s.schema_alias)
  }
  return map
}

function resolveEffective(
  rawSchemaName: string,
  aliasMap: Map<string, string | null>,
): string {
  return effectiveSchemaName(rawSchemaName, aliasMap.get(rawSchemaName) ?? null)
}

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
  const aliasMap = useMemo(() => buildAliasMap(catalog), [catalog])

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
        <FkSuggestionsList
          datasourceId={datasourceId}
          aliasMap={aliasMap}
          onMutated={invalidate}
        />
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
                      {tableLabel(resolveEffective(r.child_schema_name, aliasMap), r.child_table_name)}.{r.child_column_name}
                    </div>
                    <div className="text-gray-400">
                      → {tableLabel(resolveEffective(r.parent_schema_name, aliasMap), r.parent_table_name)}.{r.parent_column_name}
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
                {tableLabel(resolveEffective(relToDelete.child_schema_name, aliasMap), relToDelete.child_table_name)}.
                {relToDelete.child_column_name}
              </code>
              {' → '}
              <code className="font-mono text-xs">
                {tableLabel(resolveEffective(relToDelete.parent_schema_name, aliasMap), relToDelete.parent_table_name)}.
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
    queryClient.invalidateQueries({ queryKey: ['fk-suggestions', datasourceId] })
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

  const aliasMap = useMemo(() => buildAliasMap(catalog), [catalog])

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
          aliasMap={aliasMap}
          prefill={prefill}
          onDone={() => {
            setShowForm(false)
            setPrefill(null)
            invalidate()
          }}
          invalidate={invalidate}
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
                <th className="text-left px-3 py-1.5 font-medium">Resolves via</th>
                <th className="text-right px-3 py-1.5 font-medium"></th>
              </tr>
            </thead>
            <tbody>
              {anchors.map((a) => (
                <AnchorRow
                  key={a.id}
                  anchor={a}
                  relationships={relationships}
                  catalogTables={catalogTables}
                  aliasMap={aliasMap}
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
  catalogTables,
  aliasMap,
  highlighted,
  onDelete,
  deleting,
  rowRef,
}: {
  anchor: ColumnAnchor
  relationships: TableRelationship[]
  catalogTables: CatalogTables
  aliasMap: Map<string, string | null>
  highlighted: boolean
  onDelete: () => void
  deleting: boolean
  rowRef: (el: HTMLTableRowElement | null) => void
}) {
  const rel = anchor.relationship_id
    ? relationships.find((r) => r.id === anchor.relationship_id)
    : undefined
  const childTable = catalogTables.find((t) => t.id === anchor.child_table_id)
  const childSchema = childTable?.schema ?? anchor.child_table_name
  const childTableName = childTable?.table ?? anchor.child_table_name
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
        <div>{tableLabel(childSchema, childTableName)}</div>
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
            <div>
              {tableLabel(resolveEffective(rel.child_schema_name, aliasMap), rel.child_table_name)}.
              {rel.child_column_name}
            </div>
            <div className="text-gray-400">
              → {tableLabel(resolveEffective(rel.parent_schema_name, aliasMap), rel.parent_table_name)}.
              {rel.parent_column_name}
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
                {t.schema !== t.schema_upstream
                  ? `  (${t.schema_upstream}.${t.table})`
                  : ''}
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
                {t.schema !== t.schema_upstream
                  ? `  (${t.schema_upstream}.${t.table})`
                  : ''}
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
  aliasMap,
  onMutated,
  childTableIdFilter,
  emptyMessage,
  onAdded,
}: {
  datasourceId: string
  aliasMap: Map<string, string | null>
  onMutated: () => void
  /// When set, only show suggestions whose `child_table_id` matches.
  childTableIdFilter?: string
  /// Override the empty-state message (used by the in-anchor empty-state block).
  emptyMessage?: string
  /// Optional callback fired with the newly created relationship after Add.
  onAdded?: (rel: TableRelationship) => void
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
    onSuccess: (rel) => {
      toast.success('Relationship added')
      onMutated()
      onAdded?.(rel)
    },
    onError: (err) => toast.error(errorMessage(err, 'Failed to add relationship from suggestion')),
  })

  if (isLoading) {
    return <div className="text-xs text-gray-400 mb-3">Loading suggestions…</div>
  }

  const list = suggestions ?? []
  const pending = list
    .filter((s) => !s.already_added)
    .filter((s) => !childTableIdFilter || s.child_table_id === childTableIdFilter)

  if (pending.length === 0) {
    return (
      <div className="text-xs text-gray-400 mb-3 italic">
        {emptyMessage ??
          'No new FK suggestions — all discovered foreign keys are already in your relationships list.'}
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
                  {tableLabel(resolveEffective(s.child_schema_name, aliasMap), s.child_table_name)}.
                  {s.child_column_name}
                </div>
                <div className="text-gray-400">
                  → {tableLabel(resolveEffective(s.parent_schema_name, aliasMap), s.parent_table_name)}.
                  {s.parent_column_name}
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

/// Determine whether a relationship's parent table contains a column matching
/// the resolved column name — i.e., whether walking that FK would actually
/// resolve the policy reference. Used to surface the most useful candidate.
function relationshipIsViable(
  rel: TableRelationship,
  resolvedColumn: string,
  catalogTables: CatalogTables,
): boolean {
  if (!resolvedColumn.trim()) return false
  const parent = catalogTables.find(
    (t) =>
      t.schema_upstream === rel.parent_schema_name && t.table === rel.parent_table_name,
  )
  if (!parent) return false
  return parent.columns.some((c) => c.column_name === resolvedColumn)
}

function ColumnAnchorForm({
  datasourceId,
  relationshipsByChild,
  catalogTables,
  aliasMap,
  prefill,
  onDone,
  invalidate,
}: {
  datasourceId: string
  relationshipsByChild: Map<string, TableRelationship[]>
  catalogTables: CatalogTables
  aliasMap: Map<string, string | null>
  prefill: { childTableId: string; resolvedColumn: string } | null
  onDone: () => void
  invalidate: () => void
}) {
  const [mode, setMode] = useState<AnchorMode>('relationship')
  const [childTableId, setChildTableId] = useState(prefill?.childTableId ?? '')
  const [resolvedColumn, setResolvedColumn] = useState(prefill?.resolvedColumn ?? '')
  const [relationshipId, setRelationshipId] = useState('')
  const [actualColumn, setActualColumn] = useState('')
  const [showFkSuggestions, setShowFkSuggestions] = useState(false)

  const candidateRelationships = childTableId
    ? (relationshipsByChild.get(childTableId) ?? [])
    : []
  const childTable = catalogTables.find((t) => t.id === childTableId)

  // Sort: viable candidates (parent has the resolved column) first, then the rest.
  const sortedCandidates = useMemo(() => {
    const annotated = candidateRelationships.map((r) => ({
      rel: r,
      viable: relationshipIsViable(r, resolvedColumn, catalogTables),
    }))
    annotated.sort((a, b) => {
      if (a.viable !== b.viable) return a.viable ? -1 : 1
      return a.rel.parent_table_name.localeCompare(b.rel.parent_table_name)
    })
    return annotated
  }, [candidateRelationships, resolvedColumn, catalogTables])
  const viableCount = sortedCandidates.filter((c) => c.viable).length

  // Auto-select when there is exactly one viable candidate — the obvious choice.
  // Fires once per (childTableId, resolvedColumn) pair so a deliberate clear
  // by the user is honored rather than silently re-populated.
  const autoSelectedFor = useRef<string | null>(null)
  useEffect(() => {
    if (mode !== 'relationship') return
    const key = `${childTableId}::${resolvedColumn}`
    if (autoSelectedFor.current === key) return
    if (relationshipId) return
    if (viableCount !== 1) return
    const only = sortedCandidates.find((c) => c.viable)
    if (only) {
      setRelationshipId(only.rel.id)
      autoSelectedFor.current = key
    }
  }, [mode, relationshipId, viableCount, sortedCandidates, childTableId, resolvedColumn])

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

  const childSchemaLabel = childTable
    ? tableLabel(childTable.schema, childTable.table)
    : ''
  const showEmptyStateGuidance =
    mode === 'relationship' && !!childTableId && candidateRelationships.length === 0

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
              setShowFkSuggestions(false)
            }}
            className="w-full border border-gray-300 rounded px-2 py-1"
          >
            <option value="">Select…</option>
            {catalogTables.map((t) => (
              <option key={t.id} value={t.id}>
                {tableLabel(t.schema, t.table)}
                {t.schema !== t.schema_upstream
                  ? `  (${t.schema_upstream}.${t.table})`
                  : ''}
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
              disabled={!childTableId || candidateRelationships.length === 0}
              className="w-full border border-gray-300 rounded px-2 py-1 disabled:bg-gray-100"
            >
              <option value="">Select…</option>
              {sortedCandidates.map(({ rel, viable }) => {
                const childEff = resolveEffective(rel.child_schema_name, aliasMap)
                const parentEff = resolveEffective(rel.parent_schema_name, aliasMap)
                const arrow = `${tableLabel(childEff, rel.child_table_name)}.${rel.child_column_name} → ${tableLabel(parentEff, rel.parent_table_name)}.${rel.parent_column_name}`
                return (
                  <option key={rel.id} value={rel.id}>
                    {viable ? '✓ ' : ''}
                    {arrow}
                  </option>
                )
              })}
            </select>
            {!showEmptyStateGuidance && candidateRelationships.length > 0 && viableCount === 0 && resolvedColumn && (
              <div className="mt-1 text-[11px] text-amber-700">
                None of these parent tables contain a column named{' '}
                <code className="font-mono">{resolvedColumn}</code>. Pick the path you intend, or
                switch to <em>Same-table alias</em>.
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

      {showEmptyStateGuidance && (
        <div
          data-testid="anchor-empty-state-guidance"
          className="mt-3 border border-amber-200 bg-amber-50 rounded-lg p-3 text-xs"
        >
          <div className="text-amber-900 font-medium mb-1">
            No relationships from <code className="font-mono">{childSchemaLabel}</code> yet.
          </div>
          <p className="text-amber-800 mb-2">
            An anchor needs an FK walk to a parent table that has{' '}
            <code className="font-mono">{resolvedColumn || 'this column'}</code>, or a same-table
            column alias if it exists under another name on this table.
          </p>
          <div className="flex flex-wrap gap-2 mb-2">
            <button
              type="button"
              onClick={() => setMode('alias')}
              className="text-xs px-2 py-1 rounded bg-white border border-amber-300 text-amber-900 hover:bg-amber-100"
            >
              Switch to Same-table alias
            </button>
            <button
              type="button"
              onClick={() => setShowFkSuggestions((v) => !v)}
              className="text-xs px-2 py-1 rounded bg-white border border-amber-300 text-amber-900 hover:bg-amber-100"
            >
              {showFkSuggestions ? 'Hide FK suggestions' : 'Add a relationship ↓'}
            </button>
          </div>
          {showFkSuggestions && (
            <div className="mt-2">
              <FkSuggestionsList
                datasourceId={datasourceId}
                aliasMap={aliasMap}
                childTableIdFilter={childTableId}
                emptyMessage="No FK suggestions for this table — add one manually below."
                onMutated={invalidate}
                onAdded={(rel) => {
                  setRelationshipId(rel.id)
                  setShowFkSuggestions(false)
                }}
              />
              <InlineManualRelationshipForm
                datasourceId={datasourceId}
                childTable={childTable}
                catalogTables={catalogTables}
                onCreated={(rel) => {
                  invalidate()
                  setRelationshipId(rel.id)
                  setShowFkSuggestions(false)
                }}
              />
            </div>
          )}
        </div>
      )}

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

/// Compact inline relationship-create form rendered inside the anchor empty-state
/// guidance. The child table is locked to the anchor's prefilled value; the user
/// only picks parent table + columns, and on success the new relationship is
/// auto-selected for the anchor.
function InlineManualRelationshipForm({
  datasourceId,
  childTable,
  catalogTables,
  onCreated,
}: {
  datasourceId: string
  childTable: FlatTable | undefined
  catalogTables: CatalogTables
  onCreated: (rel: TableRelationship) => void
}) {
  const [childColumnName, setChildColumnName] = useState('')
  const [parentTableId, setParentTableId] = useState('')
  const [parentColumnName, setParentColumnName] = useState('')

  const parentTable = catalogTables.find((t) => t.id === parentTableId)

  const createMutation = useMutation({
    mutationFn: () =>
      createRelationship(datasourceId, {
        child_table_id: childTable?.id ?? '',
        child_column_name: childColumnName,
        parent_table_id: parentTableId,
        parent_column_name: parentColumnName,
      }),
    onSuccess: (rel) => {
      toast.success('Relationship added')
      setChildColumnName('')
      setParentTableId('')
      setParentColumnName('')
      onCreated(rel)
    },
    onError: (err) => toast.error(errorMessage(err, 'Failed to add relationship')),
  })

  const canSubmit =
    !!childTable && !!childColumnName && !!parentTableId && !!parentColumnName

  if (!childTable) return null

  return (
    <div
      data-testid="anchor-inline-relationship-form"
      className="mt-2 border border-amber-200 rounded p-2 bg-white"
    >
      <div className="text-[11px] font-medium text-amber-900 mb-2">
        Or create a relationship from{' '}
        <code className="font-mono">{tableLabel(childTable.schema, childTable.table)}</code>:
      </div>
      <div className="grid grid-cols-3 gap-2 text-xs">
        <div>
          <label className="block text-[10px] text-gray-500 mb-0.5">Child column (FK)</label>
          <select
            value={childColumnName}
            onChange={(e) => setChildColumnName(e.target.value)}
            className="w-full border border-gray-300 rounded px-2 py-1"
          >
            <option value="">Select…</option>
            {childTable.columns.map((c) => (
              <option key={c.id} value={c.column_name}>
                {c.column_name}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="block text-[10px] text-gray-500 mb-0.5">Parent table</label>
          <select
            value={parentTableId}
            onChange={(e) => {
              setParentTableId(e.target.value)
              setParentColumnName('')
            }}
            className="w-full border border-gray-300 rounded px-2 py-1"
          >
            <option value="">Select…</option>
            {catalogTables
              .filter((t) => t.id !== childTable.id)
              .map((t) => (
                <option key={t.id} value={t.id}>
                  {tableLabel(t.schema, t.table)}
                  {t.schema !== t.schema_upstream
                    ? `  (${t.schema_upstream}.${t.table})`
                    : ''}
                </option>
              ))}
          </select>
        </div>
        <div>
          <label className="block text-[10px] text-gray-500 mb-0.5">Parent column (PK)</label>
          <select
            value={parentColumnName}
            onChange={(e) => setParentColumnName(e.target.value)}
            disabled={!parentTable}
            className="w-full border border-gray-300 rounded px-2 py-1 disabled:bg-gray-100"
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
      <div className="flex justify-end mt-2">
        <button
          type="button"
          onClick={() => createMutation.mutate()}
          disabled={!canSubmit || createMutation.isPending}
          className="text-xs bg-amber-600 text-white px-3 py-1 rounded hover:bg-amber-700 disabled:opacity-50"
        >
          {createMutation.isPending ? 'Adding…' : 'Add and use'}
        </button>
      </div>
    </div>
  )
}
