import { useState } from 'react'
import { useDebounce } from '../hooks/useDebounce'
import { Link, useNavigate } from 'react-router-dom'
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { deleteRole, listRoles } from '../api/roles'
import type { Role } from '../types/role'
import { CopyableId } from '../components/CopyableId'

export function RolesListPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [page, setPage] = useState(1)
  const [search, setSearch] = useState('')
  const debouncedSearch = useDebounce(search, 300)

  const { data, isLoading, isError } = useQuery({
    queryKey: ['roles', page, debouncedSearch],
    queryFn: () => listRoles({ page, page_size: 20, search: debouncedSearch || undefined }),
    placeholderData: keepPreviousData,
  })

  const deleteMutation = useMutation({
    mutationFn: deleteRole,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['roles'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
    },
  })

  function handleDelete(role: Role) {
    if (!confirm(`Delete role "${role.name}"? This cannot be undone.`)) return
    deleteMutation.mutate(role.id)
  }

  const totalPages = data ? Math.ceil(data.total / data.page_size) : 1

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-gray-900">Roles</h1>
          {data && <p className="text-sm text-gray-500 mt-0.5">{data.total} total</p>}
        </div>
        <Link
          to="/roles/create"
          className="bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg px-4 py-2 transition-colors"
        >
          + New role
        </Link>
      </div>

      <div className="flex items-center gap-3 mb-4">
        <input
          type="search"
          value={search}
          onChange={(e) => { setSearch(e.target.value); setPage(1) }}
          placeholder="Search by name…"
          className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 w-64"
        />
      </div>

      <div className="bg-white rounded-xl border border-gray-200 overflow-hidden">
        {isLoading ? (
          <div className="p-8 text-center text-gray-400 text-sm">Loading...</div>
        ) : isError ? (
          <div className="p-8 text-center text-red-500 text-sm">Failed to load roles.</div>
        ) : data?.data.length === 0 ? (
          <div className="p-8 text-center text-gray-400 text-sm">
            No roles yet.{' '}
            <Link to="/roles/create" className="text-blue-600 hover:underline">
              Create one
            </Link>{' '}
            to get started.
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Name</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">ID</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Status</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Direct Members</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Created</th>
                <th className="px-4 py-3" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {data?.data.map((role) => (
                <tr
                  key={role.id}
                  className="hover:bg-gray-50 cursor-pointer"
                  onClick={() => navigate(`/roles/${role.id}`)}
                >
                  <td className="px-4 py-3">
                    <div className="max-w-xs">
                      <div
                        className="font-medium text-gray-900 truncate"
                        title={role.name}
                      >
                        {role.name}
                      </div>
                      {role.description && (
                        <div
                          className="text-xs text-gray-500 mt-0.5 truncate"
                          title={role.description}
                        >
                          {role.description}
                        </div>
                      )}
                    </div>
                  </td>
                  <td className="px-4 py-3" onClick={(e) => e.stopPropagation()}>
                    <CopyableId id={role.id} short />
                  </td>
                  <td className="px-4 py-3">
                    <span
                      className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                        role.is_active
                          ? 'bg-green-100 text-green-700'
                          : 'bg-gray-100 text-gray-500'
                      }`}
                    >
                      {role.is_active ? 'Active' : 'Inactive'}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-gray-600">{role.direct_member_count}</td>
                  <td className="px-4 py-3 text-gray-500 text-xs">
                    {new Date(role.created_at).toLocaleDateString()}
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-3 justify-end">
                      <button
                        onClick={(e) => { e.stopPropagation(); navigate(`/roles/${role.id}`) }}
                        className="text-blue-600 hover:text-blue-800 text-xs font-medium"
                      >
                        Edit
                      </button>
                      <button
                        onClick={(e) => { e.stopPropagation(); handleDelete(role) }}
                        className="text-red-500 hover:text-red-700 text-xs font-medium"
                      >
                        Delete
                      </button>
                    </div>
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
            disabled={page === 1}
            className="px-3 py-1.5 text-sm border border-gray-300 rounded-lg disabled:opacity-40 hover:bg-gray-50 transition-colors"
          >
            Previous
          </button>
          <span className="text-sm text-gray-600">
            Page {page} of {totalPages}
          </span>
          <button
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page === totalPages}
            className="px-3 py-1.5 text-sm border border-gray-300 rounded-lg disabled:opacity-40 hover:bg-gray-50 transition-colors"
          >
            Next
          </button>
        </div>
      )}
    </div>
  )
}
