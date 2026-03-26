import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { getPolicy, updatePolicy } from '../api/policies'
import { getDecisionFunction } from '../api/decisionFunctions'
import { PolicyForm } from '../components/PolicyForm'
import type { PolicyFormValues } from '../components/PolicyForm'
import { PolicyAssignmentEditPanel } from '../components/PolicyAssignmentPanel'
import { PolicyCodeView } from '../components/PolicyCodeView'
import { AuditTimeline } from '../components/AuditTimeline'
import { useCatalogHints } from '../hooks/useCatalogHints'

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

  // Load the full decision function if the policy has one attached
  const decisionFnId = policy?.decision_function_id ?? null
  const { data: decisionFunction, isLoading: decisionFnLoading } = useQuery({
    queryKey: ['decision-function', decisionFnId],
    queryFn: () => getDecisionFunction(decisionFnId!),
    enabled: !!decisionFnId,
  })

  const hintDatasourceId = policy?.assignments?.[0]?.data_source_id ?? ''
  const catalogHints = useCatalogHints(hintDatasourceId)

  async function handleSubmit(values: PolicyFormValues) {
    if (!policy) return
    setIsSubmitting(true)
    setError(null)
    try {
      await updatePolicy(policyId, {
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
        decision_function_id: values.decision_function_id,
        version: policy.version,
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
          initialDecisionFunction={decisionFunction ?? null}
          decisionFnLoading={!!decisionFnId && decisionFnLoading}
          onSubmit={handleSubmit}
          submitLabel="Save changes"
          isSubmitting={isSubmitting}
          error={error}
          catalogHints={catalogHints}
        />
      </div>

      <div className="bg-white rounded-xl border border-gray-200 p-6 mt-4">
        <PolicyAssignmentEditPanel
          policyId={policyId}
          assignments={policy.assignments ?? []}
          onAssignmentChange={() =>
            queryClient.invalidateQueries({ queryKey: ['policy', policyId] })
          }
        />
      </div>

      <PolicyCodeView policy={policy} assignments={policy.assignments ?? []} />

      <div className="bg-white rounded-xl border border-gray-200 p-6 mt-4">
        <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
        <AuditTimeline resourceType="policy" resourceId={policyId} />
      </div>
    </div>
  )
}
