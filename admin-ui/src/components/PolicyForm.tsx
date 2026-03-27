import { useState } from 'react'
import type { PolicyResponse, PolicyType, TargetEntry } from '../types/policy'
import type { DecisionFunctionResponse, DecisionFunctionSummary } from '../types/decisionFunction'
import { listDecisionFunctions, getDecisionFunction } from '../api/decisionFunctions'
import { validatePolicyName } from '../utils/nameValidation'
import { DecisionFunctionModal } from './DecisionFunctionModal'

const POLICY_TYPES: { value: PolicyType; label: string }[] = [
  { value: 'row_filter', label: 'Row Filter' },
  { value: 'column_mask', label: 'Column Mask' },
  { value: 'column_allow', label: 'Column Allow' },
  { value: 'column_deny', label: 'Column Deny' },
  { value: 'table_deny', label: 'Table Deny' },
]

const DENY_TYPES: PolicyType[] = ['column_deny', 'table_deny']
const CHIP_DISPLAY_LIMIT = 20
const FILTER_THRESHOLD = 15

function emptyTarget(): TargetEntry {
  return { schemas: ['*'], tables: ['*'], columns: ['*'] }
}

function emptyTargetString(): { schemas: string; tables: string; columns: string } {
  return { schemas: '*', tables: '*', columns: '*' }
}

export interface CatalogHints {
  schemas: string[]
  tables: Map<string, string[]>
  columns: Map<string, string[]>
}

export interface PolicyFormValues {
  name: string
  description: string
  policy_type: PolicyType
  is_enabled: boolean
  targets: TargetEntry[]
  filter_expression: string
  mask_expression: string
  decision_function_id?: string | null
}

