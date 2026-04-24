import { useState } from 'react'
import { useDebounce } from '../hooks/useDebounce'
import { Link, useNavigate } from 'react-router-dom'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { listDataSources, testDataSource } from '../api/datasources'
import type { DataSource } from '../types/datasource'
import { CopyableId } from '../components/CopyableId'

export function DataSourcesListPage() {
  const navigate = useNavigate()
  const [page, setPage] = useState(1)
  const [search, setSearch] = useState('')
  const debouncedSearch = useDebounce(search, 300)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message?: string }>>({})

  const { data, isLoading, isError } = useQuery({
    queryKey: ['datasources', page, debouncedSearch],
    queryFn: () => listDataSources({ page, page_size: 20, search: debouncedSearch || undefined }),
    placeholderData: keepPreviousData,
  })

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

      <div className="flex items-center gap-3 mb-4">
        <input
          type="search"
          value={search}
          onChange={(e) => { setSearch(e.target.value); setPage(1) }}
          placeholder="Search by name…"
          className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 w-64"
        />
      </div>

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
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Name</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">ID</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Type</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Host</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Status</th>
                <th className="px-4 py-3" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {data?.data.map((ds) => (
                <tr key={ds.id} className="hover:bg-gray-50">
                  <td className="px-4 py-3 font-medium text-gray-900 font-mono text-xs">
                    {ds.name}
                  </td>
                  <td className="px-4 py-3">
                    <CopyableId id={ds.id} short />
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
