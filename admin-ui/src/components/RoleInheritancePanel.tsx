import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { addParent, removeParent, listRoles } from '../api/roles'
import type { RoleRef } from '../types/role'

interface RoleInheritancePanelProps {
  roleId: string
  parentRoles: RoleRef[]
  childRoles: RoleRef[]
  onInheritanceChange: () => void
}

export function RoleInheritancePanel({
  roleId,
  parentRoles,
  childRoles,
  onInheritanceChange,
}: RoleInheritancePanelProps) {
  const [showAddParent, setShowAddParent] = useState(false)
  const [selectedParentId, setSelectedParentId] = useState('')
  const [addError, setAddError] = useState<string | null>(null)

  const { data: allRolesData } = useQuery({
    queryKey: ['roles', 'all'],
    queryFn: () => listRoles({ page_size: 200 }),
    enabled: showAddParent,
  })

  const excludedIds = new Set([
    roleId,
    ...parentRoles.map((r) => r.id),
    ...childRoles.map((r) => r.id),
  ])
  const availableRoles = (allRolesData?.data ?? []).filter((r) => !excludedIds.has(r.id))

  const addParentMutation = useMutation({
    mutationFn: () => addParent(roleId, selectedParentId),
    onSuccess: () => {
      setAddError(null)
      setSelectedParentId('')
      setShowAddParent(false)
      onInheritanceChange()
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to add parent role'
      setAddError(msg)
    },
  })

  const removeParentMutation = useMutation({
    mutationFn: (parentId: string) => removeParent(roleId, parentId),
    onSuccess: () => onInheritanceChange(),
  })

  return (
    <div className="space-y-6">
      {/* Parent Roles */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-base font-semibold text-gray-900">
            Parent Roles{' '}
            <span className="text-sm font-normal text-gray-500">({parentRoles.length})</span>
          </h2>
          <button
            type="button"
            onClick={() => setShowAddParent(!showAddParent)}
            className="text-sm text-blue-600 hover:text-blue-800 font-medium"
          >
            {showAddParent ? 'Cancel' : '+ Add parent'}
          </button>
        </div>

        {showAddParent && (
          <div className="border border-gray-200 rounded-lg p-4 bg-gray-50 mb-4">
            <h3 className="text-xs font-semibold text-gray-600 uppercase tracking-wide mb-3">
              Select Parent Role
            </h3>
            {(allRolesData?.data ?? []).length >= 200 && (
              <div className="mb-2 text-xs text-amber-600 bg-amber-50 border border-amber-200 rounded px-2 py-1.5">
                Showing first 200 roles. Some roles may not be listed.
              </div>
            )}
            <select
              value={selectedParentId}
              onChange={(e) => setSelectedParentId(e.target.value)}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
            >
              <option value="">Select a role...</option>
              {availableRoles.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name}
                </option>
              ))}
            </select>

            {addError && <div className="mt-2 text-xs text-red-600">{addError}</div>}

            <button
              type="button"
              onClick={() => addParentMutation.mutate()}
              disabled={!selectedParentId || addParentMutation.isPending}
              className="mt-3 bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium rounded px-3 py-1.5 transition-colors disabled:opacity-50"
            >
              {addParentMutation.isPending ? 'Adding...' : 'Add parent role'}
            </button>
          </div>
        )}

        {parentRoles.length === 0 ? (
          <div className="text-sm text-gray-400">No parent roles.</div>
        ) : (
          <div className="border border-gray-200 rounded-lg overflow-hidden">
            <table className="w-full text-sm">
              <thead className="bg-gray-50 border-b border-gray-200">
                <tr>
                  <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Role</th>
                  <th className="px-3 py-2" />
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {parentRoles.map((role) => (
                  <tr key={role.id} className="hover:bg-gray-50">
                    <td className="px-3 py-2 text-gray-800 font-medium">{role.name}</td>
                    <td className="px-3 py-2 text-right">
                      <button
                        onClick={() => removeParentMutation.mutate(role.id)}
                        disabled={removeParentMutation.isPending}
                        className="text-xs text-red-500 hover:text-red-700 disabled:opacity-50"
                      >
                        Remove
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Child Roles (read-only) */}
      <div>
        <h2 className="text-base font-semibold text-gray-900 mb-3">
          Child Roles{' '}
          <span className="text-sm font-normal text-gray-500">({childRoles.length})</span>
        </h2>
        <p className="text-xs text-gray-400 mb-2">
          These roles inherit from this role. Manage from the child role's page.
        </p>

        {childRoles.length === 0 ? (
          <div className="text-sm text-gray-400">No child roles.</div>
        ) : (
          <div className="border border-gray-200 rounded-lg overflow-hidden">
            <table className="w-full text-sm">
              <thead className="bg-gray-50 border-b border-gray-200">
                <tr>
                  <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Role</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {childRoles.map((role) => (
                  <tr key={role.id} className="hover:bg-gray-50">
                    <td className="px-3 py-2 text-gray-800 font-medium">{role.name}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  )
}
