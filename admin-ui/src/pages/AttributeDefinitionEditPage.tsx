import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  getAttributeDefinition,
  createAttributeDefinition,
  updateAttributeDefinition,
} from '../api/attributeDefinitions'
import { AuditTimeline } from '../components/AuditTimeline'
import {
  AttributeDefinitionForm,
  type AttributeDefinitionFormValues,
} from '../components/AttributeDefinitionForm'
import type { CreateAttributeDefinitionPayload } from '../types/attributeDefinition'

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
      <div className="mb-6">
        <button
          onClick={() => navigate('/attributes')}
          className="text-sm text-gray-500 hover:text-gray-700 mb-2"
        >
          &larr; Back to Attribute Definitions
        </button>
        <h1 className="text-xl font-bold text-gray-900">New Attribute Definition</h1>
      </div>

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

export function AttributeDefinitionEditPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const { data: existing, isLoading } = useQuery({
    queryKey: ['attribute-definitions', id],
    queryFn: () => getAttributeDefinition(id!),
    enabled: !!id,
  })

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
      navigate('/attributes')
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
    <div className="p-6 max-w-2xl">
      <div className="mb-6">
        <button
          onClick={() => navigate('/attributes')}
          className="text-sm text-gray-500 hover:text-gray-700 mb-2"
        >
          &larr; Back to Attribute Definitions
        </button>
        <h1 className="text-xl font-bold text-gray-900">Edit Attribute Definition</h1>
        <p className="text-sm text-gray-500 mt-1">
          <span className="font-mono">{existing.key}</span> ({existing.entity_type})
        </p>
      </div>

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

      {id && (
        <div className="bg-white rounded-xl border border-gray-200 p-6 mt-6">
          <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
          <AuditTimeline resourceType="attribute_definition" resourceId={id} />
        </div>
      )}
    </div>
  )
}
