import { useState, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { listAttributeDefinitions } from '../api/attributeDefinitions'
import type { AttributeDefinition } from '../types/attributeDefinition'
import type { AttributeValue } from '../types/user'

interface Props {
  /** Current attribute values from the user model. */
  attributes: Record<string, AttributeValue>
  /** Called with the updated full attribute map when user edits. */
  onChange: (attributes: Record<string, AttributeValue>) => void
}

export function UserAttributeEditor({ attributes, onChange }: Props) {
  const { data: defsData, isLoading } = useQuery({
    queryKey: ['attribute-definitions', 'user'],
    queryFn: () => listAttributeDefinitions({ entity_type: 'user', page_size: 200 }),
  })

  const definitions = defsData?.data ?? []

  // Local state mirrors props; syncs upward via onChange
  const [values, setValues] = useState<Record<string, AttributeValue>>(attributes)

  useEffect(() => {
    setValues(attributes)
  }, [attributes])

  function handleChange(key: string, value: AttributeValue) {
    const next = { ...values }
    const isEmpty = Array.isArray(value) ? value.length === 0 : value === '' || value === undefined
    if (isEmpty) {
      delete next[key]
    } else {
      next[key] = value
    }
    setValues(next)
    onChange(next)
  }

  function handleRemove(key: string) {
    const next = { ...values }
    delete next[key]
    setValues(next)
    onChange(next)
  }

  function handleAdd(key: string) {
    const def = definitions.find((d) => d.key === key)
    if (!def) return
    // Set to default or appropriate empty initial value
    const initial: AttributeValue =
      def.value_type === 'list'
        ? []
        : def.value_type === 'boolean'
          ? 'false'
          : def.default_value ?? ''
    const next = { ...values, [key]: initial }
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

  // Only show definitions the user has a value for
  const assignedDefs = definitions.filter((d) => d.key in values)
  // Available definitions for the "Add" dropdown
  const availableDefs = definitions.filter((d) => !(d.key in values))
  // Orphaned keys (set but no definition)
  const orphanedKeys = Object.keys(values).filter((k) => !definitions.some((d) => d.key === k))

  return (
    <div className="space-y-3 max-w-lg">
      {assignedDefs.map((def) => (
        <AttributeField
          key={def.id}
          definition={def}
          value={values[def.key]}
          onChange={(v) => handleChange(def.key, v)}
          onRemove={() => handleRemove(def.key)}
        />
      ))}

      {/* Orphaned attributes (set but no definition) */}
      {orphanedKeys.map((k) => (
        <div key={k} className="flex items-center gap-3">
          <label className="block text-sm font-medium text-gray-400 w-40 truncate">
            {k} <span className="text-xs">(no definition)</span>
          </label>
          <input
            type="text"
            value={typeof values[k] === 'string' ? values[k] : JSON.stringify(values[k])}
            disabled
            className="flex-1 border border-gray-200 bg-gray-50 rounded-lg px-3 py-2 text-sm text-gray-400"
          />
          <button
            type="button"
            onClick={() => handleRemove(k)}
            className="text-xs text-red-500 hover:text-red-700"
          >
            Remove
          </button>
        </div>
      ))}

      {assignedDefs.length === 0 && orphanedKeys.length === 0 && (
        <p className="text-sm text-gray-400">No attributes assigned.</p>
      )}

      {/* Add attribute */}
      <AddAttributeDropdown definitions={availableDefs} onAdd={handleAdd} />
    </div>
  )
}

function AddAttributeDropdown({
  definitions,
  onAdd,
}: {
  definitions: AttributeDefinition[]
  onAdd: (key: string) => void
}) {
  const [open, setOpen] = useState(false)

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="text-sm text-blue-600 hover:text-blue-800 font-medium"
      >
        + Add attribute
      </button>
    )
  }

  return (
    <div className="space-y-2">
      {definitions.length > 0 ? (
        <div className="flex items-center gap-2">
          <select
            defaultValue=""
            onChange={(e) => {
              if (e.target.value) {
                onAdd(e.target.value)
                setOpen(false)
              }
            }}
            className="border border-gray-300 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            autoFocus
          >
            <option value="" disabled>
              Select attribute...
            </option>
            {definitions.map((d) => (
              <option key={d.id} value={d.key}>
                {d.display_name} ({d.value_type})
                {d.default_value != null
                  ? ` — default: ${d.default_value}`
                  : ' — no default (null)'}
              </option>
            ))}
          </select>
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="text-sm text-gray-500 hover:text-gray-700"
          >
            Cancel
          </button>
        </div>
      ) : (
        <div className="flex items-center gap-2">
          <span className="text-sm text-gray-400">All defined attributes are assigned.</span>
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="text-sm text-gray-500 hover:text-gray-700"
          >
            Cancel
          </button>
        </div>
      )}
      <a
        href="/attributes/create"
        className="text-sm text-gray-500 hover:text-blue-600 hover:underline"
      >
        Or define a new attribute
      </a>
    </div>
  )
}

