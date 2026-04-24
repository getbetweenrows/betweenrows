import { useState } from 'react'
import { useDebounce } from '../hooks/useDebounce'
import { Link, useNavigate } from 'react-router-dom'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { listAttributeDefinitions } from '../api/attributeDefinitions'
import { CopyableId } from '../components/CopyableId'
import {
  SUPPORTED_ENTITY_TYPES,
  type AttributeDefinition,
} from '../types/attributeDefinition'

// Renders a value_type as a type-signature string, e.g.:
//   string
//   string ∈ {read, write, admin}
//   list<string>
//   list<string> ∈ {us, eu}
function formatValueType(def: AttributeDefinition): string {
  const base = def.value_type === 'list' ? 'list<string>' : def.value_type
  if (def.allowed_values && def.allowed_values.length > 0) {
    return `${base} ∈ {${def.allowed_values.join(', ')}}`
  }
  return base
}

export function AttributeDefinitionsPage() {
  const navigate = useNavigate()
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

      <div className="bg-white rounded-xl border border-gray-200 overflow-hidden">
        {isLoading ? (
          <div className="p-8 text-center text-gray-400 text-sm">Loading...</div>
        ) : items.length === 0 ? (
          <div className="p-8 text-center text-gray-400 text-sm">No attribute definitions found.</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Name</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Key</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">ID</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Type</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Default</th>
                <th className="px-4 py-3 font-medium text-gray-600 text-xs text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {items.map((def) => (
                <tr key={def.id} className="hover:bg-gray-50 transition-colors">
                  <td className="px-4 py-3">
                    <div className="max-w-xs">
                      <div
                        className="font-medium text-gray-900 truncate"
                        title={def.display_name}
                      >
                        {def.display_name}
                      </div>
                      {def.description && (
                        <div
                          className="text-xs text-gray-500 mt-0.5 truncate"
                          title={def.description}
                        >
                          {def.description}
                        </div>
                      )}
                    </div>
                  </td>
                  <td className="px-4 py-3">
                    <code className="inline-block font-mono text-xs text-gray-600 bg-gray-100 rounded px-1.5 py-0.5">
                      {`{${def.entity_type}.${def.key}}`}
                    </code>
                  </td>
                  <td className="px-4 py-3">
                    <CopyableId id={def.id} short />
                  </td>
                  <td className="px-4 py-3 font-mono text-xs text-gray-600">
                    {formatValueType(def)}
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
