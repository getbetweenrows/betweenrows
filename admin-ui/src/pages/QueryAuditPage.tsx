import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { format } from 'sql-formatter'
import { listAuditLogs } from '../api/audit'
import type { AuditLogEntry } from '../api/audit'
import { EntitySelect } from '../components/EntitySelect'
import { searchUsers, searchDataSources } from '../utils/entitySearchFns'

function formatSql(sql: string): string {
  try {
    return format(sql, { language: 'postgresql', tabWidth: 2, keywordCase: 'upper' })
  } catch {
    return sql
  }
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleString()
}

function truncate(s: string, n: number) {
  return s.length > n ? s.slice(0, n) + '…' : s
}

function StatusBadge({ status }: { status: AuditLogEntry['status'] }) {
  if (status === 'success') {
    return (
      <span className="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium bg-green-50 text-green-700">
        success
      </span>
    )
  }
  if (status === 'denied') {
    return (
      <span className="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium bg-amber-50 text-amber-700">
        denied
      </span>
    )
  }
  return (
    <span className="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium bg-red-50 text-red-700">
      error
    </span>
  )
}

export function QueryAuditPage() {
  const [page, setPage] = useState(1)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [filterUser, setFilterUser] = useState('')
  const [filterDs, setFilterDs] = useState('')
  const [filterFrom, setFilterFrom] = useState('')
  const [filterTo, setFilterTo] = useState('')
  const [filterStatus, setFilterStatus] = useState('')
  const [appliedFilters, setAppliedFilters] = useState<{
    user_id?: string
    datasource_id?: string
    from?: string
    to?: string
    status?: string
  }>({})

  const { data, isLoading, isError } = useQuery({
    queryKey: ['audit-logs', page, appliedFilters],
    queryFn: () =>
      listAuditLogs({
        page,
        page_size: 20,
        ...appliedFilters,
      }),
  })

  function handleFilter(e: React.FormEvent) {
    e.preventDefault()
    setAppliedFilters({
      user_id: filterUser || undefined,
      datasource_id: filterDs || undefined,
      from: filterFrom || undefined,
      to: filterTo || undefined,
      status: filterStatus || undefined,
    })
    setPage(1)
  }

  function clearFilters() {
    setFilterUser('')
    setFilterDs('')
    setFilterFrom('')
    setFilterTo('')
    setFilterStatus('')
    setAppliedFilters({})
    setPage(1)
  }

  function toggleExpand(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const totalPages = data ? Math.ceil(data.total / data.page_size) : 1
  const hasFilters = Object.values(appliedFilters).some(Boolean)

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-gray-900">Query Audit Log</h1>
          {data && <p className="text-sm text-gray-500 mt-0.5">{data.total} total entries</p>}
        </div>
      </div>

      {/* Filters */}
      <form onSubmit={handleFilter} className="bg-white rounded-xl border border-gray-200 p-4 mb-4">
        <div className="grid grid-cols-5 gap-3 mb-3">
          <EntitySelect
            label="User"
            value={filterUser}
            onChange={setFilterUser}
            searchFn={searchUsers}
            placeholder="Search users…"
          />
          <EntitySelect
            label="Data Source"
            value={filterDs}
            onChange={setFilterDs}
            searchFn={searchDataSources}
            placeholder="Search data sources…"
          />
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">From</label>
            <input
              type="datetime-local"
              value={filterFrom}
              onChange={(e) => setFilterFrom(e.target.value)}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">To</label>
            <input
              type="datetime-local"
              value={filterTo}
              onChange={(e) => setFilterTo(e.target.value)}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">Status</label>
            <select
              value={filterStatus}
              onChange={(e) => setFilterStatus(e.target.value)}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
            >
              <option value="">All</option>
              <option value="success">Success</option>
              <option value="error">Error</option>
              <option value="denied">Denied</option>
            </select>
          </div>
        </div>
        <div className="flex gap-2">
          <button
            type="submit"
            className="border border-gray-300 rounded px-3 py-1.5 text-xs hover:bg-gray-50 transition-colors"
          >
            Apply filters
          </button>
          {hasFilters && (
            <button
              type="button"
              onClick={clearFilters}
              className="text-xs text-gray-500 hover:text-gray-700 px-2"
            >
              Clear
            </button>
          )}
        </div>
      </form>

      {/* Table */}
      <div className="bg-white rounded-xl border border-gray-200 overflow-hidden">
        {isLoading ? (
          <div className="p-8 text-center text-gray-400 text-sm">Loading…</div>
        ) : isError ? (
          <div className="p-8 text-center text-red-500 text-sm">Failed to load audit logs.</div>
        ) : data?.data.length === 0 ? (
          <div className="p-8 text-center text-gray-400 text-sm">No audit entries found.</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Time</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">User</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Data Source</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Query</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Status</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Policies</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Duration</th>
                <th className="px-4 py-3" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {data?.data.map((entry: AuditLogEntry) => (
                <>
                  <tr key={entry.id} className="hover:bg-gray-50">
                    <td className="px-4 py-3 text-gray-600 text-xs whitespace-nowrap">
                      {formatDate(entry.created_at)}
                    </td>
                    <td className="px-4 py-3 text-gray-900 text-xs font-medium">{entry.username}</td>
                    <td className="px-4 py-3 text-gray-600 text-xs font-mono">{entry.datasource_name}</td>
                    <td className="px-4 py-3 text-xs font-mono text-gray-700 max-w-xs">
                      {truncate(entry.original_query, 80)}
                    </td>
                    <td className="px-4 py-3 text-xs">
                      <StatusBadge status={entry.status} />
                    </td>
                    <td className="px-4 py-3 text-xs text-gray-600">
                      {entry.policies_applied.length > 0 ? (
                        <span className="bg-blue-50 text-blue-700 rounded-full px-2 py-0.5 text-xs">
                          {entry.policies_applied.length}
                        </span>
                      ) : (
                        <span className="text-gray-400">—</span>
                      )}
                    </td>
                    <td className="px-4 py-3 text-xs text-gray-600">
                      {entry.execution_time_ms != null ? `${entry.execution_time_ms}ms` : '—'}
                    </td>
                    <td className="px-4 py-3">
                      <button
                        onClick={() => toggleExpand(entry.id)}
                        className="text-xs text-blue-600 hover:text-blue-800"
                      >
                        {expanded.has(entry.id) ? 'Collapse' : 'Details'}
                      </button>
                    </td>
                  </tr>
                  {expanded.has(entry.id) && (
                    <tr key={`${entry.id}-detail`} className="bg-gray-50">
                      <td colSpan={8} className="px-4 py-4">
                        <div className="space-y-3">
                          {entry.error_message && (
                            <div className="bg-red-50 border border-red-200 rounded p-3">
                              <p className="text-xs font-semibold text-red-700 mb-1">Error</p>
                              <p className="text-xs font-mono text-red-800">{entry.error_message}</p>
                            </div>
                          )}
                          <div>
                            <p className="text-xs font-semibold text-gray-600 mb-1">Original query</p>
                            <pre className="text-xs font-mono text-gray-800 bg-white border border-gray-200 rounded p-3 overflow-auto whitespace-pre-wrap">
                              {formatSql(entry.original_query)}
                            </pre>
                          </div>
                          {entry.rewritten_query && (
                            <div>
                              <p className="text-xs font-semibold text-gray-600 mb-1">Rewritten query</p>
                              <pre className="text-xs font-mono text-gray-800 bg-white border border-gray-200 rounded p-3 overflow-auto whitespace-pre-wrap">
                                {formatSql(entry.rewritten_query)}
                              </pre>
                            </div>
                          )}
                          {entry.policies_applied.length > 0 && (
                            <div>
                              <p className="text-xs font-semibold text-gray-600 mb-1">Policies applied</p>
                              <ul className="space-y-1">
                                {entry.policies_applied.map((p) => (
                                  <li key={p.policy_id} className="text-xs text-gray-700">
                                    <span className="font-medium">{p.name}</span>
                                    <span className="text-gray-400 ml-2">v{p.version}</span>
                                    <span className="text-gray-400 ml-2 font-mono">{p.policy_id}</span>
                                    {p.decision && (
                                      <span className="ml-2">
                                        <span className={`inline-flex items-center rounded px-1.5 py-0.5 text-xs font-mono ${p.decision.result?.fire ? 'bg-green-50 text-green-700' : 'bg-amber-50 text-amber-700'}`}>
                                          {p.decision.result?.fire ? 'fired' : 'skipped'}
                                        </span>
                                        <span className="text-gray-400 ml-1">{p.decision.fuel_consumed ?? p.decision.result?.fuel_consumed} fuel</span>
                                        <span className="text-gray-400 ml-1">{p.decision.time_us ?? p.decision.result?.time_us}us</span>
                                        {p.decision.error && (
                                          <span className="text-red-500 ml-1">{p.decision.error}</span>
                                        )}
                                        {(p.decision.logs?.length ?? 0) > 0 && (
                                          <details className="inline ml-2">
                                            <summary className="text-blue-600 cursor-pointer">logs ({p.decision.logs!.length})</summary>
                                            <pre className="text-xs font-mono text-gray-600 bg-gray-100 rounded p-1 mt-1">{p.decision.logs!.join('\n')}</pre>
                                          </details>
                                        )}
                                      </span>
                                    )}
                                  </li>
                                ))}
                              </ul>
                            </div>
                          )}
                          <div className="flex gap-6 text-xs text-gray-500">
                            {entry.client_ip && <span>IP: {entry.client_ip}</span>}
                            {entry.client_info && <span>App: {entry.client_info}</span>}
                          </div>
                        </div>
                      </td>
                    </tr>
                  )}
                </>
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
