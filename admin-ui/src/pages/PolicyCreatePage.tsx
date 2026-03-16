import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { createPolicy } from '../api/policies'
import { PolicyForm } from '../components/PolicyForm'
import type { PolicyFormValues } from '../components/PolicyForm'

export function PolicyCreatePage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleSubmit(values: PolicyFormValues) {
    setIsSubmitting(true)
    setError(null)
    try {
      await createPolicy({
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

      <div className="bg-white rounded-xl border border-gray-200 p-6">
        <PolicyForm
          onSubmit={handleSubmit}
          submitLabel="Create policy"
          isSubmitting={isSubmitting}
          error={error}
        />
      </div>
    </div>
  )
}
