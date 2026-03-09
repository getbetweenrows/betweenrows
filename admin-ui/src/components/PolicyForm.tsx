import { useState } from 'react'
import type { ObligationRequest, PolicyResponse } from '../types/policy'
import { validatePolicyName } from '../utils/nameValidation'

const OBLIGATION_TYPES = [
  { value: 'row_filter', label: 'Row Filter' },
  { value: 'column_mask', label: 'Column Mask' },
  { value: 'column_access', label: 'Column Access' },
  { value: 'object_access', label: 'Object Access (Deny Schema/Table)' },
]

interface ObligationFormState {
  obligation_type: string
  schema: string
  table: string
  filter_expression: string
  column: string
  columns: string
  mask_expression: string
  action: string
}

function emptyObligation(): ObligationFormState {
  return {
    obligation_type: 'row_filter',
    schema: '*',
    table: '*',
    filter_expression: '',
    column: '',
    columns: '',
    mask_expression: '',
    action: 'deny',
  }
}

function obligationToRequest(o: ObligationFormState): ObligationRequest {
  if (o.obligation_type === 'row_filter') {
    const base = { schema: o.schema || '*', table: o.table || '*' }
    return { obligation_type: 'row_filter', definition: { ...base, filter_expression: o.filter_expression } }
  }
  if (o.obligation_type === 'column_mask') {
    const base = { schema: o.schema || '*', table: o.table || '*' }
    return {
      obligation_type: 'column_mask',
      definition: { ...base, column: o.column, mask_expression: o.mask_expression },
    }
  }
  if (o.obligation_type === 'object_access') {
    const def: Record<string, unknown> = { schema: o.schema || '*', action: 'deny' }
    if (o.table && o.table !== '' && o.table !== '*') def.table = o.table
    return { obligation_type: 'object_access', definition: def }
  }
  // default: column_access
  const base = { schema: o.schema || '*', table: o.table || '*' }
  return {
    obligation_type: 'column_access',
    definition: {
      ...base,
      columns: o.columns.split(',').map((c) => c.trim()).filter(Boolean),
      action: 'deny',
    },
  }
}

function responseToFormState(obl: { obligation_type: string; definition: Record<string, unknown> }): ObligationFormState {
  const d = obl.definition
  return {
    obligation_type: obl.obligation_type,
    schema: String(d.schema ?? '*'),
    table: String(d.table ?? ''),
    filter_expression: String(d.filter_expression ?? ''),
    column: String(d.column ?? ''),
    columns: Array.isArray(d.columns) ? (d.columns as string[]).join(', ') : String(d.columns ?? ''),
    mask_expression: String(d.mask_expression ?? ''),
    action: String(d.action ?? 'deny'),
  }
}

export interface PolicyFormValues {
  name: string
  description: string
  effect: 'permit' | 'deny'
  is_enabled: boolean
  obligations: ObligationRequest[]
}

interface PolicyFormProps {
  initial?: PolicyResponse
  onSubmit: (values: PolicyFormValues) => Promise<void>
  submitLabel: string
  isSubmitting: boolean
  error?: string | null
}

