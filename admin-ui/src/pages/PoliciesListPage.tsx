import { useState } from 'react'
import { useDebounce } from '../hooks/useDebounce'
import { Link, useNavigate } from 'react-router-dom'
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { listPolicies, updatePolicy } from '../api/policies'
import type { PolicyResponse } from '../types/policy'
import { CopyableId } from '../components/CopyableId'

export function PoliciesListPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [page, setPage] = useState(1)
  const [search, setSearch] = useState('')
  const debouncedSearch = useDebounce(search, 300)

  const { data, isLoading, isError } = useQuery({
    queryKey: ['policies', page, debouncedSearch],
    queryFn: () => listPolicies({ page, page_size: 20, search: debouncedSearch || undefined }),
    placeholderData: keepPreviousData,
  })

  const toggleMutation = useMutation({
    mutationFn: ({ id, is_enabled, version }: { id: string; is_enabled: boolean; version: number }) =>
      updatePolicy(id, { is_enabled, version }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['policies'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
    },
  })

  function handleToggle(policy: PolicyResponse) {
    toggleMutation.mutate({ id: policy.id, is_enabled: !policy.is_enabled, version: policy.version })
  }

  const totalPages = data ? Math.ceil(data.total / data.page_size) : 1

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-gray-900">Policies</h1>
          {data && <p className="text-sm text-gray-500 mt-0.5">{data.total} total</p>}
        </div>
        <Link
          to="/policies/create"
          className="bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg px-4 py-2 transition-colors"
        >
          + New policy
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
          <div className="p-8 text-center text-gray-400 text-sm">Loading…</div>
        ) : isError ? (
          <div className="p-8 text-center text-red-500 text-sm">Failed to load policies.</div>
        ) : data?.data.length === 0 ? (
          <div className="p-8 text-center text-gray-400 text-sm">
            No policies yet.{' '}
            <Link to="/policies/create" className="text-blue-600 hover:underline">
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
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Type</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Targets</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Assignments</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Enabled</th>
                <th className="px-4 py-3" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {data?.data.map((policy) => (
                <tr key={policy.id} className="hover:bg-gray-50">
                  <td className="px-4 py-3">
                    <div className="max-w-xs">
                      <div
                        className="font-medium text-gray-900 truncate"
                        title={policy.name}
                      >
                        {policy.name}
                      </div>
                      {policy.description && (
                        <div
                          className="text-xs text-gray-500 mt-0.5 truncate"
                          title={policy.description}
                        >
                          {policy.description}
                        </div>
                      )}
                    </div>
                  </td>
                  <td className="px-4 py-3">
                    <CopyableId id={policy.id} short />
                  </td>
                  <td className="px-4 py-3">
                    <span
                      className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                        policy.policy_type === 'column_deny' || policy.policy_type === 'table_deny'
                          ? 'bg-red-100 text-red-700'
                          : 'bg-blue-100 text-blue-700'
                      }`}
                    >
                      {policy.policy_type}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-gray-600">{policy.targets?.length ?? 0}</td>
                  <td className="px-4 py-3 text-gray-600">{policy.assignment_count}</td>
                  <td className="px-4 py-3">
                    <button
                      onClick={() => handleToggle(policy)}
                      disabled={toggleMutation.isPending}
                      className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors focus:outline-none disabled:opacity-50 ${
                        policy.is_enabled ? 'bg-blue-600' : 'bg-gray-300'
                      }`}
                    >
                      <span
                        className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform ${
                          policy.is_enabled ? 'translate-x-4.5' : 'translate-x-0.5'
                        }`}
                      />
                    </button>
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-3 justify-end">
                      <button
                        onClick={() => navigate(`/policies/${policy.id}/edit`)}
                        className="text-blue-600 hover:text-blue-800 text-xs font-medium"
                      >
                        Edit
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
