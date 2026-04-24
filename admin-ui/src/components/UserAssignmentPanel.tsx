import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'
import { getDataSourceUsers, setDataSourceUsers } from '../api/datasources'
import { listUsers } from '../api/users'

interface UserAssignmentPanelProps {
  datasourceId: string
}

export function UserAssignmentPanel({ datasourceId }: UserAssignmentPanelProps) {
  const queryClient = useQueryClient()

  const { data: allUsers = [], isLoading: usersLoading } = useQuery({
    queryKey: ['users', 'all'],
    queryFn: () => listUsers({ page: 1, page_size: 100 }).then((r) => r.data),
  })

  const { data: assignedUsers = [], isLoading: assignedLoading } = useQuery({
    queryKey: ['datasource-users', datasourceId],
    queryFn: () => getDataSourceUsers(datasourceId),
  })

  const [selected, setSelected] = useState<Set<string> | null>(null)

  // Derive selected set from loaded assigned users (only on first load)
  const assignedIds = new Set(assignedUsers.map((u) => u.id))
  const effectiveSelected = selected ?? assignedIds

  const saveMutation = useMutation({
    mutationFn: (ids: string[]) => setDataSourceUsers(datasourceId, ids),
    onSuccess: () => {
      toast.success('User assignments saved')
      queryClient.invalidateQueries({ queryKey: ['datasource-users', datasourceId] })
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to save user assignments'
      toast.error(msg)
    },
  })

  function toggleUser(id: string) {
    setSelected((prev) => {
      const base = prev ?? assignedIds
      const next = new Set(base)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  function handleSave() {
    saveMutation.mutate([...effectiveSelected])
  }

  const isLoading = usersLoading || assignedLoading

  return (
    <div>
      <h2 className="text-base font-semibold text-gray-900 mb-3">User Access</h2>
      <p className="text-sm text-gray-500 mb-4">
        Only assigned users can connect to this data source via pgwire. Being an admin does not
        automatically grant data plane access.
      </p>

      {isLoading ? (
        <div className="text-sm text-gray-400">Loading…</div>
      ) : allUsers.length === 0 ? (
        <div className="text-sm text-gray-400">No users found.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-hidden">
          {allUsers.map((user, idx) => (
            <label
              key={user.id}
              className={`flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-gray-50 transition-colors ${
                idx > 0 ? 'border-t border-gray-100' : ''
              }`}
            >
              <input
                type="checkbox"
                checked={effectiveSelected.has(user.id)}
                onChange={() => toggleUser(user.id)}
                className="rounded border-gray-300 text-blue-600 focus:ring-blue-500"
              />
              <div className="flex-1 min-w-0">
                <span className="text-sm font-medium text-gray-900">{user.username}</span>
              </div>
              {user.is_admin && (
                <span className="text-xs bg-purple-100 text-purple-700 rounded-full px-2 py-0.5 font-medium">
                  Admin
                </span>
              )}
              {!user.is_active && (
                <span className="text-xs bg-red-100 text-red-700 rounded-full px-2 py-0.5 font-medium">
                  Inactive
                </span>
              )}
            </label>
          ))}
        </div>
      )}

      <div className="flex items-center gap-3 mt-4">
        <button
          type="button"
          onClick={handleSave}
          disabled={saveMutation.isPending || isLoading}
          className="bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg px-4 py-2 transition-colors disabled:opacity-50"
        >
          {saveMutation.isPending ? 'Saving…' : 'Save assignments'}
        </button>
      </div>
    </div>
  )
}
