import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { deleteDataSource, listDataSources, testDataSource } from '../api/datasources'
import type { DataSource } from '../types/datasource'

export function DataSourcesListPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [page, setPage] = useState(1)
  const [search, setSearch] = useState('')
  const [searchInput, setSearchInput] = useState('')
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message?: string }>>({})

  const { data, isLoading, isError } = useQuery({
    queryKey: ['datasources', page, search],
    queryFn: () => listDataSources({ page, page_size: 20, search: search || undefined }),
  })

  const deleteMutation = useMutation({
    mutationFn: deleteDataSource,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['datasources'] }),
  })

  function handleSearch(e: React.FormEvent) {
    e.preventDefault()
    setSearch(searchInput)
    setPage(1)
  }

  function handleDelete(ds: DataSource) {
    if (!confirm(`Delete data source "${ds.name}"? This cannot be undone.`)) return
    deleteMutation.mutate(ds.id)
  }

  async function handleTest(ds: DataSource) {
    setTestingId(ds.id)
    try {
      const result = await testDataSource(ds.id)
      setTestResults((prev) => ({ ...prev, [ds.id]: result }))
    } catch {
      setTestResults((prev) => ({ ...prev, [ds.id]: { success: false, message: 'Request failed' } }))
    } finally {
      setTestingId(null)
    }
  }

  const totalPages = data ? Math.ceil(data.total / data.page_size) : 1

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-gray-900">Data Sources</h1>
          {data && (
            <p className="text-sm text-gray-500 mt-0.5">{data.total} total</p>
          )}
        </div>
        <Link
          to="/datasources/create"
          className="bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg px-4 py-2 transition-colors"
        >
          + New data source
        </Link>
      </div>

      {/* Search */}
      <form onSubmit={handleSearch} className="flex gap-2 mb-4">
        <input
          type="search"
          value={searchInput}
          onChange={(e) => setSearchInput(e.target.value)}
          placeholder="Search by name…"
          className="border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 w-64"
        />
        <button
          type="submit"
          className="border border-gray-300 rounded-lg px-4 py-2 text-sm hover:bg-gray-50 transition-colors"
        >
          Search
        </button>
        {search && (
          <button
            type="button"
            onClick={() => { setSearch(''); setSearchInput(''); setPage(1) }}
            className="text-sm text-gray-500 hover:text-gray-700 px-2"
          >
            Clear
          </button>
        )}
      </form>

      {/* Table */}
      <div className="bg-white rounded-xl border border-gray-200 overflow-hidden">
        {isLoading ? (
          <div className="p-8 text-center text-gray-400 text-sm">Loading…</div>
        ) : isError ? (
          <div className="p-8 text-center text-red-500 text-sm">Failed to load data sources.</div>
        ) : data?.data.length === 0 ? (
          <div className="p-8 text-center text-gray-400 text-sm">
            No data sources yet.{' '}
            <Link to="/datasources/create" className="text-blue-600 hover:underline">
              Create one
            </Link>{' '}
            to get started.
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-4 py-3 font-medium text-gray-600">Name</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600">Type</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600">Host</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600">Status</th>
                <th className="px-4 py-3" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {data?.data.map((ds) => (
                <tr key={ds.id} className="hover:bg-gray-50">
                  <td className="px-4 py-3 font-medium text-gray-900 font-mono text-xs">
                    {ds.name}
                  </td>
                  <td className="px-4 py-3 text-gray-600 capitalize">{ds.ds_type}</td>
                  <td className="px-4 py-3 text-gray-600 text-xs font-mono">
                    {String(ds.config.host ?? '—')}
                    {ds.config.port ? `:${ds.config.port}` : ''}
                  </td>
                  <td className="px-4 py-3">
                    <span
                      className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                        ds.is_active
                          ? 'bg-green-100 text-green-700'
                          : 'bg-gray-100 text-gray-500'
                      }`}
                    >
                      {ds.is_active ? 'Active' : 'Inactive'}
                    </span>
                    {testResults[ds.id] && (
                      <span
                        className={`ml-2 text-xs font-medium ${
                          testResults[ds.id].success ? 'text-green-600' : 'text-red-500'
                        }`}
                      >
                        {testResults[ds.id].success ? '✓' : `✗ ${testResults[ds.id].message ?? ''}`}
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-3 justify-end">
                      <button
                        onClick={() => handleTest(ds)}
                        disabled={testingId === ds.id}
                        className="text-gray-500 hover:text-gray-700 text-xs font-medium disabled:opacity-50"
                      >
                        {testingId === ds.id ? 'Testing…' : 'Test'}
                      </button>
                      <button
                        onClick={() => navigate(`/datasources/${ds.id}/edit`)}
                        className="text-blue-600 hover:text-blue-800 text-xs font-medium"
                      >
                        Edit
                      </button>
                      <button
                        onClick={() => handleDelete(ds)}
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

      {/* Pagination */}
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
