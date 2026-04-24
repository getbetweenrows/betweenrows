import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'
import { getRole, updateRole, deleteRole } from '../api/roles'
import { RoleForm } from '../components/RoleForm'
import { RoleMemberPanel } from '../components/RoleMemberPanel'
import { RoleInheritancePanel } from '../components/RoleInheritancePanel'
import { AuditTimeline } from '../components/AuditTimeline'
import { CopyableId } from '../components/CopyableId'
import { PageHeader } from '../components/layout/PageHeader'
import { SecondaryNav, type SectionDef } from '../components/layout/SecondaryNav'
import { SectionPane, type SectionWidth } from '../components/layout/SectionPane'
import { StatusDot } from '../components/Status'
import { DangerZone, DangerRow } from '../components/DangerZone'
import { ConfirmDeleteModal } from '../components/ConfirmDeleteModal'
import { useSectionParam } from '../hooks/useSectionParam'
import type { RoleDetail } from '../types/role'
import type { RoleFormValues } from '../components/RoleForm'

type SectionId = 'details' | 'membership' | 'access' | 'activity'

interface RoleSection extends SectionDef<SectionId> {
  width: SectionWidth
}

const SECTIONS: readonly RoleSection[] = [
  { id: 'details', label: 'Details', width: 'narrow' },
  { id: 'membership', label: 'Membership', width: 'wide' },
  { id: 'access', label: 'Access grants', width: 'wide' },
  { id: 'activity', label: 'Activity', width: 'wide' },
]
const VALID_IDS: readonly SectionId[] = SECTIONS.map((s) => s.id)

export function RoleEditPage() {
  const { id } = useParams<{ id: string }>()
  const roleId = id ?? ''
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showDelete, setShowDelete] = useState(false)
  const [activeSection, selectSection] = useSectionParam<SectionId>(VALID_IDS, 'details')

  const { data: role, isLoading, isError } = useQuery({
    queryKey: ['role', roleId],
    queryFn: () => getRole(roleId),
    enabled: !!roleId,
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
      toast.success('Saved')
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to save role'
      setError(msg)
    } finally {
      setIsSubmitting(false)
    }
  }

  function invalidate() {
    queryClient.invalidateQueries({ queryKey: ['role', roleId] })
    queryClient.invalidateQueries({ queryKey: ['roles'] })
    queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
  }

  if (isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading…</div>
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
    <div className="p-6">
      <PageHeader
        breadcrumb={[
          { label: 'Roles', href: '/roles' },
          { label: role.name },
        ]}
        title={role.name}
        status={<StatusDot active={role.is_active} />}
        metadata={[
          <span key="members">
            {role.direct_member_count} direct / {role.effective_member_count} effective
            members
          </span>,
          <CopyableId key="id" id={roleId} short />,
        ]}
      />

      <div className="flex items-start gap-6">
        <SecondaryNav
          ariaLabel="Role sections"
          sections={SECTIONS}
          active={activeSection}
          onSelect={selectSection}
        />

        <div className="flex-1 min-w-0">
          {SECTIONS.map((s) => (
            <SectionPane key={s.id} active={activeSection === s.id} width={s.width}>
              {s.id === 'details' && (
                <>
                  <div className="bg-white rounded-xl border border-gray-200 p-6">
                    <RoleForm
                      initial={{ name: role.name, description: role.description }}
                      onSubmit={handleDetailsSubmit}
                      onCancel={() => navigate('/roles')}
                      submitLabel="Save changes"
                      isSubmitting={isSubmitting}
                      error={error}
                    />
                  </div>
                  <RoleDangerZone onDelete={() => setShowDelete(true)} />
                </>
              )}

              {s.id === 'membership' && (
                <div className="space-y-4">
                  <div className="bg-white rounded-xl border border-gray-200 p-6">
                    <RoleMemberPanel
                      roleId={roleId}
                      members={role.members}
                      onMemberChange={invalidate}
                    />
                  </div>
                  <div className="bg-white rounded-xl border border-gray-200 p-6">
                    <RoleInheritancePanel
                      roleId={roleId}
                      parentRoles={role.parent_roles}
                      childRoles={role.child_roles}
                      onInheritanceChange={invalidate}
                    />
                  </div>
                </div>
              )}

              {s.id === 'access' && (
                <div className="space-y-4">
                  <div className="bg-white rounded-xl border border-gray-200 p-6">
                    <DatasourceAccessSection role={role} />
                  </div>
                  <div className="bg-white rounded-xl border border-gray-200 p-6">
                    <PolicyAssignmentsSection role={role} />
                  </div>
                </div>
              )}

              {s.id === 'activity' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
                  <AuditTimeline resourceType="role" resourceId={roleId} />
                </div>
              )}
            </SectionPane>
          ))}
        </div>
      </div>

      {showDelete && (
        <DeleteRoleModal role={role} onClose={() => setShowDelete(false)} />
      )}
    </div>
  )
}