export function PolicyForm({ initial, onSubmit, submitLabel, isSubmitting, error }: PolicyFormProps) {
  const [name, setName] = useState(initial?.name ?? '')
  const [nameError, setNameError] = useState<string | null>(null)
  const [description, setDescription] = useState(initial?.description ?? '')
  const [effect, setEffect] = useState<'permit' | 'deny'>(initial?.effect ?? 'permit')
  const [isEnabled, setIsEnabled] = useState(initial?.is_enabled ?? true)
  const [obligations, setObligations] = useState<ObligationFormState[]>(
    initial?.obligations?.map(responseToFormState) ?? [],
  )

  const availableObligationTypes = effect === 'deny'
    ? OBLIGATION_TYPES.filter(t => t.value !== 'column_mask')
    : OBLIGATION_TYPES

  function handleEffectChange(newEffect: 'permit' | 'deny') {
    setEffect(newEffect)
    if (newEffect === 'deny') {
      setObligations(prev => prev.filter(o => o.obligation_type !== 'column_mask'))
    }
  }

  function addObligation() {
    setObligations((prev) => [...prev, emptyObligation()])
  }

  function removeObligation(idx: number) {
    setObligations((prev) => prev.filter((_, i) => i !== idx))
  }

  function updateObligation(idx: number, field: keyof ObligationFormState, value: string) {
    setObligations((prev) =>
      prev.map((o, i) => (i === idx ? { ...o, [field]: value } : o)),
    )
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const nErr = validatePolicyName(name)
    setNameError(nErr)
    if (nErr) return
    await onSubmit({
      name,
      description,
      effect,
      is_enabled: isEnabled,
      obligations: obligations.map(obligationToRequest),
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
          <label className="block text-sm font-medium text-gray-700 mb-1">Effect</label>
          <select
            value={effect}
            onChange={(e) => handleEffectChange(e.target.value as 'permit' | 'deny')}
            className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            <option value="permit">Permit</option>
            <option value="deny">Deny</option>
          </select>
          {effect === 'deny' && (
            <p className="text-xs text-amber-600 mt-1">Column masking is not available on deny policies.</p>
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

      {/* Obligations */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-semibold text-gray-900">Obligations</h3>
          <button
            type="button"
            onClick={addObligation}
            className="text-sm text-blue-600 hover:text-blue-800 font-medium"
          >
            + Add obligation
          </button>
        </div>

        {obligations.length === 0 ? (
          <p className="text-sm text-gray-400 italic">
            No obligations yet. Add one to define what this policy enforces.
          </p>
        ) : (
          <div className="space-y-4">
            {obligations.map((obl, idx) => (
              <div key={idx} className="border border-gray-200 rounded-lg p-4 bg-gray-50">
                <div className="flex items-center justify-between mb-3">
                  <span className="text-xs font-medium text-gray-500 uppercase tracking-wide">
                    Obligation {idx + 1}
                  </span>
                  <button
                    type="button"
                    onClick={() => removeObligation(idx)}
                    className="text-xs text-red-500 hover:text-red-700"
                  >
                    Remove
                  </button>
                </div>

                <div className="grid grid-cols-3 gap-3 mb-3">
                  <div>
                    <label className="block text-xs font-medium text-gray-600 mb-1">Type</label>
                    <select
                      value={obl.obligation_type}
                      onChange={(e) => updateObligation(idx, 'obligation_type', e.target.value)}
                      className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
                    >
                      {availableObligationTypes.map((t) => (
                        <option key={t.value} value={t.value}>
                          {t.label}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="block text-xs font-medium text-gray-600 mb-1">Schema</label>
                    <input
                      type="text"
                      value={obl.schema}
                      onChange={(e) => updateObligation(idx, 'schema', e.target.value)}
                      placeholder={obl.obligation_type === 'object_access' ? 'analytics' : '* or public'}
                      className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
                    />
                    <p className="text-xs text-gray-400 mt-1">
                      {obl.obligation_type === 'object_access'
                        ? <>Use <code className="bg-gray-100 px-1 rounded">prefix*</code> for glob matching.</>
                        : <>Use <code className="bg-gray-100 px-1 rounded">*</code> to match all schemas.</>}
                    </p>
                  </div>
                  <div>
                    <label className="block text-xs font-medium text-gray-600 mb-1">
                      Table
                      {obl.obligation_type === 'object_access' && (
                        <span className="text-gray-400 font-normal ml-1">(optional)</span>
                      )}
                    </label>
                    <input
                      type="text"
                      value={obl.table}
                      onChange={(e) => updateObligation(idx, 'table', e.target.value)}
                      placeholder={obl.obligation_type === 'object_access' ? 'leave blank for entire schema' : '* or orders'}
                      className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
                    />
                    {obl.obligation_type !== 'object_access' && (
                      <p className="text-xs text-gray-400 mt-1">Use <code className="bg-gray-100 px-1 rounded">*</code> to match all tables.</p>
                    )}
                  </div>
                </div>

                {obl.obligation_type === 'row_filter' && (
                  <div>
                    <label className="block text-xs font-medium text-gray-600 mb-1">
                      Filter expression
                    </label>
                    <input
                      type="text"
                      value={obl.filter_expression}
                      onChange={(e) => updateObligation(idx, 'filter_expression', e.target.value)}
                      placeholder="organization_id = {user.tenant}"
                      className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500"
                    />
                    <p className="text-xs text-gray-400 mt-1">
                      Use <code className="bg-gray-100 px-1 rounded">{'{user.tenant}'}</code>,{' '}
                      <code className="bg-gray-100 px-1 rounded">{'{user.username}'}</code>, or{' '}
                      <code className="bg-gray-100 px-1 rounded">{'{user.id}'}</code> as placeholders.
                    </p>
                  </div>
                )}

                {obl.obligation_type === 'column_mask' && (
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="block text-xs font-medium text-gray-600 mb-1">Column</label>
                      <input
                        type="text"
                        value={obl.column}
                        onChange={(e) => updateObligation(idx, 'column', e.target.value)}
                        placeholder="ssn"
                        className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500"
                      />
                    </div>
                    <div>
                      <label className="block text-xs font-medium text-gray-600 mb-1">
                        Mask expression
                      </label>
                      <input
                        type="text"
                        value={obl.mask_expression}
                        onChange={(e) => updateObligation(idx, 'mask_expression', e.target.value)}
                        placeholder="'***-**-' || RIGHT(ssn, 4)"
                        className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500"
                      />
                    </div>
                  </div>
                )}

                {obl.obligation_type === 'column_access' && (
                  <div>
                    <label className="block text-xs font-medium text-gray-600 mb-1">
                      Columns to deny (comma-separated)
                    </label>
                    <input
                      type="text"
                      value={obl.columns}
                      onChange={(e) => updateObligation(idx, 'columns', e.target.value)}
                      placeholder="ssn, credit_card, phone"
                      className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500"
                    />
                    <p className="text-xs text-gray-400 mt-1">
                      Combined with <code className="bg-gray-100 px-1 rounded">schema: *</code> and <code className="bg-gray-100 px-1 rounded">table: *</code> to deny a column across all tables. The table itself remains visible — only the column is hidden.
                    </p>
                  </div>
                )}

                {obl.obligation_type === 'object_access' && (
                  <p className="text-xs text-gray-500 mt-1">
                    Hides the entire schema (when Table is blank) or a specific table from the user's session.
                    Takes effect in both <em>open</em> and <em>policy-required</em> datasource modes.
                  </p>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

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
