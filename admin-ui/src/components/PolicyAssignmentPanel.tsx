import { useState } from 'react'
import { Link } from 'react-router-dom'
import { useQuery, useMutation } from '@tanstack/react-query'
import { listDatasourcePolicies, assignPolicy, removeAssignment } from '../api/policies'
import { listDataSources } from '../api/datasources'
import { listUsers } from '../api/users'
import type { PolicyAssignmentResponse } from '../types/policy'

// ---------- Read-only assignments summary (used on PolicyCodeView) ----------

interface PolicyAssignmentsReadonlyProps {
  assignments: PolicyAssignmentResponse[]
}

export function PolicyAssignmentsReadonly({ assignments }: PolicyAssignmentsReadonlyProps) {
  return (
    <div>
      <h2 className="text-base font-semibold text-gray-900 mb-3">Assignments</h2>
      {assignments.length === 0 ? (
        <div className="text-sm text-gray-400">No assignments yet.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Datasource</th>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">User</th>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Priority</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {assignments.map((a) => (
                <tr key={a.id} className="hover:bg-gray-50">
                  <td className="px-3 py-2">
                    <Link
                      to={`/datasources/${a.data_source_id}/edit`}
                      className="font-medium text-blue-600 hover:text-blue-800"
                    >
                      {a.datasource_name}
                    </Link>
                  </td>
                  <td className="px-3 py-2 text-gray-600">
                    {a.username ?? <span className="text-gray-400 italic">all users</span>}
                  </td>
                  <td className="px-3 py-2 text-gray-600">{a.priority}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}

// ---------- Editable assignment panel scoped to a policy (used on policy edit page) ----------

interface PolicyAssignmentEditPanelProps {
  policyId: string
  assignments: PolicyAssignmentResponse[]
  onAssignmentChange: () => void
}

export function PolicyAssignmentEditPanel({
  policyId,
  assignments,
  onAssignmentChange,
}: PolicyAssignmentEditPanelProps) {
  const [addError, setAddError] = useState<string | null>(null)
  const [selectedDatasourceId, setSelectedDatasourceId] = useState('')
  const [selectedUserId, setSelectedUserId] = useState('')
  const [priority, setPriority] = useState('100')

  const { data: datasourcesData } = useQuery({
    queryKey: ['datasources', 'all'],
    queryFn: () => listDataSources({ page_size: 200 }),
  })

  const { data: usersData } = useQuery({
    queryKey: ['users', 'all'],
    queryFn: () => listUsers({ page: 1, page_size: 100 }).then((r) => r.data),
  })

  const addMutation = useMutation({
    mutationFn: () =>
      assignPolicy(selectedDatasourceId, {
        policy_id: policyId,
        user_id: selectedUserId || null,
        priority: parseInt(priority, 10) || 100,
      }),
    onSuccess: () => {
      setAddError(null)
      setSelectedDatasourceId('')
      setSelectedUserId('')
      setPriority('100')
      onAssignmentChange()
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to assign policy'
      setAddError(msg)
    },
  })

  const removeMutation = useMutation({
    mutationFn: (a: PolicyAssignmentResponse) => removeAssignment(a.data_source_id, a.id),
    onSuccess: () => {
      onAssignmentChange()
    },
  })

  const activeDatasources = (datasourcesData?.data ?? []).filter((ds) => ds.is_active)
  const allUsers = usersData ?? []

  return (
    <div>
      <h2 className="text-base font-semibold text-gray-900 mb-3">Assignments</h2>

      {assignments.length === 0 ? (
        <div className="text-sm text-gray-400 mb-4">No assignments yet.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-hidden mb-4">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Datasource</th>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">User</th>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Priority</th>
                <th className="px-3 py-2" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {assignments.map((a) => (
                <tr key={a.id} className="hover:bg-gray-50">
                  <td className="px-3 py-2">
                    <Link
                      to={`/datasources/${a.data_source_id}/edit`}
                      className="font-medium text-blue-600 hover:text-blue-800"
                    >
                      {a.datasource_name}
                    </Link>
                  </td>
                  <td className="px-3 py-2 text-gray-600">
                    {a.username ?? <span className="text-gray-400 italic">all users</span>}
                  </td>
                  <td className="px-3 py-2 text-gray-600">{a.priority}</td>
                  <td className="px-3 py-2 text-right">
                    <button
                      onClick={() => removeMutation.mutate(a)}
                      disabled={removeMutation.isPending}
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

      {/* Add assignment form */}
      <div className="border border-gray-200 rounded-lg p-4 bg-gray-50">
        <h3 className="text-xs font-semibold text-gray-600 uppercase tracking-wide mb-3">Add Assignment</h3>
        <div className="grid grid-cols-3 gap-3">
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">Datasource</label>
            <select
              value={selectedDatasourceId}
              onChange={(e) => setSelectedDatasourceId(e.target.value)}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
            >
              <option value="">Select a datasource…</option>
              {activeDatasources.map((ds) => (
                <option key={ds.id} value={ds.id}>
                  {ds.name}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">
              User <span className="text-gray-400 font-normal">(optional)</span>
            </label>
            <select
              value={selectedUserId}
              onChange={(e) => setSelectedUserId(e.target.value)}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
            >
              <option value="">All users</option>
              {allUsers.map((u) => (
                <option key={u.id} value={u.id}>
                  {u.username}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">Priority</label>
            <input
              type="number"
              value={priority}
              onChange={(e) => setPriority(e.target.value)}
              min={1}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>
        </div>

        {addError && (
          <div className="mt-3 text-xs text-red-600">{addError}</div>
        )}

        <button
          type="button"
          onClick={() => addMutation.mutate()}
          disabled={!selectedDatasourceId || addMutation.isPending}
          className="mt-3 bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium rounded px-3 py-1.5 transition-colors disabled:opacity-50"
        >
          {addMutation.isPending ? 'Assigning…' : 'Assign policy'}
        </button>
      </div>
    </div>
  )
}

// ---------- Read-only assignment list for datasource edit page ----------

interface DatasourceAssignmentsReadonlyProps {
  datasourceId: string
}

export function DatasourceAssignmentsReadonly({ datasourceId }: DatasourceAssignmentsReadonlyProps) {
  const { data: assignments = [], isLoading } = useQuery({
    queryKey: ['datasource-policies', datasourceId],
    queryFn: () => listDatasourcePolicies(datasourceId),
  })

  return (
    <div>
      <div className="flex items-start justify-between mb-3">
        <div>
          <h2 className="text-base font-semibold text-gray-900">Policy Assignments</h2>
          <p className="text-xs text-gray-400 mt-0.5">Manage assignments from the policy edit page.</p>
        </div>
      </div>

      {isLoading ? (
        <div className="text-sm text-gray-400">Loading…</div>
      ) : assignments.length === 0 ? (
        <div className="text-sm text-gray-400">No policies assigned yet.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Policy</th>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">User</th>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Priority</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {assignments.map((a) => (
                <tr key={a.id} className="hover:bg-gray-50">
                  <td className="px-3 py-2">
                    <Link
                      to={`/policies/${a.policy_id}/edit`}
                      className="font-medium text-blue-600 hover:text-blue-800"
                    >
                      {a.policy_name}
                    </Link>
                  </td>
                  <td className="px-3 py-2 text-gray-600">
                    {a.username ?? <span className="text-gray-400 italic">all users</span>}
                  </td>
                  <td className="px-3 py-2 text-gray-600">{a.priority}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
