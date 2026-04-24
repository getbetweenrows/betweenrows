import { useMemo, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'
import { deletePolicy, getPolicy, getPolicyAnchorCoverage, updatePolicy } from '../api/policies'
import { PolicyForm } from '../components/PolicyForm'
import type { PolicyFormValues } from '../components/PolicyForm'
import { PolicyAssignmentEditPanel } from '../components/PolicyAssignmentPanel'
import { PolicyAnchorCoveragePanel } from '../components/PolicyAnchorCoveragePanel'
import { PolicyCodeView } from '../components/PolicyCodeView'
import { AuditTimeline } from '../components/AuditTimeline'
import { useCatalogHints } from '../hooks/useCatalogHints'
import { CopyableId } from '../components/CopyableId'
import { PageHeader } from '../components/layout/PageHeader'
import { SecondaryNav, type SectionDef } from '../components/layout/SecondaryNav'
import { SectionPane, type SectionWidth } from '../components/layout/SectionPane'
import { StatusDot, StatusChip } from '../components/Status'
import { DangerZone, DangerRow } from '../components/DangerZone'
import { ConfirmDeleteModal } from '../components/ConfirmDeleteModal'
import { useSectionParam } from '../hooks/useSectionParam'
import type { PolicyAnchorCoverageResponse, PolicyResponse } from '../types/policy'

const SECTION_WIDTH_CLASSES: Record<SectionWidth, string> = {
  narrow: 'max-w-2xl',
  wide: 'max-w-5xl',
  full: 'max-w-none',
}

type SectionId = 'details' | 'assignments' | 'coverage' | 'code' | 'activity'

interface PolicySection extends SectionDef<SectionId> {
  width: SectionWidth
}

/// Count broken-anchor entries (those with at least one missing-anchor or
/// alias-missing-target verdict) so the side-nav can flag the section.
function countBrokenCoverageEntries(
  data: PolicyAnchorCoverageResponse | undefined,
): number {
  if (!data) return 0
  return data.coverage.filter((entry) =>
    entry.verdicts.some(
      (v) => v.kind === 'missing_anchor' || v.kind === 'missing_column_on_alias_target',
    ),
  ).length
}

function sectionsFor(
  policy: PolicyResponse,
  brokenCoverageCount: number,
): readonly PolicySection[] {
  const all: PolicySection[] = [
    { id: 'details', label: 'Details', group: 'Configuration', width: 'narrow' },
    { id: 'assignments', label: 'Assignments', group: 'Access', width: 'wide' },
    {
      id: 'coverage',
      label: 'Anchor coverage',
      group: 'Schema',
      width: 'wide',
      indicator:
        brokenCoverageCount > 0
          ? {
              tone: 'red',
              label: String(brokenCoverageCount),
              ariaLabel: `${brokenCoverageCount} table${brokenCoverageCount === 1 ? '' : 's'} will silently deny`,
            }
          : undefined,
    },
    { id: 'code', label: 'View as code', group: 'Schema', width: 'wide' },
    { id: 'activity', label: 'Activity', group: 'History', width: 'wide' },
  ]
  // Anchor coverage only makes sense for row_filter policies.
  return all.filter((s) => s.id !== 'coverage' || policy.policy_type === 'row_filter')
}

export function PolicyEditPage() {
  const { id } = useParams<{ id: string }>()
  const policyId = id ?? ''
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showDelete, setShowDelete] = useState(false)

  const { data: policy, isLoading, isError } = useQuery({
    queryKey: ['policy', policyId],
    queryFn: () => getPolicy(policyId),
    enabled: !!policyId,
  })

  const hintDatasourceId = policy?.assignments?.[0]?.data_source_id ?? ''
  const catalogHints = useCatalogHints(hintDatasourceId)

  const isRowFilter = policy?.policy_type === 'row_filter'
  const { data: anchorCoverage } = useQuery({
    queryKey: ['policy-anchor-coverage', policyId, policy?.version ?? 0],
    queryFn: () => getPolicyAnchorCoverage(policyId),
    enabled: !!policy && isRowFilter,
  })
  const brokenCoverageCount = useMemo(
    () => countBrokenCoverageEntries(anchorCoverage),
    [anchorCoverage],
  )

  const sections = policy ? sectionsFor(policy, brokenCoverageCount) : []
  const validIds = sections.map((s) => s.id)
  const [activeSection, selectSection] = useSectionParam<SectionId>(
    validIds.length ? validIds : ['details'],
    'details',
  )

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
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      toast.success('Saved')
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
    <div className="p-6">
      <PageHeader
        breadcrumb={[
          { label: 'Policies', href: '/policies' },
          { label: policy.name },
        ]}
        title={policy.name}
        status={<StatusDot active={policy.is_enabled} activeLabel="Enabled" inactiveLabel="Disabled" />}
        metadata={[
          <StatusChip key="type" label={policy.policy_type} tone="blue" />,
          <span key="version">Version {policy.version}</span>,
          <CopyableId key="id" id={policyId} short />,
        ]}
      />

      <div className="flex items-start gap-6">
        <SecondaryNav
          ariaLabel="Policy sections"
          sections={sections}
          active={activeSection}
          onSelect={selectSection}
        />

        <div className="flex-1 min-w-0">
          {brokenCoverageCount > 0 && activeSection !== 'coverage' && (
            <div
              className={
                SECTION_WIDTH_CLASSES[
                  sections.find((s) => s.id === activeSection)?.width ?? 'wide'
                ]
              }
            >
              <div
                data-testid="anchor-coverage-banner"
                className="mb-4 flex items-center justify-between gap-3 bg-white border border-red-300 rounded-lg px-4 py-2.5"
              >
                <div className="text-sm text-red-800">
                  <span className="font-semibold">
                    This row filter will silently deny on {brokenCoverageCount}{' '}
                    {brokenCoverageCount === 1 ? 'table' : 'tables'}.
                  </span>{' '}
                  <span className="text-red-700/80">
                    Add a column anchor to fix it.
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => selectSection('coverage')}
                  className="shrink-0 text-xs font-medium px-3 py-1.5 rounded border border-red-300 text-red-700 bg-white hover:bg-red-50"
                >
                  Review
                </button>
              </div>
            </div>
          )}
          {sections.map((s) => (
            <SectionPane key={s.id} active={activeSection === s.id} width={s.width}>
              {s.id === 'details' && (
                <>
                  <div className="bg-white rounded-xl border border-gray-200 p-6">
                    <PolicyForm
                      initial={policy}
                      onSubmit={handleSubmit}
                      submitLabel="Save changes"
                      isSubmitting={isSubmitting}
                      error={error}
                      catalogHints={catalogHints}
                    />
                  </div>
                  <PolicyDangerZone onDelete={() => setShowDelete(true)} />
                </>
              )}

              {s.id === 'assignments' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <PolicyAssignmentEditPanel
                    policyId={policyId}
                    assignments={policy.assignments ?? []}
                    onAssignmentChange={() => {
                      queryClient.invalidateQueries({ queryKey: ['policy', policyId] })
                      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
                    }}
                  />
                </div>
              )}

              {s.id === 'coverage' && (
                <PolicyAnchorCoveragePanel data={anchorCoverage} />
              )}

              {s.id === 'code' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <PolicyCodeView policy={policy} assignments={policy.assignments ?? []} />
                </div>
              )}

              {s.id === 'activity' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
                  <AuditTimeline resourceType="policy" resourceId={policyId} />
                </div>
              )}
            </SectionPane>
          ))}
        </div>
      </div>

      {showDelete && (
        <DeletePolicyModal policy={policy} onClose={() => setShowDelete(false)} />
      )}
    </div>
  )
}

