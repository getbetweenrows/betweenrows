import { useState } from 'react'
import type { PolicyResponse, PolicyType, TargetEntry } from '../types/policy'
import { validatePolicyName } from '../utils/nameValidation'

const POLICY_TYPES: { value: PolicyType; label: string }[] = [
  { value: 'row_filter', label: 'Row Filter' },
  { value: 'column_mask', label: 'Column Mask' },
  { value: 'column_allow', label: 'Column Allow' },
  { value: 'column_deny', label: 'Column Deny' },
  { value: 'table_deny', label: 'Table Deny' },
]

const DENY_TYPES: PolicyType[] = ['column_deny', 'table_deny']

function emptyTarget(): TargetEntry {
  return { schemas: ['*'], tables: ['*'], columns: ['*'] }
}

export interface PolicyFormValues {
  name: string
  description: string
  policy_type: PolicyType
  is_enabled: boolean
  targets: TargetEntry[]
  filter_expression: string
  mask_expression: string
}

interface PolicyFormProps {
  initial?: PolicyResponse
  onSubmit: (values: PolicyFormValues) => Promise<void>
  submitLabel: string
  isSubmitting: boolean
  error?: string | null
}

function targetsFromPolicy(policy: PolicyResponse): TargetEntry[] {
  if (policy.targets && policy.targets.length > 0) return policy.targets
  return [emptyTarget()]
}

function targetToString(arr: string[]): string {
  return arr.join(', ')
}

function stringToArray(s: string): string[] {
  return s
    .split(',')
    .map((v) => v.trim())
    .filter(Boolean)
}

