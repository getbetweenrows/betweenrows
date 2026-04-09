import { type FormEvent, useState, useEffect } from 'react'
import {
  SUPPORTED_ENTITY_TYPES,
  type ValueType,
  type EntityType,
  type AttributeDefinition,
} from '../types/attributeDefinition'

export interface AttributeDefinitionFormValues {
  key: string
  entity_type: EntityType
  display_name: string
  value_type: ValueType
  default_value: string
  allowed_values: string[] | undefined
  description: string
}

interface Props {
  mode: 'create' | 'edit'
  initial?: AttributeDefinition
  onSubmit: (values: AttributeDefinitionFormValues) => Promise<void>
  onCancel: () => void
  submitLabel: string
  isSubmitting: boolean
  error?: string | null
}

const VALUE_TYPES: ValueType[] = ['string', 'integer', 'boolean', 'list']
const ENTITY_TYPES = SUPPORTED_ENTITY_TYPES

const inputCls =
  'w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500'

export function AttributeDefinitionForm({
  mode,
  initial,
  onSubmit,
  onCancel,
  submitLabel,
  isSubmitting,
  error,
}: Props) {
  const [key, setKey] = useState('')
  const [entityType, setEntityType] = useState<EntityType>('user')
  const [displayName, setDisplayName] = useState('')
  const [valueType, setValueType] = useState<ValueType>('string')
  const [defaultValue, setDefaultValue] = useState('')
  const [allowedValuesText, setAllowedValuesText] = useState('')
  const [description, setDescription] = useState('')

  useEffect(() => {
    if (initial) {
      setKey(initial.key)
      setEntityType(initial.entity_type)
      setDisplayName(initial.display_name)
      setValueType(initial.value_type)
      setDefaultValue(initial.default_value ?? '')
      setAllowedValuesText(initial.allowed_values?.join(', ') ?? '')
      setDescription(initial.description ?? '')
    }
  }, [initial])

  async function handleSubmit(e: FormEvent) {
    e.preventDefault()
    const allowedValues = allowedValuesText.trim()
      ? allowedValuesText
          .split(',')
          .map((v) => v.trim())
          .filter(Boolean)
      : undefined

    await onSubmit({
      key,
      entity_type: entityType,
      display_name: displayName,
      value_type: valueType,
      default_value: defaultValue,
      allowed_values: allowedValues,
      description,
    })
  }

  return (
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
              className={`${inputCls} font-mono`}
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
              className={inputCls}
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
          className={inputCls}
        />
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Value type <span className="text-red-500">*</span>
        </label>
        <select
          value={valueType}
          onChange={(e) => setValueType(e.target.value as ValueType)}
          className={inputCls}
        >
          {VALUE_TYPES.map((t) => (
            <option key={t} value={t}>
              {t === 'list' ? 'list (multiple strings)' : t}
            </option>
          ))}
        </select>
        {valueType === 'list' && (
          <p className="text-xs text-gray-400 mt-1">
            List attributes store multiple string values. Use with <code className="bg-gray-100 px-1 rounded">IN {'{'}user.key{'}'}</code> in filter expressions.
          </p>
        )}
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
          className={inputCls}
        />
        <p className="text-xs text-gray-400 mt-1">
          {valueType === 'list'
            ? 'Constrains which strings can appear as elements in the list.'
            : 'Leave empty to allow any value of the selected type.'}
        </p>
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Default value
        </label>
        {valueType === 'boolean' ? (
          <select
            value={defaultValue}
            onChange={(e) => setDefaultValue(e.target.value)}
            className={inputCls}
          >
            <option value="">No default (null)</option>
            <option value="true">true</option>
            <option value="false">false</option>
          </select>
        ) : (
          <div className="flex items-center gap-2">
            <input
              type={valueType === 'integer' ? 'number' : 'text'}
              value={defaultValue}
              onChange={(e) => setDefaultValue(e.target.value)}
              placeholder={
                valueType === 'list'
                  ? 'No default (null) — or JSON array, e.g., ["default"]'
                  : 'No default (null)'
              }
              className={`${inputCls} flex-1`}
            />
            {defaultValue && (
              <button
                type="button"
                onClick={() => setDefaultValue('')}
                className="text-gray-400 hover:text-red-500 text-sm px-1"
                title="Clear to null"
              >
                &times;
              </button>
            )}
          </div>
        )}
        <p className="text-xs text-gray-400 mt-1">
          {defaultValue
            ? `Users without this attribute will be treated as having the value "${defaultValue}" when policies are evaluated. This value is applied by the proxy at query time — it is not stored on the user.`
            : 'When null: users without this attribute will have NULL substituted in policy expressions. In SQL, comparisons with NULL (e.g., tenant = NULL) evaluate to NULL, which is treated as false — so equality filters return zero rows. This is applied by the proxy at query time, not stored on the user.'}
        </p>
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Description <span className="text-gray-400 font-normal">(optional)</span>
        </label>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Optional description"
          rows={2}
          className={inputCls}
        />
      </div>

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg px-4 py-3">
          {error}
        </div>
      )}

      <div className="flex gap-3 pt-2">
        <button
          type="submit"
          disabled={isSubmitting}
          className="bg-blue-600 hover:bg-blue-700 disabled:opacity-60 text-white font-medium rounded-lg px-5 py-2 text-sm transition-colors"
        >
          {isSubmitting ? 'Saving...' : submitLabel}
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="text-gray-600 hover:text-gray-900 font-medium text-sm px-3 py-2 transition-colors"
        >
          Cancel
        </button>
      </div>
    </form>
  )
}
