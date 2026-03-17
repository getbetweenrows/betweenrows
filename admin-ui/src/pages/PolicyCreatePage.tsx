import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { createPolicy, assignPolicy } from '../api/policies'
import { listDataSources } from '../api/datasources'
import { listUsers } from '../api/users'
import { PolicyForm } from '../components/PolicyForm'
import type { PolicyFormValues } from '../components/PolicyForm'
import { useCatalogHints } from '../hooks/useCatalogHints'

export function PolicyCreatePage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selectedDatasourceId, setSelectedDatasourceId] = useState('')
  const [assignUserId, setAssignUserId] = useState('')
  const [assignPriority, setAssignPriority] = useState('100')

  const { data: datasourcesData } = useQuery({
    queryKey: ['datasources', 'all'],
    queryFn: () => listDataSources({ page_size: 200 }),
  })

  const activeDatasources = (datasourcesData?.data ?? []).filter((ds) => ds.is_active)

  const catalogHints = useCatalogHints(selectedDatasourceId)

  const { data: usersData } = useQuery({
    queryKey: ['users', 'all'],
    queryFn: () => listUsers({ page: 1, page_size: 100 }).then((r) => r.data),
  })

  async function handleSubmit(values: PolicyFormValues) {
    if (!selectedDatasourceId) return
    setIsSubmitting(true)
    setError(null)
    try {
      const policy = await createPolicy({
        name: values.name,
        description: values.description || undefined,
        policy_type: values.policy_type,
        is_enabled: values.is_enabled,
        targets: values.targets,
        definition:
          values.policy_type === 'row_filter'
            ? { filter_expression: values.filter_expression }
            : values.policy_type === 'column_mask'
              ? { mask_expression: values.mask_expression }
              : null,
      })
      try {
        await assignPolicy(selectedDatasourceId, {
          policy_id: policy.id,
          user_id: assignUserId || null,
          priority: parseInt(assignPriority, 10) || 100,
        })
      } catch (assignErr: unknown) {
        const msg =
          (assignErr as { response?: { data?: { error?: string } } })?.response?.data?.error ??
          'Policy created but assignment failed — assign it manually from the datasource page.'
        setError(msg)
        setIsSubmitting(false)
        return
      }
      queryClient.invalidateQueries({ queryKey: ['policies'] })
      navigate('/policies')
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to create policy'
      setError(msg)
    } finally {
      setIsSubmitting(false)
    }
  }

  const allUsers = usersData ?? []

  return (
    <div className="p-6 max-w-2xl">
      <div className="mb-6">
        <button
          onClick={() => navigate('/policies')}
          className="text-sm text-gray-500 hover:text-gray-700 mb-2"
        >
          ← Back to Policies
        </button>
        <h1 className="text-xl font-bold text-gray-900">Create Policy</h1>
      </div>

      {/* 1. Datasource picker */}
      <div className="bg-white rounded-xl border border-gray-200 p-6 mb-4">
        <div className="mb-4">
          <label className="block text-sm font-medium text-gray-700 mb-1">Datasource</label>
          <select
            value={selectedDatasourceId}
            onChange={(e) => setSelectedDatasourceId(e.target.value)}
            className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            <option value="">Select a datasource…</option>
            {activeDatasources.map((ds) => (
              <option key={ds.id} value={ds.id}>
                {ds.name}
              </option>
            ))}
          </select>
          <p className="text-xs text-gray-400 mt-1">
            Policy will be auto-assigned to this datasource on creation.
          </p>
        </div>

        {/* Assignment options — shown alongside datasource selection */}
        {selectedDatasourceId && (
          <div className="grid grid-cols-2 gap-4 pt-4 border-t border-gray-100">
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">
                Assign to user <span className="text-gray-400 font-normal">(optional)</span>
              </label>
              <select
                value={assignUserId}
                onChange={(e) => setAssignUserId(e.target.value)}
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
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
                value={assignPriority}
                onChange={(e) => setAssignPriority(e.target.value)}
                min={1}
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
              />
            </div>
          </div>
        )}
      </div>

      {/* 2. PolicyForm — only shown after datasource selected */}
      {selectedDatasourceId && (
        <div className="bg-white rounded-xl border border-gray-200 p-6">
          <PolicyForm
            onSubmit={handleSubmit}
            submitLabel="Create & assign policy"
            isSubmitting={isSubmitting}
            error={error}
            catalogHints={catalogHints}
          />
        </div>
      )}
    </div>
  )
}
