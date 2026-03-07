import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { getPolicy, updatePolicy } from '../api/policies'
import { PolicyForm } from '../components/PolicyForm'
import type { PolicyFormValues } from '../components/PolicyForm'
import { PolicyAssignmentsReadonly } from '../components/PolicyAssignmentPanel'
import { PolicyCodeView } from '../components/PolicyCodeView'

export function PolicyEditPage() {
  const { id } = useParams<{ id: string }>()
  const policyId = id ?? ''
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const { data: policy, isLoading, isError } = useQuery({
    queryKey: ['policy', policyId],
    queryFn: () => getPolicy(policyId),
    enabled: !!policyId,
  })

  async function handleSubmit(values: PolicyFormValues) {
    if (!policy) return
    setIsSubmitting(true)
    setError(null)
    try {
      await updatePolicy(policyId, {
        name: values.name,
        description: values.description || undefined,
        effect: values.effect,
        is_enabled: values.is_enabled,
        version: policy.version,
        obligations: values.obligations,
      })
      queryClient.invalidateQueries({ queryKey: ['policies'] })
      queryClient.invalidateQueries({ queryKey: ['policy', policyId] })
      navigate('/policies')
    } catch (err: unknown) {
      const apiErr = err as { response?: { status?: number; data?: { error?: string } } }
      if (apiErr?.response?.status === 409) {
        setError(
          'This policy was modified by someone else. Please go back and reload before editing.',
        )
      } else {
        setError(apiErr?.response?.data?.error ?? 'Failed to save policy')
      }
    } finally {
      setIsSubmitting(false)
    }
  }

  if (isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading…</div>
  }

  if (isError || !policy) {
    return (
      <div className="p-6 text-sm text-red-500">
        Policy not found.{' '}
        <button onClick={() => navigate('/policies')} className="underline">
          Go back
        </button>
      </div>
    )
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
        <h1 className="text-xl font-bold text-gray-900">
          Edit: <span className="font-mono text-lg">{policy.name}</span>
        </h1>
        <p className="text-sm text-gray-500 mt-1">Version {policy.version}</p>
      </div>

      <div className="bg-white rounded-xl border border-gray-200 p-6">
        <PolicyForm
          initial={policy}
          onSubmit={handleSubmit}
          submitLabel="Save changes"
          isSubmitting={isSubmitting}
          error={error}
        />
      </div>

      <div className="bg-white rounded-xl border border-gray-200 p-6 mt-4">
        <PolicyAssignmentsReadonly assignments={policy.assignments ?? []} />
      </div>

      <PolicyCodeView policy={policy} assignments={policy.assignments ?? []} />
    </div>
  )
}
