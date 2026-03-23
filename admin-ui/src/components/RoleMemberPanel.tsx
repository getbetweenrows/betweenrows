import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { addMembers, removeMember, getEffectiveMembers } from '../api/roles'
import { listUsers } from '../api/users'
import type { RoleMember } from '../types/role'

interface RoleMemberPanelProps {
  roleId: string
  members: RoleMember[]
  onMemberChange: () => void
}

export function RoleMemberPanel({ roleId, members, onMemberChange }: RoleMemberPanelProps) {
  const [showAddForm, setShowAddForm] = useState(false)
  const [selectedUserIds, setSelectedUserIds] = useState<string[]>([])
  const [addError, setAddError] = useState<string | null>(null)
  const [userSearch, setUserSearch] = useState('')

  const { data: usersData } = useQuery({
    queryKey: ['users', 'all'],
    queryFn: () => listUsers({ page: 1, page_size: 200 }),
    enabled: showAddForm,
  })

  const { data: effectiveMembers = [] } = useQuery({
    queryKey: ['role-effective-members', roleId],
    queryFn: () => getEffectiveMembers(roleId),
  })

  const directMemberIds = new Set(members.map((m) => m.id))
  const availableUsers = (usersData?.data ?? []).filter(
    (u) => !directMemberIds.has(u.id) && u.username.toLowerCase().includes(userSearch.toLowerCase()),
  )

  const addMutation = useMutation({
    mutationFn: () => addMembers(roleId, selectedUserIds),
    onSuccess: () => {
      setAddError(null)
      setSelectedUserIds([])
      setShowAddForm(false)
      onMemberChange()
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to add members'
      setAddError(msg)
    },
  })

  const removeMutation = useMutation({
    mutationFn: (userId: string) => removeMember(roleId, userId),
    onSuccess: () => onMemberChange(),
  })

  function toggleUser(userId: string) {
    setSelectedUserIds((prev) =>
      prev.includes(userId) ? prev.filter((id) => id !== userId) : [...prev, userId],
    )
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-3">
        <h2 className="text-base font-semibold text-gray-900">
          Members <span className="text-sm font-normal text-gray-500">({effectiveMembers.length} effective)</span>
        </h2>
        <button
          type="button"
          onClick={() => setShowAddForm(!showAddForm)}
          className="text-sm text-blue-600 hover:text-blue-800 font-medium"
        >
          {showAddForm ? 'Cancel' : '+ Add members'}
        </button>
      </div>

      {showAddForm && (
        <div className="border border-gray-200 rounded-lg p-4 bg-gray-50 mb-4">
          <h3 className="text-xs font-semibold text-gray-600 uppercase tracking-wide mb-3">
            Select Users to Add
          </h3>
          <input
            type="search"
            value={userSearch}
            onChange={(e) => setUserSearch(e.target.value)}
            placeholder="Search users..."
            className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500 mb-2"
          />
          <div className="max-h-48 overflow-y-auto border border-gray-200 rounded bg-white">
            {availableUsers.length === 0 ? (
              <div className="px-3 py-2 text-xs text-gray-400">No available users found.</div>
            ) : (
              availableUsers.map((user) => (
                <label
                  key={user.id}
                  className="flex items-center gap-2 px-3 py-1.5 hover:bg-gray-50 cursor-pointer text-sm"
                >
                  <input
                    type="checkbox"
                    checked={selectedUserIds.includes(user.id)}
                    onChange={() => toggleUser(user.id)}
                    className="rounded"
                  />
                  <span className="text-gray-800">{user.username}</span>
                </label>
              ))
            )}
          </div>

          {addError && <div className="mt-2 text-xs text-red-600">{addError}</div>}

          <button
            type="button"
            onClick={() => addMutation.mutate()}
            disabled={selectedUserIds.length === 0 || addMutation.isPending}
            className="mt-3 bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium rounded px-3 py-1.5 transition-colors disabled:opacity-50"
          >
            {addMutation.isPending
              ? 'Adding...'
              : `Add ${selectedUserIds.length} user${selectedUserIds.length !== 1 ? 's' : ''}`}
          </button>
        </div>
      )}

      {effectiveMembers.length === 0 ? (
        <div className="text-sm text-gray-400">No members yet.</div>
      ) : (
        <div className="border border-gray-200 rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Username</th>
                <th className="text-left px-3 py-2 font-medium text-gray-600 text-xs">Source</th>
                <th className="px-3 py-2" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {effectiveMembers.map((member) => (
                <tr key={member.user_id} className="hover:bg-gray-50">
                  <td className="px-3 py-2 text-gray-800">{member.username}</td>
                  <td className="px-3 py-2">
                    <span
                      className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                        member.source === 'direct'
                          ? 'bg-blue-100 text-blue-700'
                          : 'bg-purple-100 text-purple-700'
                      }`}
                    >
                      {member.source}
                    </span>
                  </td>
                  <td className="px-3 py-2 text-right">
                    {directMemberIds.has(member.user_id) && (
                      <button
                        onClick={() => removeMutation.mutate(member.user_id)}
                        disabled={removeMutation.isPending}
                        className="text-xs text-red-500 hover:text-red-700 disabled:opacity-50"
                      >
                        Remove
                      </button>
                    )}
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