export function PolicyForm({ initial, onSubmit, submitLabel, isSubmitting, error }: PolicyFormProps) {
  const [name, setName] = useState(initial?.name ?? '')
  const [nameError, setNameError] = useState<string | null>(null)
  const [description, setDescription] = useState(initial?.description ?? '')
  const [policyType, setPolicyType] = useState<PolicyType>(
    (initial?.policy_type as PolicyType) ?? 'row_filter',
  )
  const [isEnabled, setIsEnabled] = useState(initial?.is_enabled ?? true)
  const [targets, setTargets] = useState<TargetEntry[]>(
    initial ? targetsFromPolicy(initial) : [emptyTarget()],
  )
  const [filterExpression, setFilterExpression] = useState(
    initial?.definition?.filter_expression ?? '',
  )
  const [maskExpression, setMaskExpression] = useState(
    initial?.definition?.mask_expression ?? '',
  )

  const needsColumns = policyType === 'column_mask' || policyType === 'column_allow' || policyType === 'column_deny'
  const needsFilter = policyType === 'row_filter'
  const needsMask = policyType === 'column_mask'
  const isDeny = DENY_TYPES.includes(policyType)

  function addTarget() {
    setTargets((prev) => [...prev, emptyTarget()])
  }

  function removeTarget(idx: number) {
    setTargets((prev) => prev.filter((_, i) => i !== idx))
  }

  function updateTargetSchemas(idx: number, value: string) {
    setTargets((prev) =>
      prev.map((t, i) => (i === idx ? { ...t, schemas: stringToArray(value) } : t)),
    )
  }

  function updateTargetTables(idx: number, value: string) {
    setTargets((prev) =>
      prev.map((t, i) => (i === idx ? { ...t, tables: stringToArray(value) } : t)),
    )
  }

  function updateTargetColumns(idx: number, value: string) {
    setTargets((prev) =>
      prev.map((t, i) => (i === idx ? { ...t, columns: stringToArray(value) } : t)),
    )
  }

  function buildTargets(): TargetEntry[] {
    return targets.map((t) => {
      if (!needsColumns) {
        const { columns: _cols, ...rest } = t
        return rest
      }
      return t
    })
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const nErr = validatePolicyName(name)
    setNameError(nErr)
    if (nErr) return
    await onSubmit({
      name,
      description,
      policy_type: policyType,
      is_enabled: isEnabled,
      targets: buildTargets(),
      filter_expression: filterExpression,
      mask_expression: maskExpression,
    })
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-6">
      {/* Basic info */}
      <div className="grid grid-cols-2 gap-4">
        <div className="col-span-2">
          <label className="block text-sm font-medium text-gray-700 mb-1">Name</label>
          <input
            type="text"
            value={name}
            onChange={(e) => { setName(e.target.value); setNameError(null) }}
            onBlur={() => setNameError(validatePolicyName(name))}
            required
            placeholder="e.g. tenant-isolation"
            className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
          {nameError ? (
            <p className="text-xs text-red-600 mt-1">{nameError}</p>
          ) : (
            <p className="text-xs text-gray-400 mt-1">1–100 chars · no leading/trailing spaces · letters, digits, spaces, <code>_ - . : ( ) ' "</code></p>
          )}
        </div>

        <div className="col-span-2">
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Description <span className="text-gray-400 font-normal">(optional)</span>
          </label>
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Brief description of what this policy does"
            className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">Policy Type</label>
          <select
            value={policyType}
            onChange={(e) => setPolicyType(e.target.value as PolicyType)}
            className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            {POLICY_TYPES.map((t) => (
              <option key={t.value} value={t.value}>
                {t.label}
              </option>
            ))}
          </select>
          {isDeny && (
            <p className="text-xs text-amber-600 mt-1">Deny policies short-circuit or strip columns.</p>
          )}
        </div>

        <div className="flex items-center gap-3 pt-6">
          <button
            type="button"
            onClick={() => setIsEnabled((v) => !v)}
            className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors focus:outline-none ${
              isEnabled ? 'bg-blue-600' : 'bg-gray-300'
            }`}
          >
            <span
              className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform ${
                isEnabled ? 'translate-x-4.5' : 'translate-x-0.5'
              }`}
            />
          </button>
          <span className="text-sm text-gray-700">{isEnabled ? 'Enabled' : 'Disabled'}</span>
        </div>
      </div>

      {/* Targets */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-semibold text-gray-900">Targets</h3>
          <button
            type="button"
            onClick={addTarget}
            className="text-sm text-blue-600 hover:text-blue-800 font-medium"
          >
            + Add target
          </button>
        </div>

        {targets.length === 0 ? (
          <p className="text-sm text-gray-400 italic">No targets yet. Add one to define where this policy applies.</p>
        ) : (
          <div className="space-y-4">
            {targets.map((target, idx) => (
              <div key={idx} className="border border-gray-200 rounded-lg p-4 bg-gray-50">
                <div className="flex items-center justify-between mb-3">
                  <span className="text-xs font-medium text-gray-500 uppercase tracking-wide">
                    Target {idx + 1}
                  </span>
                  <button
                    type="button"
                    onClick={() => removeTarget(idx)}
                    className="text-xs text-red-500 hover:text-red-700"
                  >
                    Remove
                  </button>
                </div>

                <div className={`grid gap-3 mb-3 ${needsColumns ? 'grid-cols-3' : 'grid-cols-2'}`}>
                  <div>
                    <label className="block text-xs font-medium text-gray-600 mb-1">Schemas</label>
                    <input
                      type="text"
                      value={targetToString(target.schemas)}
                      onChange={(e) => updateTargetSchemas(idx, e.target.value)}
                      placeholder="public, analytics"
                      className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
                    />
                    <p className="text-xs text-gray-400 mt-1">Comma-separated. Use <code className="bg-gray-100 px-1 rounded">*</code> for all.</p>
                  </div>
                  <div>
                    <label className="block text-xs font-medium text-gray-600 mb-1">Tables</label>
                    <input
                      type="text"
                      value={targetToString(target.tables)}
                      onChange={(e) => updateTargetTables(idx, e.target.value)}
                      placeholder="orders, customers"
                      className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
                    />
                    <p className="text-xs text-gray-400 mt-1">Comma-separated. Use <code className="bg-gray-100 px-1 rounded">*</code> for all.</p>
                  </div>
                  {needsColumns && (
                    <div>
                      <label className="block text-xs font-medium text-gray-600 mb-1">Columns</label>
                      <input
                        type="text"
                        value={targetToString(target.columns ?? [])}
                        onChange={(e) => updateTargetColumns(idx, e.target.value)}
                        placeholder="ssn, salary"
                        className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500"
                      />
                      <p className="text-xs text-gray-400 mt-1">Comma-separated. Use <code className="bg-gray-100 px-1 rounded">*</code> for all.</p>
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Definition — conditional on policy type */}
      {needsFilter && (
        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">Filter Expression</label>
          <input
            type="text"
            value={filterExpression}
            onChange={(e) => setFilterExpression(e.target.value)}
            placeholder="organization_id = {user.tenant}"
            className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
          <p className="text-xs text-gray-400 mt-1">
            Use <code className="bg-gray-100 px-1 rounded">{'{user.tenant}'}</code>,{' '}
            <code className="bg-gray-100 px-1 rounded">{'{user.username}'}</code>, or{' '}
            <code className="bg-gray-100 px-1 rounded">{'{user.id}'}</code> as placeholders.
          </p>
        </div>
      )}

      {needsMask && (
        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">Mask Expression</label>
          <input
            type="text"
            value={maskExpression}
            onChange={(e) => setMaskExpression(e.target.value)}
            placeholder="CONCAT('***-**-', RIGHT(ssn, 4))"
            className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
          <p className="text-xs text-gray-400 mt-1">
            SQL expression to replace the column value. Reference the column by name.
          </p>
        </div>
      )}

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg px-4 py-3">
          {error}
        </div>
      )}

      <button
        type="submit"
        disabled={isSubmitting}
        className="bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg px-5 py-2 transition-colors disabled:opacity-50"
      >
        {isSubmitting ? 'Saving…' : submitLabel}
      </button>
    </form>
  )
}
