import { type FormEvent, useState } from 'react'

export interface RoleFormValues {
  name: string
  description: string
}

interface RoleFormProps {
  initial?: { name?: string; description?: string | null }
  onSubmit: (values: RoleFormValues) => Promise<void>
  onCancel: () => void
  submitLabel: string
  isSubmitting: boolean
  error?: string | null
}

const inputCls =
  'w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500'

export function RoleForm({ initial, onSubmit, onCancel, submitLabel, isSubmitting, error }: RoleFormProps) {
  const [name, setName] = useState(initial?.name ?? '')
  const [nameError, setNameError] = useState<string | null>(null)
  const [description, setDescription] = useState(initial?.description ?? '')

  function validateName(value: string): string | null {
    const trimmed = value.trim()
    if (trimmed.length < 3) return 'Name must be at least 3 characters.'
    if (trimmed.length > 50) return 'Name must be at most 50 characters.'
    return null
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault()
    const nErr = validateName(name)
    setNameError(nErr)
    if (nErr) return
    await onSubmit({ name: name.trim(), description: description.trim() })
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-5 max-w-lg">
      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Name <span className="text-red-500 ml-1">*</span>
        </label>
        <input
          type="text"
          value={name}
          onChange={(e) => { setName(e.target.value); setNameError(null) }}
          onBlur={() => setNameError(validateName(name))}
          required
          placeholder="e.g. data-analysts"
          className={inputCls}
        />
        {nameError ? (
          <p className="text-xs text-red-600 mt-1">{nameError}</p>
        ) : (
          <p className="text-xs text-gray-400 mt-1">3-50 characters</p>
        )}
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Description <span className="text-gray-400 font-normal">(optional)</span>
        </label>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Brief description of this role"
          rows={3}
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
