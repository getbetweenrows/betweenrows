import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'
import {
  getAttributeDefinition,
  createAttributeDefinition,
  updateAttributeDefinition,
  deleteAttributeDefinition,
} from '../api/attributeDefinitions'
import { AuditTimeline } from '../components/AuditTimeline'
import {
  AttributeDefinitionForm,
  type AttributeDefinitionFormValues,
} from '../components/AttributeDefinitionForm'
import type {
  AttributeDefinition,
  CreateAttributeDefinitionPayload,
} from '../types/attributeDefinition'
import { PageHeader } from '../components/layout/PageHeader'
import { SecondaryNav, type SectionDef } from '../components/layout/SecondaryNav'
import { SectionPane, type SectionWidth } from '../components/layout/SectionPane'
import { DangerZone, DangerRow } from '../components/DangerZone'
import { ModalShell } from '../components/ModalShell'
import { useSectionParam } from '../hooks/useSectionParam'

export function AttributeDefinitionCreatePage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleSubmit(values: AttributeDefinitionFormValues) {
    setError(null)
    setIsSubmitting(true)
    try {
      const payload: CreateAttributeDefinitionPayload = {
        key: values.key,
        entity_type: values.entity_type,
        display_name: values.display_name,
        value_type: values.value_type,
        default_value: values.default_value || undefined,
        allowed_values: values.allowed_values,
        description: values.description || undefined,
      }
      await createAttributeDefinition(payload)
      await queryClient.invalidateQueries({ queryKey: ['attribute-definitions'] })
      navigate('/attributes')
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to create attribute definition'
      setError(msg)
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <div className="p-6 max-w-2xl">
      <PageHeader
        breadcrumb={[
          { label: 'Attribute Definitions', href: '/attributes' },
          { label: 'New attribute definition' },
        ]}
        title="New attribute definition"
      />

      <div className="bg-white rounded-xl border border-gray-200 p-6">
        <AttributeDefinitionForm
          mode="create"
          onSubmit={handleSubmit}
          onCancel={() => navigate('/attributes')}
          submitLabel="Create"
          isSubmitting={isSubmitting}
          error={error}
        />
      </div>
    </div>
  )
}

type SectionId = 'details' | 'activity'

interface AdSection extends SectionDef<SectionId> {
  width: SectionWidth
}

const SECTIONS: readonly AdSection[] = [
  { id: 'details', label: 'Details', width: 'narrow' },
  { id: 'activity', label: 'Activity', width: 'wide' },
]
const VALID_IDS: readonly SectionId[] = SECTIONS.map((s) => s.id)

