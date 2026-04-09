import { useState } from 'react'
import { useDebounce } from '../hooks/useDebounce'
import { Link, useNavigate } from 'react-router-dom'
import { keepPreviousData, useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  listAttributeDefinitions,
  deleteAttributeDefinition,
} from '../api/attributeDefinitions'
import { CopyableId } from '../components/CopyableId'
import { SUPPORTED_ENTITY_TYPES } from '../types/attributeDefinition'

const VALUE_TYPE_BADGE: Record<string, string> = {
  string: 'bg-blue-100 text-blue-700',
  integer: 'bg-purple-100 text-purple-700',
  boolean: 'bg-amber-100 text-amber-700',
  list: 'bg-green-100 text-green-700',
}

export function AttributeDefinitionsPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [page, setPage] = useState(1)
  const [search, setSearch] = useState('')
  const debouncedSearch = useDebounce(search, 300)
  const [entityFilter, setEntityFilter] = useState<string>('user')

  const { data, isLoading } = useQuery({
    queryKey: ['attribute-definitions', page, entityFilter, debouncedSearch],
    queryFn: () =>
      listAttributeDefinitions({
        entity_type: entityFilter || undefined,
        search: debouncedSearch || undefined,
        page,
        page_size: 50,
      }),
    placeholderData: keepPreviousData,
  })

  const [deleteError, setDeleteError] = useState<string | null>(null)
  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAttributeDefinition(id, false),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['attribute-definitions'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      setDeleteError(null)
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to delete'
      // If conflict (in use), offer force delete
      setDeleteError(msg)
    },
  })

  const forceDeleteMutation = useMutation({
    mutationFn: (id: string) => deleteAttributeDefinition(id, true),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['attribute-definitions'] })
      queryClient.invalidateQueries({ queryKey: ['users'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      setDeleteError(null)
    },
  })

  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)

  function handleDelete(id: string) {
    setPendingDeleteId(id)
    setDeleteError(null)
    deleteMutation.mutate(id)
  }

  function handleForceDelete() {
    if (pendingDeleteId) {
      forceDeleteMutation.mutate(pendingDeleteId)
      setPendingDeleteId(null)
    }
  }

  const items = data?.data ?? []
  const total = data?.total ?? 0
  const pageSize = 50
  const totalPages = Math.ceil(total / pageSize)

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-gray-900">Attribute Definitions</h1>
          <p className="text-sm text-gray-500 mt-0.5">{total} total</p>
        </div>
        <Link
          to="/attributes/create"
          className="bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg px-4 py-2 transition-colors"
        >
          + New definition
        </Link>
      </div>

      <div className="flex items-center gap-3 mb-4">
        <label className="text-sm text-gray-600 flex items-center gap-2">
          Entity type:
          <select
            value={entityFilter}
            onChange={(e) => {
              setEntityFilter(e.target.value)
              setPage(1)
            }}
            className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            {SUPPORTED_ENTITY_TYPES.map((t) => (
              <option key={t} value={t}>{t.charAt(0).toUpperCase() + t.slice(1)}</option>
            ))}
            <option value="">All</option>
          </select>
        </label>
        <input
          type="search"
          value={search}
          onChange={(e) => { setSearch(e.target.value); setPage(1) }}
          placeholder="Search by key or display name…"
          className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 w-64"
        />
      </div>

      {deleteError && (
        <div className="mb-4 p-3 bg-red-50 border border-red-200 rounded-lg text-sm text-red-700 flex items-center justify-between">
          <span>{deleteError}</span>
          {deleteError.includes('force=true') && (
            <button
              onClick={handleForceDelete}
              className="ml-4 bg-red-600 hover:bg-red-700 text-white text-xs font-medium rounded px-3 py-1 transition-colors"
            >
              Force delete
            </button>
          )}
        </div>
      )}

      <div className="bg-white rounded-xl border border-gray-200 overflow-hidden">
        {isLoading ? (
          <div className="p-8 text-center text-gray-400 text-sm">Loading...</div>
        ) : items.length === 0 ? (
          <div className="p-8 text-center text-gray-400 text-sm">No attribute definitions found.</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Key</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">ID</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Entity</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Display Name</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Type</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Allowed Values</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Default</th>
                <th className="px-4 py-3 font-medium text-gray-600 text-xs text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {items.map((def) => (
                <tr key={def.id} className="hover:bg-gray-50 transition-colors">
                  <td className="px-4 py-3 font-mono text-sm">{def.key}</td>
                  <td className="px-4 py-3">
                    <CopyableId id={def.id} short />
                  </td>
                  <td className="px-4 py-3 text-gray-500">{def.entity_type}</td>
                  <td className="px-4 py-3">{def.display_name}</td>
                  <td className="px-4 py-3">
                    <span
                      className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${VALUE_TYPE_BADGE[def.value_type] ?? 'bg-gray-100 text-gray-700'}`}
                    >
                      {def.value_type}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-gray-500">
                    {def.allowed_values
                      ? def.allowed_values.join(', ')
                      : <span className="text-gray-300">any</span>}
                  </td>
                  <td className="px-4 py-3 text-gray-500">
                    {def.default_value != null
                      ? def.default_value
                      : <span className="text-gray-300 italic">null</span>}
                  </td>
                  <td className="px-4 py-3 text-right space-x-2">
                    <button
                      onClick={() => navigate(`/attributes/${def.id}/edit`)}
                      className="text-blue-600 hover:text-blue-800 text-sm"
                    >
                      Edit
                    </button>
                    <button
                      onClick={() => handleDelete(def.id)}
                      className="text-red-600 hover:text-red-800 text-sm"
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {totalPages > 1 && (
        <div className="flex items-center gap-2 mt-4 justify-end">
          <button
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page <= 1}
            className="px-3 py-1.5 text-sm border border-gray-300 rounded-lg disabled:opacity-40 hover:bg-gray-50 transition-colors"
          >
            Previous
          </button>
          <span className="text-sm text-gray-600">
            Page {page} of {totalPages}
          </span>
          <button
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page >= totalPages}
            className="px-3 py-1.5 text-sm border border-gray-300 rounded-lg disabled:opacity-40 hover:bg-gray-50 transition-colors"
          >
            Next
          </button>
        </div>
      )}
    </div>
  )
}
