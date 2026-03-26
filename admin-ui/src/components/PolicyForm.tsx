import { useState, useEffect } from 'react'
import type { PolicyResponse, PolicyType, TargetEntry } from '../types/policy'
import type { EvaluateContext, OnErrorBehavior, LogLevel, DecisionFunctionResponse } from '../types/decisionFunction'
import { testDecisionFn, createDecisionFunction, updateDecisionFunction } from '../api/decisionFunctions'
import { validatePolicyName } from '../utils/nameValidation'

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
  initialDecisionFunction?: DecisionFunctionResponse | null
  decisionFnLoading?: boolean
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

// --- Main PolicyForm component ---

export function PolicyForm({ initial, initialDecisionFunction, decisionFnLoading, onSubmit, submitLabel, isSubmitting, error, catalogHints }: PolicyFormProps) {
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

  // Decision function state
  const [showDecisionFn, setShowDecisionFn] = useState(!!initialDecisionFunction || !!initial?.decision_function_id)
  const [decisionFnId, setDecisionFnId] = useState<string | null>(initial?.decision_function_id ?? null)
  const [decisionFnName, setDecisionFnName] = useState(initialDecisionFunction?.name ?? '')
  const [decisionFnDescription, setDecisionFnDescription] = useState(initialDecisionFunction?.description ?? '')
  const [decisionFn, setDecisionFn] = useState(initialDecisionFunction?.decision_fn ?? '')
  const [decisionConfigStr, setDecisionConfigStr] = useState(
    initialDecisionFunction?.decision_config ? JSON.stringify(initialDecisionFunction.decision_config, null, 2) : '{}',
  )
  const [evaluateContext, setEvaluateContext] = useState<EvaluateContext>(
    initialDecisionFunction?.evaluate_context ?? 'session',
  )
  const [onError, setOnError] = useState<OnErrorBehavior>(initialDecisionFunction?.on_error ?? 'deny')
  const [logLevel, setLogLevel] = useState<LogLevel>(initialDecisionFunction?.log_level ?? 'off')
  const [decisionFnIsEnabled, setDecisionFnIsEnabled] = useState(initialDecisionFunction?.is_enabled ?? true)
  const [decisionFnVersion, setDecisionFnVersion] = useState(initialDecisionFunction?.version ?? 0)
  const [decisionFnError, setDecisionFnError] = useState<string | null>(null)
  const [decisionFnSaving, setDecisionFnSaving] = useState(false)

  // Sync decision function state when it loads asynchronously
  useEffect(() => {
    if (!initialDecisionFunction) return
    setDecisionFnId(initialDecisionFunction.id)
    setDecisionFnName(initialDecisionFunction.name)
    setDecisionFnDescription(initialDecisionFunction.description ?? '')
    setDecisionFn(initialDecisionFunction.decision_fn)
    setDecisionConfigStr(
      initialDecisionFunction.decision_config
        ? JSON.stringify(initialDecisionFunction.decision_config, null, 2)
        : '{}',
    )
    setEvaluateContext(initialDecisionFunction.evaluate_context)
    setOnError(initialDecisionFunction.on_error)
    setLogLevel(initialDecisionFunction.log_level)
    setDecisionFnIsEnabled(initialDecisionFunction.is_enabled)
    setDecisionFnVersion(initialDecisionFunction.version)
    setShowDecisionFn(true)
  }, [initialDecisionFunction])

  // Test panel state
  const [testContextStr, setTestContextStr] = useState(
    JSON.stringify(
      {
        session: {
          user: { id: '00000000-0000-0000-0000-000000000000', username: 'testuser', tenant: 'default', roles: ['analyst'] },
          time: { hour: 14, day_of_week: 'Monday' },
          datasource: { name: 'my_ds', access_mode: 'policy_required' },
        },
        ...(initialDecisionFunction?.evaluate_context === 'query'
          ? {
              query: {
                tables: ['orders'],
                columns: ['id', 'amount'],
                join_count: 0,
                has_aggregation: false,
                has_subquery: false,
                has_where: true,
                statement_type: 'SELECT',
              },
            }
          : {}),
      },
      null,
      2,
    ),
  )
  const [testResult, setTestResult] = useState<string | null>(null)
  const [isTesting, setIsTesting] = useState(false)

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

  async function handleSaveDecisionFunction() {
    setDecisionFnSaving(true)
    setDecisionFnError(null)
    try {
      let config: Record<string, unknown> = {}
      try { config = JSON.parse(decisionConfigStr) } catch { /* keep empty */ }

      if (decisionFnId) {
        // Update existing
        const updated = await updateDecisionFunction(decisionFnId, {
          name: decisionFnName || undefined,
          description: decisionFnDescription || undefined,
          decision_fn: decisionFn,
          decision_config: config,
          evaluate_context: evaluateContext,
          on_error: onError,
          log_level: logLevel,
          is_enabled: decisionFnIsEnabled,
          version: decisionFnVersion,
        })
        setDecisionFnVersion(updated.version)
        setDecisionFnId(updated.id)
      } else {
        // Create new
        const created = await createDecisionFunction({
          name: decisionFnName || `${name} (decision)`,
          description: decisionFnDescription || undefined,
          decision_fn: decisionFn,
          decision_config: config,
          evaluate_context: evaluateContext,
          on_error: onError,
          log_level: logLevel,
        })
        setDecisionFnId(created.id)
        setDecisionFnVersion(created.version)
        if (!decisionFnName) setDecisionFnName(created.name)
      }
    } catch (err) {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error ?? 'Failed to save decision function'
      setDecisionFnError(msg)
    } finally {
      setDecisionFnSaving(false)
    }
  }

  function handleDetachDecisionFunction() {
    setDecisionFnId(null)
    setShowDecisionFn(false)
  }

  async function handleTestDecisionFn() {
    setIsTesting(true)
    setTestResult(null)
    try {
      const context = JSON.parse(testContextStr)
      const config = JSON.parse(decisionConfigStr)
      const result = await testDecisionFn({ decision_fn: decisionFn, context, config })
      setTestResult(JSON.stringify(result, null, 2))
    } catch (err) {
      setTestResult(`Error: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setIsTesting(false)
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
      decision_function_id: decisionFnId,
    }

    await onSubmit(values)
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

      {/* Decision Function (Optional) */}
      <div className="border border-gray-200 rounded-lg overflow-hidden">
        <button
          type="button"
          onClick={() => setShowDecisionFn((v) => !v)}
          className="w-full flex items-center justify-between px-4 py-3 bg-gray-50 hover:bg-gray-100 transition-colors"
        >
          <span className="text-sm font-semibold text-gray-900">
            Decision Function{' '}
            <span className="text-gray-400 font-normal">
              {decisionFnId ? `(${initial?.decision_function?.name ?? 'attached'})` : '(Optional)'}
            </span>
          </span>
          <span className="text-gray-400 text-xs">{showDecisionFn ? 'Collapse' : 'Expand'}</span>
        </button>

        {showDecisionFn && decisionFnLoading && (
          <div className="p-4 flex items-center gap-2 text-sm text-gray-400">
            <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24" fill="none">
              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
            </svg>
            Loading decision function…
          </div>
        )}
        {showDecisionFn && !decisionFnLoading && (
          <div className="p-4 space-y-4">
            <p className="text-xs text-gray-500">
              A JavaScript function that gates whether this policy fires. If the function returns{' '}
              <code className="bg-gray-100 px-1 rounded">{'{ fire: false }'}</code>, the policy is skipped.
              Decision functions are saved as separate entities and can be shared across policies.
            </p>

            {/* Function name */}
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Function Name</label>
              <input
                type="text"
                value={decisionFnName}
                onChange={(e) => setDecisionFnName(e.target.value)}
                placeholder={`${name || 'policy'} (decision)`}
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
              />
            </div>

            <div className="grid grid-cols-3 gap-4">
              <div>
                <label className="block text-xs font-medium text-gray-600 mb-1">Evaluate Context</label>
                <select
                  value={evaluateContext}
                  onChange={(e) => setEvaluateContext(e.target.value as EvaluateContext)}
                  className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
                >
                  <option value="session">Session (user/time context)</option>
                  <option value="query">Query (full context)</option>
                </select>
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-600 mb-1">On Error</label>
                <select
                  value={onError}
                  onChange={(e) => setOnError(e.target.value as OnErrorBehavior)}
                  className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
                >
                  <option value="deny">Deny (fail-secure)</option>
                  <option value="skip">Skip (fail-open)</option>
                </select>
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-600 mb-1">Log Level</label>
                <select
                  value={logLevel}
                  onChange={(e) => setLogLevel(e.target.value as LogLevel)}
                  className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500"
                >
                  <option value="off">Off</option>
                  <option value="error">Error only</option>
                  <option value="info">Info (console.log + errors)</option>
                </select>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => setDecisionFnIsEnabled((v) => !v)}
                className={`relative inline-flex h-4 w-7 items-center rounded-full transition-colors focus:outline-none ${
                  decisionFnIsEnabled ? 'bg-blue-600' : 'bg-gray-300'
                }`}
              >
                <span
                  className={`inline-block h-3 w-3 transform rounded-full bg-white transition-transform ${
                    decisionFnIsEnabled ? 'translate-x-3.5' : 'translate-x-0.5'
                  }`}
                />
              </button>
              <span className="text-xs text-gray-600">
                {decisionFnIsEnabled ? 'Function enabled (gates policy)' : 'Function paused (policy always fires)'}
              </span>
            </div>

            {/* Code Editor */}
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Function Source</label>
              <textarea
                value={decisionFn}
                onChange={(e) => setDecisionFn(e.target.value)}
                rows={12}
                placeholder={`function evaluate(ctx, config) {\n  // ctx.session.user: { id, username, tenant, roles }\n  // ctx.session.time: { hour, day_of_week }\n  // ctx.session.datasource: { name, access_mode }\n${evaluateContext === 'query' ? '  // ctx.query: { tables, columns, join_count, has_aggregation, has_subquery, has_where, statement_type }\n' : ''}\n  return { fire: true };\n}`}
                className="w-full border border-gray-300 rounded-lg px-3 py-2 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 bg-gray-900 text-green-300"
                spellCheck={false}
              />
            </div>

            {/* Config Editor */}
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Config JSON</label>
              <textarea
                value={decisionConfigStr}
                onChange={(e) => setDecisionConfigStr(e.target.value)}
                rows={3}
                placeholder='{ "max_joins": 3 }'
                className="w-full border border-gray-300 rounded-lg px-3 py-2 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500"
                spellCheck={false}
              />
              <p className="text-xs text-gray-400 mt-1">
                Parameters passed as the second argument to your function. Use this instead of hardcoding values in JavaScript — makes it easy to change thresholds per environment.
              </p>
            </div>

            {/* Reference Panel */}
            <div className="bg-blue-50 border border-blue-100 rounded-lg px-4 py-3">
              <h4 className="text-xs font-semibold text-blue-800 mb-2">Available Context</h4>
              <div className="text-xs text-blue-700 font-mono space-y-0.5">
                <p>ctx.session.user.id, .username, .tenant, .roles[]</p>
                <p>ctx.session.time.hour, .day_of_week</p>
                <p>ctx.session.datasource.name, .access_mode</p>
                {evaluateContext === 'query' && (
                  <p className="text-blue-900 font-semibold">
                    ctx.query.tables[], .columns[], .join_count, .has_aggregation, .has_subquery, .has_where, .statement_type
                  </p>
                )}
                {evaluateContext === 'session' && (
                  <p className="text-blue-400 italic">ctx.query — not available in session mode</p>
                )}
              </div>
            </div>

            {/* Test Panel */}
            <div className="border border-gray-200 rounded-lg p-3 space-y-3">
              <h4 className="text-xs font-semibold text-gray-700">Test</h4>
              <div>
                <label className="block text-xs font-medium text-gray-600 mb-1">Mock Context</label>
                <textarea
                  value={testContextStr}
                  onChange={(e) => setTestContextStr(e.target.value)}
                  rows={6}
                  className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500"
                  spellCheck={false}
                />
              </div>
              <button
                type="button"
                onClick={handleTestDecisionFn}
                disabled={isTesting || !decisionFn.trim()}
                className="bg-green-600 hover:bg-green-700 text-white text-xs font-medium rounded px-3 py-1.5 transition-colors disabled:opacity-50"
              >
                {isTesting ? 'Running…' : 'Run Test'}
              </button>
              {testResult && (
                <pre className="bg-gray-900 text-green-300 rounded p-3 text-xs font-mono overflow-x-auto whitespace-pre-wrap max-h-48 overflow-y-auto">
                  {testResult}
                </pre>
              )}
            </div>

            {/* Save / Detach actions */}
            <div className="flex items-center gap-3 pt-2 border-t border-gray-100">
              <button
                type="button"
                onClick={handleSaveDecisionFunction}
                disabled={decisionFnSaving || !decisionFn.trim()}
                className="bg-purple-600 hover:bg-purple-700 text-white text-xs font-medium rounded px-3 py-1.5 transition-colors disabled:opacity-50"
              >
                {decisionFnSaving ? 'Saving…' : decisionFnId ? 'Update Function' : 'Create Function'}
              </button>
              {decisionFnId && (
                <button
                  type="button"
                  onClick={handleDetachDecisionFunction}
                  className="text-xs text-red-500 hover:text-red-700"
                >
                  Detach Function
                </button>
              )}
              {decisionFnError && (
                <span className="text-xs text-red-600">{decisionFnError}</span>
              )}
              {decisionFnId && !decisionFnError && (
                <span className="text-xs text-green-600">Attached</span>
              )}
            </div>
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
