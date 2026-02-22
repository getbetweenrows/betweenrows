import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { getDataSourceTypes, testDataSource } from '../api/datasources'
import type { DataSourceType, FieldDef } from '../types/datasource'

interface DataSourceFormProps {
  /** If provided, the form is in edit mode for this id. */
  datasourceId?: string
  /** Pre-filled values (non-secret config from the API response). */
  initialValues?: {
    name?: string
    ds_type?: string
    config?: Record<string, unknown>
    is_active?: boolean
  }
  onSubmit: (values: {
    name: string
    ds_type: string
    config: Record<string, unknown>
    is_active: boolean
  }) => Promise<void>
  submitLabel?: string
  isSubmitting?: boolean
}

export function DataSourceForm({
  datasourceId,
  initialValues,
  onSubmit,
  submitLabel = 'Save',
  isSubmitting = false,
}: DataSourceFormProps) {
  const isEdit = datasourceId !== undefined

  const { data: types = [], isLoading: typesLoading } = useQuery({
    queryKey: ['datasource-types'],
    queryFn: getDataSourceTypes,
    staleTime: Infinity,
  })

  const [name, setName] = useState(initialValues?.name ?? '')
  const [dsType, setDsType] = useState(initialValues?.ds_type ?? '')
  const [isActive, setIsActive] = useState(initialValues?.is_active ?? true)
  const [fieldValues, setFieldValues] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {}
    if (initialValues?.config) {
      for (const [k, v] of Object.entries(initialValues.config)) {
        init[k] = String(v)
      }
    }
    return init
  })
  const [testResult, setTestResult] = useState<{
    success: boolean
    message?: string
  } | null>(null)
  const [testLoading, setTestLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const selectedType: DataSourceType | undefined = types.find((t) => t.ds_type === dsType)

  function handleTypeChange(newType: string) {
    setDsType(newType)
    setFieldValues({})
    setTestResult(null)
    // Apply defaults for the new type
    const typeDef = types.find((t) => t.ds_type === newType)
    if (typeDef) {
      const defaults: Record<string, string> = {}
      for (const field of typeDef.fields) {
        if (field.default_value !== undefined && !field.is_secret) {
          defaults[field.key] = field.default_value
        }
      }
      setFieldValues(defaults)
    }
  }

  function setField(key: string, value: string) {
    setFieldValues((prev) => ({ ...prev, [key]: value }))
  }

  async function handleTest() {
    if (!datasourceId) return
    setTestLoading(true)
    setTestResult(null)
    try {
      const result = await testDataSource(datasourceId)
      setTestResult(result)
    } catch {
      setTestResult({ success: false, message: 'Request failed' })
    } finally {
      setTestLoading(false)
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setTestResult(null)

    if (!name.trim()) {
      setError('Name is required')
      return
    }
    if (!dsType) {
      setError('Please select a data source type')
      return
    }

    // Build flat config object from field values
    const config: Record<string, unknown> = {}
    if (selectedType) {
      for (const field of selectedType.fields) {
        const val = fieldValues[field.key] ?? ''
        if (val !== '' || !isEdit) {
          // In edit mode, skip empty non-secret fields that aren't being updated
          config[field.key] = field.field_type === 'number' ? (val !== '' ? Number(val) : undefined) : val
        }
      }
    }

    try {
      await onSubmit({ name: name.trim(), ds_type: dsType, config, is_active: isActive })
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Something went wrong'
      setError(msg)
    }
  }

  if (typesLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading type definitions…</div>
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-5">
      {error && (
        <div className="bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg px-4 py-3">
          {error}
        </div>
      )}

      {/* Name */}
      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Name <span className="text-red-500">*</span>
        </label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. production-db"
          className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          required
        />
        <p className="text-xs text-gray-500 mt-1">
          This is the database name clients use in their connection string.
        </p>
      </div>

      {/* Type selector */}
      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Type <span className="text-red-500">*</span>
        </label>
        <select
          value={dsType}
          onChange={(e) => handleTypeChange(e.target.value)}
          disabled={isEdit}
          className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:bg-gray-50 disabled:text-gray-500"
          required
        >
          <option value="">Select a type…</option>
          {types.map((t) => (
            <option key={t.ds_type} value={t.ds_type}>
              {t.label}
            </option>
          ))}
        </select>
      </div>

      {/* Dynamic fields */}
      {selectedType && (
        <fieldset className="space-y-4 border border-gray-200 rounded-lg p-4">
          <legend className="text-sm font-medium text-gray-700 px-1">Connection</legend>
          {selectedType.fields.map((field) => (
            <DynamicField
              key={field.key}
              field={field}
              value={fieldValues[field.key] ?? ''}
              onChange={(v) => setField(field.key, v)}
              isEdit={isEdit}
            />
          ))}
        </fieldset>
      )}

      {/* is_active (edit mode only) */}
      {isEdit && (
        <div className="flex items-center gap-2">
          <input
            id="is_active"
            type="checkbox"
            checked={isActive}
            onChange={(e) => setIsActive(e.target.checked)}
            className="rounded border-gray-300 text-blue-600 focus:ring-blue-500"
          />
          <label htmlFor="is_active" className="text-sm text-gray-700">
            Active
          </label>
        </div>
      )}

      {/* Actions */}
      <div className="flex items-center gap-3 pt-1">
        <button
          type="submit"
          disabled={isSubmitting}
          className="bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg px-4 py-2 transition-colors disabled:opacity-50"
        >
          {isSubmitting ? 'Saving…' : submitLabel}
        </button>

        {isEdit && datasourceId && (
          <button
            type="button"
            onClick={handleTest}
            disabled={testLoading}
            className="border border-gray-300 text-gray-700 hover:bg-gray-50 text-sm font-medium rounded-lg px-4 py-2 transition-colors disabled:opacity-50"
          >
            {testLoading ? 'Testing…' : 'Test Connection'}
          </button>
        )}

        {testResult && (
          <span
            className={`text-sm font-medium ${
              testResult.success ? 'text-green-600' : 'text-red-600'
            }`}
          >
            {testResult.success ? '✓ Connected' : `✗ ${testResult.message ?? 'Failed'}`}
          </span>
        )}
      </div>
    </form>
  )
}

function DynamicField({
  field,
  value,
  onChange,
  isEdit,
}: {
  field: FieldDef
  value: string
  onChange: (v: string) => void
  isEdit: boolean
}) {
  const placeholder = field.is_secret && isEdit ? 'Leave blank to keep current' : (field.default_value ?? '')
  const inputType = field.is_secret ? 'password' : field.field_type === 'number' ? 'number' : 'text'

  return (
    <div>
      <label className="block text-sm font-medium text-gray-700 mb-1">
        {field.label}
        {field.required && !field.is_secret && <span className="text-red-500 ml-0.5">*</span>}
      </label>

      {field.field_type === 'select' ? (
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          required={field.required}
          className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          <option value="">Select…</option>
          {field.options?.map((opt) => (
            <option key={opt} value={opt}>
              {opt}
            </option>
          ))}
        </select>
      ) : field.field_type === 'textarea' ? (
        <textarea
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          required={field.required && !(field.is_secret && isEdit)}
          rows={3}
          className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
        />
      ) : (
        <input
          type={inputType}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          required={field.required && !(field.is_secret && isEdit)}
          autoComplete={field.is_secret ? 'new-password' : undefined}
          className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
      )}
    </div>
  )
}
