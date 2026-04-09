import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { listAdminAuditLogs } from '../api/adminAudit'
import { actionBadgeClass } from '../utils/auditBadge'

interface AuditTimelineProps {
  resourceType: string
  resourceId: string
}

export function AuditTimeline({ resourceType, resourceId }: AuditTimelineProps) {
  const [page, setPage] = useState(1)

  const { data, isLoading, isError } = useQuery({
    queryKey: ['admin-audit', resourceType, resourceId, page],
    queryFn: () =>
      listAdminAuditLogs({
        resource_type: resourceType,
        resource_id: resourceId,
        page,
        page_size: 5,
      }),
  })

  const totalPages = data ? Math.ceil(data.total / data.page_size) : 1

  if (isLoading) {
    return <div className="text-sm text-gray-400">Loading activity...</div>
  }

  if (isError) {
    return <div className="text-sm text-red-500">Failed to load activity.</div>
  }

  if (!data || data.data.length === 0) {
    return <div className="text-sm text-gray-400">No activity recorded yet.</div>
  }

  return (
    <div>
      <div className="space-y-3">
        {data.data.map((entry) => (
          <div
            key={entry.id}
            className="flex gap-3 border-l-2 border-gray-200 pl-4 py-1"
          >
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-0.5">
                <span
                  className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${actionBadgeClass(entry.action)}`}
                >
                  {entry.action}
                </span>
                <span className="text-xs text-gray-400">
                  {new Date(entry.created_at).toLocaleString()}
                </span>
              </div>
              <p className="text-xs text-gray-500">
                Actor: <span className="font-mono text-gray-600">{entry.actor_id}</span>
              </p>
              {entry.changes && Object.keys(entry.changes).length > 0 && (
                <details className="mt-1">
                  <summary className="text-xs text-gray-400 cursor-pointer hover:text-gray-600">
                    View changes
                  </summary>
                  <pre className="mt-1 text-xs bg-gray-50 border border-gray-200 rounded p-2 overflow-x-auto text-gray-600">
                    {JSON.stringify(entry.changes, null, 2)}
                  </pre>
                </details>
              )}
            </div>
          </div>
        ))}
      </div>

      {totalPages > 1 && (
        <div className="flex items-center gap-2 mt-4">
          <button
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page === 1}
            className="px-3 py-1 text-xs border border-gray-300 rounded disabled:opacity-40 hover:bg-gray-50 transition-colors"
          >
            Previous
          </button>
          <span className="text-xs text-gray-600">
            Page {page} of {totalPages}
          </span>
          <button
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page === totalPages}
            className="px-3 py-1 text-xs border border-gray-300 rounded disabled:opacity-40 hover:bg-gray-50 transition-colors"
          >
            Next
          </button>
        </div>
      )}
    </div>
  )
}

