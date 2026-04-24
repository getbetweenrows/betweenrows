import { useState, useEffect } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'
import { listRoles, getDatasourceRoles, setDatasourceRoleAccess } from '../api/roles'

interface RoleAccessPanelProps {
  datasourceId: string
}

export function RoleAccessPanel({ datasourceId }: RoleAccessPanelProps) {
  const queryClient = useQueryClient()
  const [selectedRoleIds, setSelectedRoleIds] = useState<Set<string>>(new Set())
  const [initialized, setInitialized] = useState(false)

  const { data: allRolesData } = useQuery({
    queryKey: ['roles', 'all'],
    queryFn: () => listRoles({ page_size: 200 }),
  })

  const { data: assignedRoles, isLoading } = useQuery({
    queryKey: ['datasource-roles', datasourceId],
    queryFn: () => getDatasourceRoles(datasourceId),
  })

  useEffect(() => {
    if (assignedRoles && !initialized) {
      setSelectedRoleIds(new Set(assignedRoles.map((r) => r.id)))
      setInitialized(true)
    }
  }, [assignedRoles, initialized])

  const saveMutation = useMutation({
    mutationFn: () => setDatasourceRoleAccess(datasourceId, Array.from(selectedRoleIds)),
    onSuccess: () => {
      toast.success('Role access saved')
      queryClient.invalidateQueries({ queryKey: ['datasource-roles', datasourceId] })
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to save role access'
      toast.error(msg)
    },
  })

  const allRoles = allRolesData?.data ?? []

  function toggleRole(roleId: string) {
    setSelectedRoleIds((prev) => {
      const next = new Set(prev)
      if (next.has(roleId)) {
        next.delete(roleId)
      } else {
        next.add(roleId)
      }
      return next
    })
  }

  const assignedSet = new Set((assignedRoles ?? []).map((r) => r.id))
  const hasChanges =
    selectedRoleIds.size !== assignedSet.size ||
    Array.from(selectedRoleIds).some((id) => !assignedSet.has(id))

  if (isLoading) {
    return <div className="text-sm text-gray-400">Loading...</div>
  }

  return (
    <div>
      <div className="flex items-start justify-between mb-3">
        <div>
          <h2 className="text-base font-semibold text-gray-900">Role Access</h2>
          <p className="text-xs text-gray-400 mt-0.5">
            Select which roles have access to this datasource.
          </p>
        </div>
      </div>

      {allRoles.length >= 200 && (
        <div className="mb-2 text-xs text-amber-600 bg-amber-50 border border-amber-200 rounded px-2 py-1.5">
          Showing first 200 roles. Some roles may not be listed.
        </div>
      )}

      {allRoles.length === 0 ? (
        <div className="text-sm text-gray-400">No roles exist yet.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-hidden">
          <div className="max-h-64 overflow-y-auto">
            {allRoles.map((role) => (
              <label
                key={role.id}
                className="flex items-center gap-3 px-3 py-2 hover:bg-gray-50 cursor-pointer border-b border-gray-100 last:border-b-0"
              >
                <input
                  type="checkbox"
                  checked={selectedRoleIds.has(role.id)}
                  onChange={() => toggleRole(role.id)}
                  className="rounded"
                />
                <span className="text-sm text-gray-800 flex-1">{role.name}</span>
                <span
                  className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                    role.is_active
                      ? 'bg-green-100 text-green-700'
                      : 'bg-gray-100 text-gray-500'
                  }`}
                >
                  {role.is_active ? 'Active' : 'Inactive'}
                </span>
              </label>
            ))}
          </div>
        </div>
      )}

      <button
        type="button"
        onClick={() => saveMutation.mutate()}
        disabled={!hasChanges || saveMutation.isPending}
        className="mt-3 bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium rounded px-3 py-1.5 transition-colors disabled:opacity-50"
      >
        {saveMutation.isPending ? 'Saving...' : 'Save role access'}
      </button>
    </div>
  )
}
