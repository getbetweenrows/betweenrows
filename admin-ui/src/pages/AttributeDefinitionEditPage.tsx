import { useState, useEffect } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  getAttributeDefinition,
  createAttributeDefinition,
  updateAttributeDefinition,
} from '../api/attributeDefinitions'
import { AuditTimeline } from '../components/AuditTimeline'
import type {
  ValueType,
  EntityType,
  CreateAttributeDefinitionPayload,
} from '../types/attributeDefinition'

const VALUE_TYPES: ValueType[] = ['string', 'integer', 'boolean']
const ENTITY_TYPES: EntityType[] = ['user', 'table', 'column']

export function AttributeDefinitionCreatePage() {
  return <AttributeDefinitionForm mode="create" />
}

export function AttributeDefinitionEditPage() {
  return <AttributeDefinitionForm mode="edit" />
}

function AttributeDefinitionForm({ mode }: { mode: 'create' | 'edit' }) {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const { data: existing, isLoading } = useQuery({
    queryKey: ['attribute-definitions', id],
    queryFn: () => getAttributeDefinition(id!),
    enabled: mode === 'edit' && !!id,
  })

  const [key, setKey] = useState('')
  const [entityType, setEntityType] = useState<EntityType>('user')
  const [displayName, setDisplayName] = useState('')
  const [valueType, setValueType] = useState<ValueType>('string')
  const [defaultValue, setDefaultValue] = useState('')
  const [allowedValuesText, setAllowedValuesText] = useState('')
  const [description, setDescription] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (existing) {
      setKey(existing.key)
      setEntityType(existing.entity_type)
      setDisplayName(existing.display_name)
      setValueType(existing.value_type)
      setDefaultValue(existing.default_value ?? '')
      setAllowedValuesText(existing.allowed_values?.join(', ') ?? '')
      setDescription(existing.description ?? '')
    }
  }, [existing])

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setSaving(true)

    const allowedValues = allowedValuesText.trim()
      ? allowedValuesText.split(',').map((v) => v.trim()).filter(Boolean)
      : undefined

    try {
      if (mode === 'create') {
        const payload: CreateAttributeDefinitionPayload = {
          key,
          entity_type: entityType,
          display_name: displayName,
          value_type: valueType,
          default_value: defaultValue || undefined,
          allowed_values: allowedValues,
          description: description || undefined,
        }
        await createAttributeDefinition(payload)
      } else {
        await updateAttributeDefinition(id!, {
          display_name: displayName,
          value_type: valueType,
          default_value: defaultValue || null,
          allowed_values: allowedValues ?? null,
          description: description || null,
        })
      }
      await queryClient.invalidateQueries({ queryKey: ['attribute-definitions'] })
      navigate('/attributes')
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        `Failed to ${mode} attribute definition`
      setError(msg)
    } finally {
      setSaving(false)
    }
  }

  if (mode === 'edit' && isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading...</div>
  }

  if (mode === 'edit' && !existing) {
    return <div className="p-6 text-sm text-red-500">Attribute definition not found.</div>
  }

  return (
    <div className="p-6 space-y-10">
      <section>
        <h1 className="text-xl font-bold text-gray-900 mb-1">
          {mode === 'create' ? 'New attribute definition' : 'Edit attribute definition'}
        </h1>
        {mode === 'edit' && (
          <p className="text-sm text-gray-500 mb-6">
            <span className="font-mono">{existing?.key}</span> ({existing?.entity_type})
          </p>
        )}

        <form onSubmit={handleSubmit} className="space-y-5 max-w-lg">
          {mode === 'create' && (
            <>
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  Key <span className="text-red-500">*</span>
                </label>
                <input
                  type="text"
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
                  placeholder="e.g., region, clearance_level"
                  required
                  pattern="[a-zA-Z][a-zA-Z0-9_]*"
                  maxLength={64}
                  className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
                />
                <p className="text-xs text-gray-400 mt-1">
                  Letters, digits, underscores. Used as {'{'}user.key{'}'} in expressions.
                </p>
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  Entity type <span className="text-red-500">*</span>
                </label>
                <select
                  value={entityType}
                  onChange={(e) => setEntityType(e.target.value as EntityType)}
                  className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                >
                  {ENTITY_TYPES.map((t) => (
                    <option key={t} value={t}>
                      {t}
                    </option>
                  ))}
                </select>
              </div>
            </>
          )}

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Display name <span className="text-red-500">*</span>
            </label>
            <input
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="e.g., Region, Clearance Level"
              required
              className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Value type <span className="text-red-500">*</span>
            </label>
            <select
              value={valueType}
              onChange={(e) => setValueType(e.target.value as ValueType)}
              className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            >
              {VALUE_TYPES.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Allowed values
            </label>
            <input
              type="text"
              value={allowedValuesText}
              onChange={(e) => setAllowedValuesText(e.target.value)}
              placeholder="Comma-separated, e.g., us-east, eu-west, ap-south"
              className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
            <p className="text-xs text-gray-400 mt-1">
              Leave empty to allow any value of the selected type.
            </p>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Default value
            </label>
            <input
              type="text"
              value={defaultValue}
              onChange={(e) => setDefaultValue(e.target.value)}
              placeholder="Optional"
              className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">Description</label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Optional description"
              rows={2}
              className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>

          {error && <p className="text-sm text-red-600">{error}</p>}

          <div className="flex gap-3">
            <button
              type="submit"
              disabled={saving}
              className="bg-blue-600 hover:bg-blue-700 disabled:opacity-60 text-white font-medium rounded-lg px-5 py-2 text-sm transition-colors"
            >
              {saving ? 'Saving...' : mode === 'create' ? 'Create' : 'Save'}
            </button>
            <button
              type="button"
              onClick={() => navigate('/attributes')}
              className="border border-gray-300 rounded-lg px-4 py-2 text-sm hover:bg-gray-50 transition-colors"
            >
              Cancel
            </button>
          </div>
        </form>
      </section>

      {mode === 'edit' && id && (
        <section className="border-t border-gray-200 pt-8">
          <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
          <AuditTimeline resourceType="attribute_definition" resourceId={id} />
        </section>
      )}
    </div>
  )
}