export function AttributeDefinitionEditPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [activeSection, selectSection] = useSectionParam<SectionId>(VALID_IDS, 'details')

  const { data: existing, isLoading } = useQuery({
    queryKey: ['attribute-definitions', id],
    queryFn: () => getAttributeDefinition(id!),
    enabled: !!id,
  })

  const [showDelete, setShowDelete] = useState(false)

  async function handleSubmit(values: AttributeDefinitionFormValues) {
    setError(null)
    setIsSubmitting(true)
    try {
      await updateAttributeDefinition(id!, {
        display_name: values.display_name,
        value_type: values.value_type,
        default_value: values.default_value || null,
        allowed_values: values.allowed_values ?? null,
        description: values.description || null,
      })
      await queryClient.invalidateQueries({ queryKey: ['attribute-definitions'] })
      await queryClient.invalidateQueries({ queryKey: ['attribute-definitions', id] })
      toast.success('Saved')
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to update attribute definition'
      setError(msg)
    } finally {
      setIsSubmitting(false)
    }
  }

  if (isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading...</div>
  }

  if (!existing) {
    return <div className="p-6 text-sm text-red-500">Attribute definition not found.</div>
  }

  return (
    <div className="p-6">
      <PageHeader
        breadcrumb={[
          { label: 'Attribute Definitions', href: '/attributes' },
          { label: existing.key },
        ]}
        title={existing.key}
        metadata={[<span key="entity">{existing.entity_type}</span>]}
      />

      <div className="flex items-start gap-6">
        <SecondaryNav
          ariaLabel="Attribute definition sections"
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
                    <AttributeDefinitionForm
                      mode="edit"
                      initial={existing}
                      onSubmit={handleSubmit}
                      onCancel={() => navigate('/attributes')}
                      submitLabel="Save"
                      isSubmitting={isSubmitting}
                      error={error}
                    />
                  </div>

                  <DangerZone>
                    <DangerRow
                      title="Delete attribute definition"
                      body={
                        <>
                          Removes the definition. If any users still have a value stored under this key,
                          the delete will fail — you'll be offered a "force delete" option that removes
                          those user values as well. Not recoverable.
                        </>
                      }
                      action={
                        <button
                          type="button"
                          onClick={() => setShowDelete(true)}
                          className="bg-red-600 text-white text-sm font-medium rounded-lg px-4 py-1.5 hover:bg-red-700 transition-colors"
                        >
                          Delete…
                        </button>
                      }
                    />
                  </DangerZone>
                </>
              )}

              {s.id === 'activity' && id && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
                  <AuditTimeline resourceType="attribute_definition" resourceId={id} />
                </div>
              )}
            </SectionPane>
          ))}
        </div>
      </div>

      {showDelete && (
        <DeleteAttributeDefinitionModal
          def={existing}
          onClose={() => setShowDelete(false)}
        />
      )}
    </div>
  )
}

function DeleteAttributeDefinitionModal({
  def,
  onClose,
}: {
  def: AttributeDefinition
  onClose: () => void
}) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [confirmKey, setConfirmKey] = useState('')
  const [err, setErr] = useState<string | null>(null)
  /// Server returned "delete would orphan user values; pass force=true to override" —
  /// flip to true and re-enable the Delete button as a force-delete.
  const [canForce, setCanForce] = useState(false)

  const deleteMutation = useMutation({
    mutationFn: (force: boolean) => deleteAttributeDefinition(def.id, force),
    onSuccess: () => {
      toast.success(`Deleted ${def.key}`)
      queryClient.invalidateQueries({ queryKey: ['attribute-definitions'] })
      queryClient.invalidateQueries({ queryKey: ['users'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      navigate('/attributes')
    },
    onError: (e: unknown) => {
      const msg =
        (e as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to delete'
      setErr(msg)
      if (msg.includes('force=true')) setCanForce(true)
    },
  })

  const canDelete = confirmKey === def.key

  return (
    <ModalShell title={`Delete ${def.key}?`} onClose={onClose}>
      <p className="text-sm text-gray-700 mb-2">This will permanently remove:</p>
      <ul className="text-xs text-gray-600 list-disc list-inside space-y-0.5 mb-4">
        <li>The attribute definition ({def.entity_type}.{def.key})</li>
        <li>With force delete: all user values stored under this key</li>
      </ul>

      <label className="block text-xs text-gray-600 mb-1">
        Type <code className="font-mono font-semibold text-gray-900">{def.key}</code> to confirm:
      </label>
      <input
        autoFocus
        type="text"
        value={confirmKey}
        onChange={(e) => {
          setConfirmKey(e.target.value)
          setErr(null)
        }}
        className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-red-500 font-mono"
      />
      {err && <p className="text-xs text-red-600 mt-2">{err}</p>}

      <div className="flex items-center justify-end gap-2 mt-5">
        <button
          type="button"
          onClick={onClose}
          className="text-sm text-gray-600 px-3 py-1.5 rounded-lg hover:bg-gray-50"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={() => deleteMutation.mutate(canForce)}
          disabled={!canDelete || deleteMutation.isPending}
          className="bg-red-600 text-white text-sm font-medium rounded-lg px-4 py-1.5 hover:bg-red-700 disabled:opacity-50"
        >
          {deleteMutation.isPending
            ? 'Deleting…'
            : canForce
              ? 'Force delete'
              : 'Delete'}
        </button>
      </div>
    </ModalShell>
  )
}