function AttributeField({
  definition,
  value,
  onChange,
  onRemove,
}: {
  definition: AttributeDefinition
  value: AttributeValue | undefined
  onChange: (value: AttributeValue) => void
  onRemove: () => void
}) {
  if (definition.value_type === 'list') {
    return (
      <ListAttributeField
        definition={definition}
        value={value}
        onChange={onChange}
        onRemove={onRemove}
      />
    )
  }

  const scalarValue = typeof value === 'string' ? value : ''
  const hasEnum = definition.allowed_values && definition.allowed_values.length > 0

  return (
    <div className="flex items-start gap-3">
      <label className="block text-sm font-medium text-gray-700 w-40 truncate pt-2" title={definition.key}>
        {definition.display_name}
        <span className="block text-xs font-normal text-gray-400 font-mono mt-0.5">
          {'{'}user.{definition.key}{'}'}
        </span>
        {definition.description && (
          <span className="block text-xs font-normal text-gray-400 truncate mt-0.5" title={definition.description}>
            {definition.description}
          </span>
        )}
      </label>

      {definition.value_type === 'boolean' ? (
        <select
          value={scalarValue}
          onChange={(e) => onChange(e.target.value)}
          className="flex-1 border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      ) : hasEnum ? (
        <select
          value={scalarValue}
          onChange={(e) => onChange(e.target.value)}
          className="flex-1 border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          {definition.allowed_values!.map((v) => (
            <option key={v} value={v}>
              {v}
            </option>
          ))}
        </select>
      ) : (
        <input
          type={definition.value_type === 'integer' ? 'number' : 'text'}
          value={scalarValue}
          onChange={(e) => onChange(e.target.value)}
          placeholder={definition.default_value ?? `Enter ${definition.value_type}`}
          className="flex-1 border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
      )}

      <button
        type="button"
        onClick={onRemove}
        className="text-gray-400 hover:text-red-500 text-sm pt-2"
        title="Remove attribute"
      >
        &times;
      </button>
    </div>
  )
}

function ListAttributeField({
  definition,
  value,
  onChange,
  onRemove,
}: {
  definition: AttributeDefinition
  value: AttributeValue | undefined
  onChange: (value: AttributeValue) => void
  onRemove: () => void
}) {
  const items: string[] = Array.isArray(value) ? value : []
  const hasEnum = definition.allowed_values && definition.allowed_values.length > 0
  const [inputValue, setInputValue] = useState('')

  function addItem(item: string) {
    const trimmed = item.trim()
    if (trimmed && !items.includes(trimmed)) {
      onChange([...items, trimmed])
    }
    setInputValue('')
  }

  function removeItem(item: string) {
    onChange(items.filter((i) => i !== item))
  }

  function toggleItem(item: string) {
    if (items.includes(item)) {
      removeItem(item)
    } else {
      onChange([...items, item])
    }
  }

  return (
    <div className="flex gap-3">
      <label className="block text-sm font-medium text-gray-700 w-40 truncate pt-2" title={definition.key}>
        {definition.display_name}
        <span className="block text-xs font-normal text-gray-400 font-mono mt-0.5">
          {'{'}user.{definition.key}{'}'}
        </span>
        {definition.description && (
          <span className="block text-xs font-normal text-gray-400 truncate mt-0.5" title={definition.description}>
            {definition.description}
          </span>
        )}
      </label>

      <div className="flex-1">
        {hasEnum ? (
          <div className="space-y-1">
            {definition.allowed_values!.map((av) => (
              <label key={av} className="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  checked={items.includes(av)}
                  onChange={() => toggleItem(av)}
                  className="rounded border-gray-300 text-blue-600 focus:ring-blue-500"
                />
                {av}
              </label>
            ))}
          </div>
        ) : (
          <div className="border border-gray-300 rounded-lg px-3 py-2 focus-within:ring-2 focus-within:ring-blue-500">
            <div className="flex flex-wrap gap-1.5">
              {items.map((item) => (
                <span
                  key={item}
                  className="inline-flex items-center gap-1 bg-blue-50 text-blue-700 text-sm px-2 py-0.5 rounded-md"
                >
                  {item}
                  <button
                    type="button"
                    onClick={() => removeItem(item)}
                    className="text-blue-400 hover:text-blue-600"
                    aria-label={`Remove ${item}`}
                  >
                    &times;
                  </button>
                </span>
              ))}
              <input
                type="text"
                value={inputValue}
                onChange={(e) => setInputValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    addItem(inputValue)
                  }
                }}
                placeholder={items.length === 0 ? 'Type and press Enter...' : ''}
                className="flex-1 min-w-[120px] text-sm outline-none bg-transparent py-0.5"
              />
            </div>
          </div>
        )}
      </div>

      <button
        type="button"
        onClick={onRemove}
        className="text-gray-400 hover:text-red-500 text-sm pt-2"
        title="Remove attribute"
      >
        &times;
      </button>
    </div>
  )
}