function DatasourceAccessSection({ role }: { role: RoleDetail }) {
  const navigate = useNavigate()
  return (
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
  )
}

function PolicyAssignmentsSection({ role }: { role: RoleDetail }) {
  return (
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
                <tr
                  key={`${pa.policy_name}-${pa.datasource_name}-${pa.source}`}
                  className="hover:bg-gray-50"
                >
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
  )
}

function RoleDangerZone({ onDelete }: { onDelete: () => void }) {
  return (
    <DangerZone>
      <DangerRow
        title="Delete role"
        body={
          <>
            Permanently removes the role. Users lose inherited access, datasource
            grants are revoked, and policy assignments scoped to this role are removed.
            Not recoverable.
          </>
        }
        action={
          <button
            type="button"
            onClick={onDelete}
            className="bg-red-600 text-white text-sm font-medium rounded-lg px-4 py-1.5 hover:bg-red-700 transition-colors"
          >
            Delete…
          </button>
        }
      />
    </DangerZone>
  )
}

function DeleteRoleModal({
  role,
  onClose,
}: {
  role: RoleDetail
  onClose: () => void
}) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [err, setErr] = useState<string | null>(null)

  const deactivateMutation = useMutation({
    mutationFn: () => updateRole(role.id, { is_active: false }),
    onSuccess: () => {
      toast.success(`${role.name} deactivated`)
      queryClient.invalidateQueries({ queryKey: ['role', role.id] })
      queryClient.invalidateQueries({ queryKey: ['roles'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      onClose()
    },
    onError: (e: unknown) => {
      const msg =
        (e as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to deactivate'
      setErr(msg)
    },
  })

  const deleteMutation = useMutation({
    mutationFn: () => deleteRole(role.id),
    onSuccess: () => {
      toast.success(`Deleted ${role.name}`)
      queryClient.invalidateQueries({ queryKey: ['roles'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      navigate('/roles')
    },
    onError: (e: unknown) => {
      const msg =
        (e as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to delete'
      setErr(msg)
    },
  })

  return (
    <ConfirmDeleteModal
      resourceName={role.name}
      consequences={
        <>
          <li>The role itself</li>
          <li>Direct memberships and inheritance edges involving this role</li>
          <li>Datasource access grants and policy assignments for this role</li>
        </>
      }
      softDelete={
        role.is_active
          ? {
              label: 'Deactivate instead',
              pendingLabel: 'Deactivating…',
              explanation: (
                <>
                  <span className="font-medium">Consider deactivating instead.</span>{' '}
                  Deactivation keeps the role and its assignments but removes effective
                  access — members lose the role's grants until reactivated.
                </>
              ),
              onConfirm: () => deactivateMutation.mutate(),
              pending: deactivateMutation.isPending,
            }
          : undefined
      }
      onDelete={() => deleteMutation.mutate()}
      deletePending={deleteMutation.isPending}
      onClose={onClose}
      error={err}
    />
  )
}
