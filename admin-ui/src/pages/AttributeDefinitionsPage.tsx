import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  listAttributeDefinitions,
  deleteAttributeDefinition,
} from '../api/attributeDefinitions'

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
  const [entityFilter, setEntityFilter] = useState<string>('user')

  const { data, isLoading } = useQuery({
    queryKey: ['attribute-definitions', page, entityFilter],
    queryFn: () =>
      listAttributeDefinitions({
        entity_type: entityFilter || undefined,
        page,
        page_size: 50,
      }),
  })

  const [deleteError, setDeleteError] = useState<string | null>(null)
  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAttributeDefinition(id, false),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['attribute-definitions'] })
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
          <p className="text-sm text-gray-500 mt-1">
            {total} definition{total !== 1 ? 's' : ''}
          </p>
        </div>
        <button
          onClick={() => navigate('/attributes/create')}
          className="bg-blue-600 hover:bg-blue-700 text-white font-medium rounded-lg px-5 py-2 text-sm transition-colors"
        >
          New definition
        </button>
      </div>

      <div className="flex items-center gap-3 mb-4">
        <label className="text-sm text-gray-600">Entity type:</label>
        <select
          value={entityFilter}
          onChange={(e) => {
            setEntityFilter(e.target.value)
            setPage(1)
          }}
          className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          <option value="user">User</option>
          <option value="table">Table</option>
          <option value="column">Column</option>
          <option value="">All</option>
        </select>
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

      {isLoading ? (
        <p className="text-sm text-gray-400">Loading...</p>
      ) : items.length === 0 ? (
        <p className="text-sm text-gray-500">No attribute definitions found.</p>
      ) : (
        <div className="bg-white rounded-xl border border-gray-200 overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-gray-50 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                <th className="px-4 py-3">Key</th>
                <th className="px-4 py-3">Display Name</th>
                <th className="px-4 py-3">Type</th>
                <th className="px-4 py-3">Allowed Values</th>
                <th className="px-4 py-3">Default</th>
                <th className="px-4 py-3 text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {items.map((def) => (
                <tr key={def.id} className="hover:bg-gray-50 transition-colors">
                  <td className="px-4 py-3 font-mono text-sm">{def.key}</td>
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
                    {def.default_value ?? <span className="text-gray-300">none</span>}
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
        </div>
      )}

      {totalPages > 1 && (
        <div className="flex justify-center gap-2 mt-4">
          <button
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page <= 1}
            className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm disabled:opacity-40 hover:bg-gray-50"
          >
            Previous
          </button>
          <span className="text-sm text-gray-500 py-1.5">
            Page {page} of {totalPages}
          </span>
          <button
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page >= totalPages}
            className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm disabled:opacity-40 hover:bg-gray-50"
          >
            Next
          </button>
        </div>
      )}
    </div>
  )
}