function PolicyDangerZone({ onDelete }: { onDelete: () => void }) {
  return (
    <DangerZone>
      <DangerRow
        title="Delete policy"
        body={
          <>
            Permanently removes this policy and all assignments. Users and roles
            previously assigned to this policy will lose its effect. Not recoverable.
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

function DeletePolicyModal({
  policy,
  onClose,
}: {
  policy: PolicyResponse
  onClose: () => void
}) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [err, setErr] = useState<string | null>(null)

  const disableMutation = useMutation({
    mutationFn: () =>
      updatePolicy(policy.id, {
        is_enabled: false,
        version: policy.version,
      }),
    onSuccess: () => {
      toast.success(`${policy.name} disabled`)
      queryClient.invalidateQueries({ queryKey: ['policies'] })
      queryClient.invalidateQueries({ queryKey: ['policy', policy.id] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      onClose()
    },
    onError: (e: unknown) => {
      const msg =
        (e as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to disable'
      setErr(msg)
    },
  })

  const deleteMutation = useMutation({
    mutationFn: () => deletePolicy(policy.id),
    onSuccess: () => {
      toast.success(`Deleted ${policy.name}`)
      queryClient.invalidateQueries({ queryKey: ['policies'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      navigate('/policies')
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
      resourceName={policy.name}
      consequences={
        <>
          <li>The policy itself (row filter / column mask / deny rules)</li>
          <li>All assignments to users, roles, and datasources</li>
        </>
      }
      softDelete={
        policy.is_enabled
          ? {
              label: 'Disable instead',
              pendingLabel: 'Disabling…',
              explanation: (
                <>
                  <span className="font-medium">Consider disabling instead.</span>{' '}
                  Disabling keeps the policy and its assignments — it just stops
                  enforcing. You can re-enable it later.
                </>
              ),
              onConfirm: () => disableMutation.mutate(),
              pending: disableMutation.isPending,
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
