import { Fragment, useState, useMemo } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { listAdminAuditLogs } from '../api/adminAudit'
import { actionBadgeClass } from '../utils/auditBadge'
import { EntitySelect } from '../components/EntitySelect'
import {
  searchUsers,
  searchDataSources,
  searchRoles,
  searchPolicies,
} from '../utils/entitySearchFns'

function formatDate(iso: string) {
  return new Date(iso).toLocaleString()
}

// The backend stores resource_type as "proxy_user" (matching the DB table name).
// The UI displays it as "user" for readability, but all API queries use "proxy_user".
function resourceBadgeClass(resourceType: string): string {
  switch (resourceType) {
    case 'role':
      return 'bg-purple-100 text-purple-700'
    case 'proxy_user':
      return 'bg-blue-100 text-blue-700'
    case 'policy':
      return 'bg-indigo-100 text-indigo-700'
    case 'datasource':
      return 'bg-emerald-100 text-emerald-700'
    default:
      return 'bg-gray-100 text-gray-600'
  }
}

export function AdminAuditPage() {
  const [page, setPage] = useState(1)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [filterResourceType, setFilterResourceType] = useState('')
  const [filterResourceId, setFilterResourceId] = useState('')
  const [filterActorId, setFilterActorId] = useState('')
  const [filterFrom, setFilterFrom] = useState('')
  const [filterTo, setFilterTo] = useState('')
  const [appliedFilters, setAppliedFilters] = useState<{
    resource_type?: string
    resource_id?: string
    actor_id?: string
    from?: string
    to?: string
  }>({})

  const resourceSearchFn = useMemo(() => {
    const fns: Record<string, (q: string) => Promise<import('../utils/entitySearchFns').EntityOption[]>> = {
      proxy_user: searchUsers,
      role: searchRoles,
      policy: searchPolicies,
      datasource: searchDataSources,
    }
    return fns[filterResourceType] ?? null
  }, [filterResourceType])

  function handleResourceTypeChange(value: string) {
    setFilterResourceType(value)
    setFilterResourceId('')
  }

  const { data, isLoading, isError } = useQuery({
    queryKey: ['admin-audit', page, appliedFilters],
    queryFn: () =>
      listAdminAuditLogs({
        page,
        page_size: 20,
        ...appliedFilters,
      }),
    placeholderData: keepPreviousData,
  })

  function handleFilter(e: React.FormEvent) {
    e.preventDefault()
    setAppliedFilters({
      resource_type: filterResourceType || undefined,
      resource_id: filterResourceId || undefined,
      actor_id: filterActorId || undefined,
      from: filterFrom ? filterFrom + ':00' : undefined,
      to: filterTo ? filterTo + ':00' : undefined,
    })
    setPage(1)
  }

  function clearFilters() {
    setFilterResourceType('')
    setFilterResourceId('')
    setFilterActorId('')
    setFilterFrom('')
    setFilterTo('')
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
          <h1 className="text-xl font-bold text-gray-900">Admin Audit Log</h1>
          {data && <p className="text-sm text-gray-500 mt-0.5">{data.total} total entries</p>}
        </div>
      </div>

      {/* Filters */}
      <form onSubmit={handleFilter} className="bg-white rounded-xl border border-gray-200 p-4 mb-4">
        <div className="grid grid-cols-5 gap-3 mb-3">
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">Resource Type</label>
            <select
              value={filterResourceType}
              onChange={(e) => handleResourceTypeChange(e.target.value)}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
            >
              <option value="">All</option>
              <option value="role">Role</option>
              {/* Backend resource_type is "proxy_user" (DB table name); displayed as "User" */}
              <option value="proxy_user">User</option>
              <option value="policy">Policy</option>
              <option value="datasource">Datasource</option>
            </select>
          </div>
          {resourceSearchFn ? (
            <EntitySelect
              label="Resource"
              value={filterResourceId}
              onChange={setFilterResourceId}
              searchFn={resourceSearchFn}
              placeholder={`Search ${
                { proxy_user: 'users', role: 'roles', policy: 'policies', datasource: 'data sources' }[filterResourceType] ?? ''
              }…`}
            />
          ) : (
            <div />
          )}
          <EntitySelect
            label="Actor"
            value={filterActorId}
            onChange={setFilterActorId}
            searchFn={searchUsers}
            placeholder="Search users…"
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
          <div className="p-8 text-center text-gray-400 text-sm">Loading...</div>
        ) : isError ? (
          <div className="p-8 text-center text-red-500 text-sm">Failed to load audit logs.</div>
        ) : data?.data.length === 0 ? (
          <div className="p-8 text-center text-gray-400 text-sm">No audit entries found.</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Time</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Resource Type</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Action</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Actor ID</th>
                <th className="text-left px-4 py-3 font-medium text-gray-600 text-xs">Resource ID</th>
                <th className="px-4 py-3" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {data?.data.map((entry) => (
                <Fragment key={entry.id}>
                  <tr className="hover:bg-gray-50">
                    <td className="px-4 py-3 text-gray-600 text-xs whitespace-nowrap">
                      {formatDate(entry.created_at)}
                    </td>
                    <td className="px-4 py-3 text-xs">
                      <span
                        className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${resourceBadgeClass(entry.resource_type)}`}
                      >
                        {entry.resource_type === 'proxy_user' ? 'user' : entry.resource_type}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-xs">
                      <span
                        className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${actionBadgeClass(entry.action)}`}
                      >
                        {entry.action}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-xs font-mono text-gray-600 max-w-[12rem] truncate">
                      {entry.actor_id}
                    </td>
                    <td className="px-4 py-3 text-xs font-mono text-gray-600 max-w-[12rem] truncate">
                      {entry.resource_id}
                    </td>
                    <td className="px-4 py-3">
                      {entry.changes && Object.keys(entry.changes).length > 0 && (
                        <button
                          onClick={() => toggleExpand(entry.id)}
                          className="text-xs text-blue-600 hover:text-blue-800"
                        >
                          {expanded.has(entry.id) ? 'Collapse' : 'Details'}
                        </button>
                      )}
                    </td>
                  </tr>
                  {expanded.has(entry.id) && entry.changes && (
                    <tr key={`${entry.id}-detail`} className="bg-gray-50">
                      <td colSpan={6} className="px-4 py-4">
                        <p className="text-xs font-semibold text-gray-600 mb-1">Changes</p>
                        <pre className="text-xs font-mono text-gray-800 bg-white border border-gray-200 rounded p-3 overflow-auto whitespace-pre-wrap">
                          {JSON.stringify(entry.changes, null, 2)}
                        </pre>
                      </td>
                    </tr>
                  )}
                </Fragment>
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