interface PolicyFormProps {
  initial?: PolicyResponse
  onSubmit: (values: PolicyFormValues) => Promise<void>
  submitLabel: string
  isSubmitting: boolean
  error?: string | null
  catalogHints?: CatalogHints
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

// --- TargetCard sub-component ---

interface TargetCardProps {
  idx: number
  rawSchemas: string
  rawTables: string
  rawColumns: string
  hints: { schemas: string[]; tables: string[]; columns: string[] }
  needsColumns: boolean
  onRemove: () => void
  onRawChange: (field: 'schemas' | 'tables' | 'columns', value: string) => void
  onBlur: (field: 'schemas' | 'tables' | 'columns') => void
  onChipClick: (field: 'schemas' | 'tables' | 'columns', value: string) => void
}

function TargetCard({
  idx,
  rawSchemas,
  rawTables,
  rawColumns,
  hints,
  needsColumns,
  onRemove,
  onRawChange,
  onBlur,
  onChipClick,
}: TargetCardProps) {
  const [schemaFilter, setSchemaFilter] = useState('')
  const [tableFilter, setTableFilter] = useState('')
  const [columnFilter, setColumnFilter] = useState('')

  function renderChips(
    hintList: string[],
    filter: string,
    setFilter: (v: string) => void,
    onChip: (v: string) => void,
    filterLabel: string,
    isMono = false,
  ) {
    if (hintList.length === 0) return null
    const filtered = filter
      ? hintList.filter((h) => h.toLowerCase().includes(filter.toLowerCase()))
      : hintList
    const visible = filtered.slice(0, CHIP_DISPLAY_LIMIT)
    const overflow = filtered.length - visible.length
    return (
      <div className="mt-1.5">
        {hintList.length > FILTER_THRESHOLD && (
          <input
            type="text"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={filterLabel}
            className="w-full border border-gray-200 rounded px-2 py-0.5 text-xs mb-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400"
          />
        )}
        <div className="flex flex-wrap gap-1">
          {visible.map((v) => (
            <button
              key={v}
              type="button"
              onClick={() => onChip(v)}
              className={`inline-flex items-center px-1.5 py-0.5 rounded text-xs bg-gray-100 text-gray-600 hover:bg-blue-100 hover:text-blue-700 transition-colors${isMono ? ' font-mono' : ''}`}
            >
              {v}
            </button>
          ))}
          {overflow > 0 && (
            <span className="text-xs text-gray-400 self-center">+{overflow} more</span>
          )}
        </div>
      </div>
    )
  }

  return (
    <div className="border border-gray-200 rounded-lg p-4 bg-gray-50">
      <div className="flex items-center justify-between mb-3">
        <span className="text-xs font-medium text-gray-500 uppercase tracking-wide">
          Target {idx + 1}
        </span>
        <button
          type="button"
          onClick={onRemove}
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
            value={rawSchemas}
            onChange={(e) => onRawChange('schemas', e.target.value)}
            onBlur={() => onBlur('schemas')}
            placeholder="public, analytics"
            className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
          />
          <p className="text-xs text-gray-400 mt-1">
            Comma-separated. Use <code className="bg-gray-100 px-1 rounded">*</code> for all.
          </p>
          {renderChips(
            hints.schemas,
            schemaFilter,
            setSchemaFilter,
            (v) => onChipClick('schemas', v),
            'Filter schemas…',
          )}
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-600 mb-1">Tables</label>
          <input
            type="text"
            value={rawTables}
            onChange={(e) => onRawChange('tables', e.target.value)}
            onBlur={() => onBlur('tables')}
            placeholder="orders, customers"
            className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
          />
          <p className="text-xs text-gray-400 mt-1">
            Comma-separated. Use <code className="bg-gray-100 px-1 rounded">*</code> for all.
          </p>
          {renderChips(
            hints.tables,
            tableFilter,
            setTableFilter,
            (v) => onChipClick('tables', v),
            'Filter tables…',
          )}
        </div>
        {needsColumns && (
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">Columns</label>
            <input
              type="text"
              value={rawColumns}
              onChange={(e) => onRawChange('columns', e.target.value)}
              onBlur={() => onBlur('columns')}
              placeholder="ssn, salary"
              className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
            <p className="text-xs text-gray-400 mt-1">
              Comma-separated. Use <code className="bg-gray-100 px-1 rounded">*</code> for all.
            </p>
            {renderChips(
              hints.columns,
              columnFilter,
              setColumnFilter,
              (v) => onChipClick('columns', v),
              'Filter columns…',
              true,
            )}
          </div>
        )}
      </div>
    </div>
  )
}

// --- Section header helper ---

function SectionHeader({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <div className="flex items-baseline gap-2 mb-3">
      <h3 className="text-sm font-semibold text-gray-900">{title}</h3>
      <span className="text-xs text-gray-400">&middot; {subtitle}</span>
    </div>
  )
}

// --- Main PolicyForm component ---

export function PolicyForm({ initial, onSubmit, submitLabel, isSubmitting, error, catalogHints }: PolicyFormProps) {
  const [name, setName] = useState(initial?.name ?? '')
  const [nameError, setNameError] = useState<string | null>(null)
  const [description, setDescription] = useState(initial?.description ?? '')
  const [policyType, setPolicyType] = useState<PolicyType>(
    (initial?.policy_type as PolicyType) ?? 'row_filter',
  )
  const [isEnabled, setIsEnabled] = useState(initial?.is_enabled ?? true)

  const initialTargets = initial ? targetsFromPolicy(initial) : [emptyTarget()]
  const [targets, setTargets] = useState<TargetEntry[]>(initialTargets)
  const [targetStrings, setTargetStrings] = useState(
    initialTargets.map((t) => ({
      schemas: targetToString(t.schemas),
      tables: targetToString(t.tables),
      columns: targetToString(t.columns ?? []),
    })),
  )

  const [filterExpression, setFilterExpression] = useState(
    initial?.definition?.filter_expression ?? '',
  )
  const [maskExpression, setMaskExpression] = useState(
    initial?.definition?.mask_expression ?? '',
  )

  // Decision function state.
  // attachedFnSummary holds lightweight display info (name, enabled, context).
  // Seeded from initial.decision_function (embedded in policy response).
  // Updated when the user saves via modal or selects an existing function.
  // The modal fetches the full DecisionFunctionResponse itself when opened for editing.
  const initialSummary: DecisionFunctionSummary | null = initial?.decision_function ?? null
  const [useDecisionFn, setUseDecisionFn] = useState(!!initial?.decision_function_id)
  const [attachedFnSummary, setAttachedFnSummary] = useState<DecisionFunctionSummary | null>(initialSummary)
  const [attachedFnDescription, setAttachedFnDescription] = useState<string | null>(null)
  const [attachedFnId, setAttachedFnId] = useState<string | null>(initial?.decision_function_id ?? null)
  const [attachedFnError, setAttachedFnError] = useState<string | null>(null)
  const [modalOpen, setModalOpen] = useState(false)
  const [modalInitialId, setModalInitialId] = useState<string | null>(null)

  // Select existing state
  const [showSelectExisting, setShowSelectExisting] = useState(false)
  const [existingFunctions, setExistingFunctions] = useState<DecisionFunctionResponse[]>([])
  const [loadingExisting, setLoadingExisting] = useState(false)

  const needsColumns = policyType === 'column_mask' || policyType === 'column_allow' || policyType === 'column_deny'
  const needsFilter = policyType === 'row_filter'
  const needsMask = policyType === 'column_mask'
  const isDeny = DENY_TYPES.includes(policyType)

  function addTarget() {
    setTargets((prev) => [...prev, emptyTarget()])
    setTargetStrings((prev) => [...prev, emptyTargetString()])
  }

  function removeTarget(idx: number) {
    setTargets((prev) => prev.filter((_, i) => i !== idx))
    setTargetStrings((prev) => prev.filter((_, i) => i !== idx))
  }

  function updateRaw(idx: number, field: 'schemas' | 'tables' | 'columns', value: string) {
    setTargetStrings((prev) =>
      prev.map((ts, i) => (i === idx ? { ...ts, [field]: value } : ts)),
    )
  }

  function syncFromRaw(idx: number, field: 'schemas' | 'tables' | 'columns') {
    const raw = targetStrings[idx]?.[field] ?? ''
    const parsed = stringToArray(raw)
    const value = parsed.length > 0 ? parsed : ['*']
    setTargets((prev) =>
      prev.map((t, i) => (i === idx ? { ...t, [field]: value } : t)),
    )
  }

  function appendToTarget(idx: number, field: 'schemas' | 'tables' | 'columns', value: string) {
    setTargets((prev) =>
      prev.map((t, i) => {
        if (i !== idx) return t
        const current = field === 'columns' ? (t.columns ?? []) : t[field]
        const updated =
          current.length === 1 && current[0] === '*'
            ? [value]
            : current.includes(value)
              ? current
              : [...current, value]
        return { ...t, [field]: updated }
      }),
    )
    setTargetStrings((prev) =>
      prev.map((ts, i) => {
        if (i !== idx) return ts
        const current = stringToArray(ts[field])
        const updated =
          current.length === 1 && current[0] === '*'
            ? [value]
            : current.includes(value)
              ? current
              : [...current, value]
        return { ...ts, [field]: updated.join(', ') }
      }),
    )
  }

  function getFilteredHints(targetIdx: number): {
    schemas: string[]
    tables: string[]
    columns: string[]
  } {
    if (!catalogHints) return { schemas: [], tables: [], columns: [] }
    const target = targets[targetIdx]
    const currentSchemas = target.schemas.filter((s) => s !== '*')
    const currentTables = target.tables.filter((t) => t !== '*')
    const currentColumns = (target.columns ?? []).filter((c) => c !== '*')

    const schemaHints = catalogHints.schemas.filter((s) => !target.schemas.includes(s))

    let tableHints: string[]
    if (target.schemas.includes('*') || currentSchemas.length === 0) {
      const all = new Set<string>()
      catalogHints.tables.forEach((ts) => ts.forEach((t) => all.add(t)))
      tableHints = Array.from(all).filter((t) => !target.tables.includes(t))
    } else {
      const relevant = new Set<string>()
      currentSchemas.forEach((s) =>
        (catalogHints.tables.get(s) ?? []).forEach((t) => relevant.add(t)),
      )
      tableHints = Array.from(relevant).filter((t) => !target.tables.includes(t))
    }

    let columnHints: string[]
    const allSchemas = target.schemas.includes('*') || currentSchemas.length === 0
    const allTables = (target.tables ?? []).includes('*') || currentTables.length === 0
    if (allSchemas && allTables) {
      const all = new Set<string>()
      catalogHints.columns.forEach((cs) => cs.forEach((c) => all.add(c)))
      columnHints = Array.from(all).filter((c) => !currentColumns.includes(c))
    } else {
      const schemasForCols = allSchemas
        ? Array.from(catalogHints.tables.keys())
        : currentSchemas
      const tablesForCols = allTables
        ? Array.from(new Set(schemasForCols.flatMap((s) => catalogHints.tables.get(s) ?? [])))
        : currentTables
      const relevant = new Set<string>()
      schemasForCols.forEach((s) =>
        tablesForCols.forEach((t) =>
          (catalogHints.columns.get(`${s}.${t}`) ?? []).forEach((c) => relevant.add(c)),
        ),
      )
      columnHints = Array.from(relevant).filter((c) => !currentColumns.includes(c))
    }

    return { schemas: schemaHints, tables: tableHints, columns: columnHints }
  }

  function buildTargets(): TargetEntry[] {
    return targetStrings.map((ts) => {
      const schemas = stringToArray(ts.schemas)
      const tables = stringToArray(ts.tables)
      const cols = stringToArray(ts.columns)
      const entry: TargetEntry = {
        schemas: schemas.length > 0 ? schemas : ['*'],
        tables: tables.length > 0 ? tables : ['*'],
      }
      if (needsColumns) {
        entry.columns = cols.length > 0 ? cols : ['*']
      }
      return entry
    })
  }

  // Decision function handlers
  function handleToggleDecisionFn(on: boolean) {
    setUseDecisionFn(on)
    if (!on) {
      setAttachedFnError(null)
      setShowSelectExisting(false)
    }
  }

  function handleDetach() {
    setAttachedFnSummary(null)
    setAttachedFnDescription(null)
    setAttachedFnId(null)
    setAttachedFnError(null)
    // Keep toggle ON so user can re-attach
  }

  function handleOpenCreateModal() {
    setModalInitialId(null)
    setModalOpen(true)
  }

  function handleOpenEditModal() {
    setModalInitialId(attachedFnId)
    setModalOpen(true)
  }

  function handleModalSaved(fn: DecisionFunctionResponse) {
    setAttachedFnSummary({ id: fn.id, name: fn.name, is_enabled: fn.is_enabled, evaluate_context: fn.evaluate_context })
    setAttachedFnDescription(fn.description ?? null)
    setAttachedFnId(fn.id)
    setAttachedFnError(null)
    setModalOpen(false)
  }

  async function handleShowSelectExisting() {
    setShowSelectExisting(true)
    setLoadingExisting(true)
    try {
      const fns = await listDecisionFunctions()
      setExistingFunctions(fns)
    } catch {
      setExistingFunctions([])
    } finally {
      setLoadingExisting(false)
    }
  }

  async function handleSelectExisting(fnId: string) {
    try {
      const fn = await getDecisionFunction(fnId)
      setAttachedFnSummary({ id: fn.id, name: fn.name, is_enabled: fn.is_enabled, evaluate_context: fn.evaluate_context })
      setAttachedFnDescription(fn.description ?? null)
      setAttachedFnId(fn.id)
      setAttachedFnError(null)
      setShowSelectExisting(false)
    } catch {
      setAttachedFnError('Failed to load selected function')
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const nErr = validatePolicyName(name)
    setNameError(nErr)
    if (nErr) return

    const values: PolicyFormValues = {
      name,
      description,
      policy_type: policyType,
      is_enabled: isEnabled,
      targets: buildTargets(),
      filter_expression: filterExpression,
      mask_expression: maskExpression,
      decision_function_id: useDecisionFn && attachedFnId ? attachedFnId : null,
    }

    await onSubmit(values)
  }

  // Stale ref: policy has a decision_function_id but the server didn't return a summary
  // (the referenced function was deleted from DB). Auto-clears when user detaches.
  const isStaleRef = !!initial?.decision_function_id && !initial?.decision_function && attachedFnId === initial?.decision_function_id

  return (
    <form onSubmit={handleSubmit} className="space-y-6">
      {/* Section 1: Policy */}
      <div>
        <SectionHeader title="Policy" subtitle="name and status" />
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

          <div className="flex items-center gap-3 pt-2">
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
      </div>

      {/* Section 2: Effect — what this policy does */}
      <div>
        <SectionHeader title="Effect" subtitle="what this policy does" />

        <div className="grid grid-cols-2 gap-4 mb-4">
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
      </div>

      {/* Section 3: Targets — where it applies */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-baseline gap-2">
            <h3 className="text-sm font-semibold text-gray-900">Targets</h3>
            <span className="text-xs text-gray-400">&middot; where it applies</span>
          </div>
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
            {targets.map((_target, idx) => {
              const hints = getFilteredHints(idx)
              const ts = targetStrings[idx] ?? emptyTargetString()
              return (
                <TargetCard
                  key={idx}
                  idx={idx}
                  rawSchemas={ts.schemas}
                  rawTables={ts.tables}
                  rawColumns={ts.columns}
                  hints={hints}
                  needsColumns={needsColumns}
                  onRemove={() => removeTarget(idx)}
                  onRawChange={(field, value) => updateRaw(idx, field, value)}
                  onBlur={(field) => syncFromRaw(idx, field)}
                  onChipClick={(field, value) => appendToTarget(idx, field, value)}
                />
              )
            })}
          </div>
        )}
      </div>

      {/* Section 4: Decision Function — when it fires (optional) */}
      <div>
        <SectionHeader title="Decision Function" subtitle="when it fires (optional)" />

        <div className="border border-gray-200 rounded-lg p-4">
          {/* Toggle */}
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={() => handleToggleDecisionFn(!useDecisionFn)}
              className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors focus:outline-none ${
                useDecisionFn ? 'bg-blue-600' : 'bg-gray-300'
              }`}
              aria-label="Use decision function"
            >
              <span
                className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform ${
                  useDecisionFn ? 'translate-x-4.5' : 'translate-x-0.5'
                }`}
              />
            </button>
            <span className="text-sm text-gray-700">Use decision function</span>
          </div>

          {/* Toggle OFF */}
          {!useDecisionFn && (
            <p className="text-xs text-gray-400 mt-2">
              Policy always fires. No custom logic evaluated.
            </p>
          )}

          {/* Toggle ON, stale reference */}
          {useDecisionFn && isStaleRef && (
            <div className="mt-3 bg-amber-50 border border-amber-200 text-amber-800 text-sm rounded-lg px-4 py-3 flex items-center justify-between">
              <span>Function not found — detach and create a new one.</span>
              <button
                type="button"
                onClick={handleDetach}
                className="text-xs text-red-600 hover:text-red-800 font-medium ml-3"
              >
                Detach
              </button>
            </div>
          )}

          {/* Toggle ON, no function attached */}
          {useDecisionFn && !attachedFnSummary && !isStaleRef && (
            <div className="mt-3 flex items-center gap-3">
              <button
                type="button"
                onClick={handleOpenCreateModal}
                className="text-sm text-blue-600 hover:text-blue-800 font-medium"
              >
                + Create New
              </button>
              {!showSelectExisting ? (
                <button
                  type="button"
                  onClick={handleShowSelectExisting}
                  className="text-sm text-blue-600 hover:text-blue-800 font-medium"
                >
                  Select Existing
                </button>
              ) : (
                <div className="flex items-center gap-2">
                  {loadingExisting ? (
                    <span className="text-xs text-gray-400">Loading…</span>
                  ) : existingFunctions.length === 0 ? (
                    <span className="text-xs text-gray-400">No functions available</span>
                  ) : (
                    <select
                      onChange={(e) => { if (e.target.value) handleSelectExisting(e.target.value) }}
                      defaultValue=""
                      className="border border-gray-300 rounded px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
                    >
                      <option value="" disabled>Pick a function…</option>
                      {existingFunctions.map((fn) => (
                        <option key={fn.id} value={fn.id}>{fn.name}</option>
                      ))}
                    </select>
                  )}
                  <button
                    type="button"
                    onClick={() => setShowSelectExisting(false)}
                    className="text-xs text-gray-400 hover:text-gray-600"
                  >
                    Cancel
                  </button>
                </div>
              )}
              {attachedFnError && (
                <span className="text-xs text-red-600">{attachedFnError}</span>
              )}
            </div>
          )}

          {/* Toggle ON, function attached — compact widget */}
          {useDecisionFn && attachedFnSummary && (
            <div className="mt-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <span className="text-sm font-mono text-gray-900">
                    <span className="text-gray-400">ƒ</span> {attachedFnSummary.name}
                  </span>
                  <span className="text-xs text-gray-500">
                    {attachedFnSummary.is_enabled ? (
                      <span className="text-green-600">Enabled</span>
                    ) : (
                      <span className="text-gray-400">Disabled</span>
                    )}
                    {' · '}
                    {attachedFnSummary.evaluate_context === 'query' ? 'Query' : 'Session'}
                  </span>
                </div>
              </div>
              {attachedFnDescription && (
                <p className="text-xs text-gray-500 mt-1">{attachedFnDescription}</p>
              )}
              <div className="flex items-center gap-3 mt-2">
                <button
                  type="button"
                  onClick={handleOpenEditModal}
                  className="text-xs text-blue-600 hover:text-blue-800 font-medium"
                >
                  Edit
                </button>
                <button
                  type="button"
                  onClick={handleDetach}
                  className="text-xs text-red-500 hover:text-red-700"
                >
                  Detach
                </button>
              </div>
            </div>
          )}
        </div>
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

      {/* Decision Function Modal */}
      <DecisionFunctionModal
        open={modalOpen}
        onClose={() => setModalOpen(false)}
        onSaved={handleModalSaved}
        initialId={modalInitialId}
        policyName={name}
      />
    </form>
  )
}
