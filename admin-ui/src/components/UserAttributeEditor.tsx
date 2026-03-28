import { useState, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { listAttributeDefinitions } from '../api/attributeDefinitions'
import type { AttributeDefinition } from '../types/attributeDefinition'

interface Props {
  /** Current attribute values from the user model. */
  attributes: Record<string, string>
  /** Called with the updated full attribute map when user edits. */
  onChange: (attributes: Record<string, string>) => void
}

export function UserAttributeEditor({ attributes, onChange }: Props) {
  const { data: defsData, isLoading } = useQuery({
    queryKey: ['attribute-definitions', 'user'],
    queryFn: () => listAttributeDefinitions({ entity_type: 'user', page_size: 200 }),
  })

  const definitions = defsData?.data ?? []

  // Local state mirrors props; syncs upward via onChange
  const [values, setValues] = useState<Record<string, string>>(attributes)

  useEffect(() => {
    setValues(attributes)
  }, [attributes])

  function handleChange(key: string, value: string) {
    const next = { ...values }
    if (value === '' || value === undefined) {
      delete next[key]
    } else {
      next[key] = value
    }
    setValues(next)
    onChange(next)
  }

  if (isLoading) {
    return <p className="text-sm text-gray-400">Loading attribute definitions...</p>
  }

  if (definitions.length === 0) {
    return (
      <p className="text-sm text-gray-500">
        No attribute definitions yet.{' '}
        <a href="/attributes/create" className="text-blue-600 hover:underline">
          Create one
        </a>{' '}
        to start using user attributes.
      </p>
    )
  }

  return (
    <div className="space-y-3 max-w-lg">
      {definitions.map((def) => (
        <AttributeField
          key={def.id}
          definition={def}
          value={values[def.key] ?? ''}
          onChange={(v) => handleChange(def.key, v)}
        />
      ))}
      {/* Show any orphaned attributes (set but no definition) */}
      {Object.keys(values)
        .filter((k) => !definitions.some((d) => d.key === k))
        .map((k) => (
          <div key={k} className="flex items-center gap-3">
            <label className="block text-sm font-medium text-gray-400 w-40 truncate">
              {k} <span className="text-xs">(no definition)</span>
            </label>
            <input
              type="text"
              value={values[k]}
              disabled
              className="flex-1 border border-gray-200 bg-gray-50 rounded-lg px-3 py-2 text-sm text-gray-400"
            />
            <button
              type="button"
              onClick={() => handleChange(k, '')}
              className="text-xs text-red-500 hover:text-red-700"
            >
              Remove
            </button>
          </div>
        ))}
    </div>
  )
}

function AttributeField({
  definition,
  value,
  onChange,
}: {
  definition: AttributeDefinition
  value: string
  onChange: (value: string) => void
}) {
  const hasEnum = definition.allowed_values && definition.allowed_values.length > 0

  return (
    <div className="flex items-center gap-3">
      <label className="block text-sm font-medium text-gray-700 w-40 truncate" title={definition.key}>
        {definition.display_name}
        {definition.description && (
          <span className="block text-xs font-normal text-gray-400 truncate" title={definition.description}>
            {definition.description}
          </span>
        )}
      </label>

      {definition.value_type === 'boolean' ? (
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="flex-1 border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          <option value="">Not set</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      ) : hasEnum ? (
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="flex-1 border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          <option value="">Not set</option>
          {definition.allowed_values!.map((v) => (
            <option key={v} value={v}>
              {v}
            </option>
          ))}
        </select>
      ) : (
        <input
          type={definition.value_type === 'integer' ? 'number' : 'text'}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={definition.default_value ?? `Enter ${definition.value_type}`}
          className="flex-1 border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
      )}

      <span className="text-xs text-gray-400 font-mono w-16 text-right">
        {'{'}user.{definition.key}{'}'}
      </span>
    </div>
  )
}
