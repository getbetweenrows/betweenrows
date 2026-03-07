import { useState } from 'react'
import { Link } from 'react-router-dom'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { listDatasourcePolicies, assignPolicy, removeAssignment } from '../api/policies'
import { listPolicies } from '../api/policies'
import { listUsers } from '../api/users'
import type { PolicyAssignmentResponse } from '../types/policy'

// ---------- Read-only assignments summary (used on policy edit page) ----------

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

// ---------- Editable assignment panel (used on datasource edit page) ----------

interface PolicyAssignmentPanelProps {
  datasourceId: string
}

export function PolicyAssignmentPanel({ datasourceId }: PolicyAssignmentPanelProps) {
  const queryClient = useQueryClient()
  const [addError, setAddError] = useState<string | null>(null)
  const [selectedPolicyId, setSelectedPolicyId] = useState('')
  const [selectedUserId, setSelectedUserId] = useState('')
  const [priority, setPriority] = useState('100')

  const { data: assignments = [], isLoading: assignmentsLoading } = useQuery({
    queryKey: ['datasource-policies', datasourceId],
    queryFn: () => listDatasourcePolicies(datasourceId),
  })

  const { data: policiesData } = useQuery({
    queryKey: ['policies', 1, ''],
    queryFn: () => listPolicies({ page: 1, page_size: 100 }),
  })

  const { data: usersData } = useQuery({
    queryKey: ['users', 'all'],
    queryFn: () => listUsers({ page: 1, page_size: 100 }).then((r) => r.data),
  })

  const addMutation = useMutation({
    mutationFn: () =>
      assignPolicy(datasourceId, {
        policy_id: selectedPolicyId,
        user_id: selectedUserId || null,
        priority: parseInt(priority, 10) || 100,
      }),
    onSuccess: () => {
      setAddError(null)
      setSelectedPolicyId('')
      setSelectedUserId('')
      setPriority('100')
      queryClient.invalidateQueries({ queryKey: ['datasource-policies', datasourceId] })
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to assign policy'
      setAddError(msg)
    },
  })

  const removeMutation = useMutation({
    mutationFn: (assignmentId: string) => removeAssignment(datasourceId, assignmentId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['datasource-policies', datasourceId] })
    },
  })

  const allPolicies = policiesData?.data ?? []
  const allUsers = usersData ?? []

  return (
    <div>
      <h2 className="text-base font-semibold text-gray-900 mb-3">Policy Assignments</h2>
      <p className="text-sm text-gray-500 mb-4">
        Assign policies to this data source. Optionally scope to a specific user; leave blank to apply to all users.
      </p>

      {assignmentsLoading ? (
        <div className="text-sm text-gray-400 mb-4">Loading…</div>
      ) : assignments.length === 0 ? (
        <div className="text-sm text-gray-400 mb-4">No policies assigned yet.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-hidden mb-4">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Policy</th>
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
                      to={`/policies/${a.policy_id}/edit`}
                      className="font-medium text-blue-600 hover:text-blue-800"
                    >
                      {a.policy_name}
                    </Link>
                  </td>
                  <td className="px-3 py-2 text-gray-600">{a.username ?? <span className="text-gray-400 italic">all users</span>}</td>
                  <td className="px-3 py-2 text-gray-600">{a.priority}</td>
                  <td className="px-3 py-2 text-right">
                    <button
                      onClick={() => removeMutation.mutate(a.id)}
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
            <label className="block text-xs font-medium text-gray-600 mb-1">Policy</label>
            <select
              value={selectedPolicyId}
              onChange={(e) => setSelectedPolicyId(e.target.value)}
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
            >
              <option value="">Select a policy…</option>
              {allPolicies.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} ({p.effect})
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
          disabled={!selectedPolicyId || addMutation.isPending}
          className="mt-3 bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium rounded px-3 py-1.5 transition-colors disabled:opacity-50"
        >
          {addMutation.isPending ? 'Assigning…' : 'Assign policy'}
        </button>
      </div>
    </div>
  )
}
