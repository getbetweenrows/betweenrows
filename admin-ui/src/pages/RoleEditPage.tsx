import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { getRole, updateRole, deleteRole } from '../api/roles'
import { RoleForm } from '../components/RoleForm'
import { RoleMemberPanel } from '../components/RoleMemberPanel'
import { RoleInheritancePanel } from '../components/RoleInheritancePanel'
import { AuditTimeline } from '../components/AuditTimeline'
import { CopyableId } from '../components/CopyableId'
import type { RoleFormValues } from '../components/RoleForm'

type Tab = 'details' | 'members' | 'inheritance' | 'datasources' | 'policies' | 'activity'

const TABS: { key: Tab; label: string }[] = [
  { key: 'details', label: 'Details' },
  { key: 'members', label: 'Members' },
  { key: 'inheritance', label: 'Inheritance' },
  { key: 'datasources', label: 'Data Sources' },
  { key: 'policies', label: 'Policies' },
  { key: 'activity', label: 'Activity' },
]

export function RoleEditPage() {
  const { id } = useParams<{ id: string }>()
  const roleId = id ?? ''
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [activeTab, setActiveTab] = useState<Tab>('details')
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const { data: role, isLoading, isError } = useQuery({
    queryKey: ['role', roleId],
    queryFn: () => getRole(roleId),
    enabled: !!roleId,
  })

  const toggleActiveMutation = useMutation({
    mutationFn: (isActive: boolean) => updateRole(roleId, { is_active: isActive }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['role', roleId] })
      queryClient.invalidateQueries({ queryKey: ['roles'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
    },
  })

  const deleteMutation = useMutation({
    mutationFn: () => deleteRole(roleId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['roles'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      navigate('/roles', { replace: true })
    },
  })

  async function handleDetailsSubmit(values: RoleFormValues) {
    setError(null)
    setIsSubmitting(true)
    try {
      await updateRole(roleId, {
        name: values.name,
        description: values.description || undefined,
      })
      queryClient.invalidateQueries({ queryKey: ['role', roleId] })
      queryClient.invalidateQueries({ queryKey: ['roles'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      setError(null)
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to save role'
      setError(msg)
    } finally {
      setIsSubmitting(false)
    }
  }

  function handleDelete() {
    if (!confirm(`Delete role "${role?.name}"? This cannot be undone.`)) return
    deleteMutation.mutate()
  }

  function handleRoleDataChange() {
    queryClient.invalidateQueries({ queryKey: ['role', roleId] })
    queryClient.invalidateQueries({ queryKey: ['roles'] })
    queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
  }

  if (isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading...</div>
  }

  if (isError || !role) {
    return (
      <div className="p-6 text-sm text-red-500">
        Role not found.{' '}
        <button onClick={() => navigate('/roles')} className="underline">
          Go back
        </button>
      </div>
    )
  }

  return (
    <div className="p-6 max-w-3xl">
      {/* Header */}
      <div className="mb-6">
        <button
          onClick={() => navigate('/roles')}
          className="text-sm text-gray-500 hover:text-gray-700 mb-2"
        >
          &larr; Back to Roles
        </button>
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-bold text-gray-900">
              {role.name}
            </h1>
            <CopyableId id={roleId} />
            <div className="flex items-center gap-3 mt-1">
              <span
                className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                  role.is_active
                    ? 'bg-green-100 text-green-700'
                    : 'bg-gray-100 text-gray-500'
                }`}
              >
                {role.is_active ? 'Active' : 'Inactive'}
              </span>
              <span className="text-xs text-gray-400">
                {role.direct_member_count} direct / {role.effective_member_count} effective members
              </span>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => toggleActiveMutation.mutate(!role.is_active)}
              disabled={toggleActiveMutation.isPending}
              className="text-sm border border-gray-300 rounded-lg px-3 py-1.5 hover:bg-gray-50 transition-colors disabled:opacity-50"
            >
              {role.is_active ? 'Deactivate' : 'Activate'}
            </button>
            <button
              onClick={handleDelete}
              disabled={deleteMutation.isPending}
              className="text-sm text-red-600 border border-red-200 rounded-lg px-3 py-1.5 hover:bg-red-50 transition-colors disabled:opacity-50"
            >
              Delete
            </button>
          </div>
        </div>
      </div>

      {/* Tabs */}
      <div className="border-b border-gray-200 mb-6">
        <nav className="flex gap-6">
          {TABS.map((tab) => (
            <button
              key={tab.key}
              onClick={() => setActiveTab(tab.key)}
              className={`pb-2 text-sm font-medium border-b-2 transition-colors ${
                activeTab === tab.key
                  ? 'border-blue-600 text-blue-600'
                  : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'
              }`}
            >
              {tab.label}
            </button>
          ))}
        </nav>
      </div>

      {/* Tab content */}
      <div className="bg-white rounded-xl border border-gray-200 p-6">
        {activeTab === 'details' && (
          <RoleForm
            initial={{ name: role.name, description: role.description }}
            onSubmit={handleDetailsSubmit}
            onCancel={() => navigate('/roles')}
            submitLabel="Save changes"
            isSubmitting={isSubmitting}
            error={error}
          />
        )}

        {activeTab === 'members' && (
          <RoleMemberPanel
            roleId={roleId}
            members={role.members}
            onMemberChange={handleRoleDataChange}
          />
        )}

        {activeTab === 'inheritance' && (
          <RoleInheritancePanel
            roleId={roleId}
            parentRoles={role.parent_roles}
            childRoles={role.child_roles}
            onInheritanceChange={handleRoleDataChange}
          />
        )}

        {activeTab === 'datasources' && (
          <div>
            <h2 className="text-base font-semibold text-gray-900 mb-3">Data Source Access</h2>
            {role.datasource_access.length === 0 ? (
              <div className="text-sm text-gray-400">
                No datasource access granted to this role.
              </div>
            ) : (
              <div className="border border-gray-200 rounded-lg overflow-hidden">
                <table className="w-full text-sm">
                  <thead className="bg-gray-50 border-b border-gray-200">
                    <tr>
                      <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Datasource</th>
                      <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Source</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-100">
                    {role.datasource_access.map((da) => (
                      <tr key={da.datasource_id} className="hover:bg-gray-50">
                        <td className="px-3 py-2">
                          <button
                            onClick={() => navigate(`/datasources/${da.datasource_id}/edit`)}
                            className="font-medium text-blue-600 hover:text-blue-800"
                          >
                            {da.datasource_name}
                          </button>
                        </td>
                        <td className="px-3 py-2">
                          <span
                            className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                              da.source === 'direct'
                                ? 'bg-blue-100 text-blue-700'
                                : 'bg-purple-100 text-purple-700'
                            }`}
                          >
                            {da.source}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        )}

        {activeTab === 'policies' && (
          <div>
            <h2 className="text-base font-semibold text-gray-900 mb-3">Policy Assignments</h2>
            {role.policy_assignments.length === 0 ? (
              <div className="text-sm text-gray-400">
                No policies assigned to this role yet.
              </div>
            ) : (
              <div className="border border-gray-200 rounded-lg overflow-hidden">
                <table className="w-full text-sm">
                  <thead className="bg-gray-50 border-b border-gray-200">
                    <tr>
                      <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Policy</th>
                      <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Datasource</th>
                      <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Source</th>
                      <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Priority</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-100">
                    {role.policy_assignments.map((pa) => (
                      <tr key={`${pa.policy_name}-${pa.datasource_name}-${pa.source}`} className="hover:bg-gray-50">
                        <td className="px-3 py-2 font-medium text-gray-800">{pa.policy_name}</td>
                        <td className="px-3 py-2 text-gray-600">{pa.datasource_name}</td>
                        <td className="px-3 py-2">
                          <span
                            className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                              pa.source === 'direct'
                                ? 'bg-blue-100 text-blue-700'
                                : 'bg-purple-100 text-purple-700'
                            }`}
                          >
                            {pa.source}
                          </span>
                        </td>
                        <td className="px-3 py-2 text-gray-600">{pa.priority}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        )}

        {activeTab === 'activity' && (
          <div>
            <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
            <AuditTimeline resourceType="role" resourceId={roleId} />
          </div>
        )}
      </div>
    </div>
  )
}
