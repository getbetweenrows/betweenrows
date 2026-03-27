import { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import CodeMirror, { type ReactCodeMirrorRef } from '@uiw/react-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { json } from '@codemirror/lang-json'
import { autocompletion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete'
import { linter, lintGutter, type Diagnostic } from '@codemirror/lint'
import type {
  DecisionFunctionResponse,
  EvaluateContext,
  OnErrorBehavior,
  LogLevel,
} from '../types/decisionFunction'
import {
  getDecisionFunction,
  createDecisionFunction,
  updateDecisionFunction,
  testDecisionFn,
} from '../api/decisionFunctions'

const TEMPLATES = [
  {
    label: 'Business hours only',
    code: `function evaluate(ctx) {
  const h = ctx.session.time.hour;
  return { fire: h >= 9 && h < 17 };
}`,
    config: '{}',
  },
  {
    label: 'Role-based',
    code: `function evaluate(ctx, config) {
  return { fire: ctx.session.user.roles.includes(config.required_role) };
}`,
    config: '{\n  "required_role": "analyst"\n}',
  },
  {
    label: 'Query complexity limit',
    code: `function evaluate(ctx, config) {
  return { fire: ctx.query.join_count <= (config.max_joins || 3) };
}`,
    config: '{\n  "max_joins": 3\n}',
  },
]

function buildCtxCompletions(evaluateContext: EvaluateContext, configStr: string) {
  const items = [
    { label: 'ctx.session.user.id', type: 'variable' as const, detail: 'UUID' },
    { label: 'ctx.session.user.username', type: 'variable' as const },
    { label: 'ctx.session.user.tenant', type: 'variable' as const },
    { label: 'ctx.session.user.roles', type: 'variable' as const, detail: 'string[]' },
    { label: 'ctx.session.time.hour', type: 'variable' as const, detail: '0-23' },
    { label: 'ctx.session.time.day_of_week', type: 'variable' as const, detail: 'Monday-Sunday' },
    { label: 'ctx.session.datasource.name', type: 'variable' as const },
    { label: 'ctx.session.datasource.access_mode', type: 'variable' as const },
  ]

  if (evaluateContext === 'query') {
    items.push(
      { label: 'ctx.query.tables', type: 'variable' as const, detail: 'string[]' },
      { label: 'ctx.query.columns', type: 'variable' as const, detail: 'string[]' },
      { label: 'ctx.query.join_count', type: 'variable' as const, detail: 'number' },
      { label: 'ctx.query.has_aggregation', type: 'variable' as const, detail: 'boolean' },
      { label: 'ctx.query.has_subquery', type: 'variable' as const, detail: 'boolean' },
      { label: 'ctx.query.has_where', type: 'variable' as const, detail: 'boolean' },
      { label: 'ctx.query.statement_type', type: 'variable' as const, detail: 'string' },
    )
  }

  // Add config.* completions from parsed config JSON
  try {
    const configObj = JSON.parse(configStr)
    if (configObj && typeof configObj === 'object') {
      for (const key of Object.keys(configObj)) {
        items.push({ label: `config.${key}`, type: 'variable' as const, detail: String(typeof configObj[key]) })
      }
    }
  } catch { /* ignore parse errors */ }

  return items
}

function ctxCompletionSource(items: { label: string; type: string; detail?: string }[]) {
  return (context: CompletionContext): CompletionResult | null => {
    // Match "ctx.", "ctx.session.", "config.", or any word with dots
    const word = context.matchBefore(/[\w.]+/)
    if (!word) return null
    // Only activate when there's a dot (property access) or at least 2 chars typed
    if (word.from === word.to) return null
    const text = context.state.sliceDoc(word.from, word.to)
    if (!text.includes('.') && text.length < 2 && !context.explicit) return null
    return {
      from: word.from,
      options: items.map((i) => ({ label: i.label, type: i.type, detail: i.detail })),
      filter: true,
    }
  }
}

/**
 * Creates a CodeMirror linter extension that validates decision function JS as the user types.
 * Runs the function against a dummy context to catch both syntax and runtime errors inline.
 * `getContext` is called on each lint pass to read the latest mock context / config from React state.
 */
function createDecisionFnLinter(getContext: () => { context: Record<string, unknown>; config: Record<string, unknown> }) {
  return linter((view) => {
    const diagnostics: Diagnostic[] = []
    const source = view.state.doc.toString()
    if (!source.trim()) return diagnostics

    // Structural check first: does the code define `evaluate`?
    if (!/\bfunction\s+evaluate\b/.test(source)) {
      diagnostics.push({
        from: 0,
        to: Math.min(source.length, 20),
        severity: 'warning',
        message: 'Expected a function named "evaluate" to be defined',
        source: 'structure',
      })
      return diagnostics
    }

    // Full execution check — catches syntax errors, runtime errors, and return shape issues
    const { context, config } = getContext()
    const result = runClientTest(source, context, config)
    if (result.error) {
      // Map errorLine to document position for inline highlighting
      let from = 0
      let to = Math.min(source.length, 1)
      if (result.errorLine) {
        const doc = view.state.doc
        if (result.errorLine <= doc.lines) {
          const line = doc.line(result.errorLine)
          from = line.from
          to = line.to
        }
      }
      diagnostics.push({
        from,
        to,
        severity: 'error',
        message: result.error,
        source: 'js-eval',
      })
    }

    return diagnostics
  }, { delay: 400 })
}

/** JSON linter — validates JSON.parse() and highlights the error position inline. */
const jsonLinter = linter((view) => {
  const source = view.state.doc.toString()
  if (!source.trim()) return []
  try {
    JSON.parse(source)
    return []
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    // V8 JSON parse errors include "at position N" or "at line N column N"
    let from = 0
    let to = Math.min(source.length, 1)
    const posMatch = msg.match(/position\s+(\d+)/)
    if (posMatch) {
      const pos = parseInt(posMatch[1], 10)
      from = Math.min(pos, source.length - 1)
      to = Math.min(pos + 1, source.length)
    }
    return [{ from, to, severity: 'error' as const, message: msg, source: 'json' }]
  }
}, { delay: 300 })

/**
 * Run the decision function in the browser's JS engine as a quick pre-check.
 * Catches syntax errors and runtime exceptions instantly without a server round-trip.
 * Returns { fire, error } — error is set if the function fails to parse or execute.
 *
 * Security note: This intentionally evaluates user-authored JS in the browser.
 * The user is writing this code themselves (it's a code editor for their own functions).
 * The authoritative execution happens server-side in a WASM sandbox with fuel limits.
 * This client-side check is purely for fast feedback on syntax/runtime errors.
 */
export function runClientTest(
  source: string,
  context: Record<string, unknown>,
  config: Record<string, unknown>,
): { fire?: boolean; error?: string; errorLine?: number } {
  // new Function('ctx','config', body) wraps as: function anonymous(ctx,config){\n<body>\n}
  // Our template literal body starts with: \n  "use strict";\n  <source>
  // So user source line 1 is at inner <anonymous> line 4 (1 anonymous wrapper + 1 empty + 1 use strict + user).
  // Empirically verified: stack line 7 = user line 3 → offset = 4.
  const WRAPPER_LINE_OFFSET = 4

  try {
    // Intentional dynamic evaluation: user-authored decision function code.
    // This is the equivalent of the browser devtools console — same trust level.
    // eslint-disable-next-line no-new-func
    const factory = new Function('ctx', 'config', `
      "use strict";
      ${source}
      if (typeof evaluate !== 'function') {
        throw new Error('Function "evaluate" is not defined');
      }
      return evaluate(ctx, config);
    `)
    const result = factory(context, config)
    if (result === null || result === undefined || typeof result !== 'object') {
      return { error: 'Function must return an object with { fire: boolean }' }
    }
    if (typeof result.fire !== 'boolean') {
      return { error: `Expected "fire" to be boolean, got ${typeof result.fire}` }
    }
    return { fire: result.fire }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    const errorLine = extractErrorLine(err, WRAPPER_LINE_OFFSET)
    return { error: message, errorLine }
  }
}

/**
 * Extract the user-source line number from a JS error's stack trace.
 *
 * V8 stack frames for eval'd code look like:
 *   "at evaluate (eval at <anonymous> (evalmachine:8:21), <anonymous>:LINE:COL)"
 * The inner "<anonymous>:LINE:COL" (after the comma) is the position inside the
 * Function body. Our wrapper template is:
 *   line 1: (newline from template literal)
 *   line 2: "use strict";
 *   line 3: <user source line 1>
 *   line 4: <user source line 2>
 *   ...
 * But new Function('ctx','config', body) wraps the body in `function anonymous(ctx,config){\n...\n}`,
 * adding 1 more line. So total offset from inner <anonymous> line to user line = 3.
 *
 * For SyntaxError (from `new Function` parse), the frame looks like:
 *   "at new Function (<anonymous>)" — no line info in the frame itself,
 *   but the error message sometimes contains position info we can't easily parse.
 */
function extractErrorLine(err: unknown, wrapperOffset: number): number | undefined {
  if (!(err instanceof Error) || !err.stack) return undefined
  // Match the inner <anonymous>:LINE:COL — the one after "), " in an eval frame
  // Pattern: "), <anonymous>:LINE:COL)" for runtime errors inside eval
  const evalMatch = err.stack.match(/\),\s*<anonymous>:(\d+):(\d+)\)/)
  if (evalMatch) {
    const rawLine = parseInt(evalMatch[1], 10)
    const userLine = rawLine - wrapperOffset
    return userLine >= 1 ? userLine : undefined
  }
  return undefined
}

export interface DecisionFunctionModalProps {
  open: boolean
  onClose: () => void
  onSaved: (fn: DecisionFunctionResponse) => void
  /** Pass an ID to edit an existing function (fetched on open), or null/undefined to create. */
  initialId?: string | null
  policyName?: string
}

export function DecisionFunctionModal({
  open,
  onClose,
  onSaved,
  initialId,
  policyName,
}: DecisionFunctionModalProps) {
  const isEdit = !!initialId

  // Form state
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [evaluateContext, setEvaluateContext] = useState<EvaluateContext>('session')
  const [onError, setOnError] = useState<OnErrorBehavior>('deny')
  const [logLevel, setLogLevel] = useState<LogLevel>('off')
  const [isEnabled, setIsEnabled] = useState(true)
  const [code, setCode] = useState('')
  const [configStr, setConfigStr] = useState('{}')
  const [version, setVersion] = useState(0)
  const [policyCount, setPolicyCount] = useState(0)
  const [loadingInitial, setLoadingInitial] = useState(false)

  // Test panel state
  const [testContextStr, setTestContextStr] = useState('')
  const [testResult, setTestResult] = useState<{
    type: 'fire' | 'skip' | 'error'
    message: string
    details?: string
  } | null>(null)
  const [isTesting, setIsTesting] = useState(false)

  // Save state
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)

  const editorRef = useRef<ReactCodeMirrorRef>(null)
  const backdropRef = useRef<HTMLDivElement>(null)

  // Initialize test context when modal opens
  useEffect(() => {
    if (!open) return
    setTestContextStr(
      JSON.stringify(
        {
          session: {
            user: { id: '00000000-0000-0000-0000-000000000000', username: 'testuser', tenant: 'default', roles: ['analyst'] },
            time: { hour: 14, day_of_week: 'Monday' },
            datasource: { name: 'my_ds', access_mode: 'policy_required' },
          },
          ...(evaluateContext === 'query'
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
  }, [open, evaluateContext])

  // Load function from API when modal opens for editing, or reset for create mode
  useEffect(() => {
    if (!open) return
    setTestResult(null)
    setSaveError(null)

    if (initialId) {
      // Edit mode — fetch fresh from server
      setLoadingInitial(true)
      getDecisionFunction(initialId)
        .then((fn) => {
          setName(fn.name)
          setDescription(fn.description ?? '')
          setEvaluateContext(fn.evaluate_context)
          setOnError(fn.on_error)
          setLogLevel(fn.log_level)
          setIsEnabled(fn.is_enabled)
          setCode(fn.decision_fn)
          setConfigStr(fn.decision_config ? JSON.stringify(fn.decision_config, null, 2) : '{}')
          setVersion(fn.version)
          setPolicyCount(fn.policy_count)
        })
        .catch(() => {
          setSaveError('Failed to load decision function')
        })
        .finally(() => setLoadingInitial(false))
    } else {
      // Create mode — reset to defaults
      setName('')
      setDescription('')
      setEvaluateContext('session')
      setOnError('deny')
      setLogLevel('off')
      setIsEnabled(true)
      setCode('')
      setConfigStr('{}')
      setVersion(0)
      setPolicyCount(0)
    }
  }, [open, initialId])

  // Escape key handler
  useEffect(() => {
    if (!open) return
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [open, onClose])

  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === backdropRef.current) onClose()
    },
    [onClose],
  )

  async function handleSave() {
    setSaving(true)
    setSaveError(null)
    try {
      let config: Record<string, unknown> = {}
      try { config = JSON.parse(configStr) } catch { /* keep empty */ }

      if (isEdit && initialId) {
        const updated = await updateDecisionFunction(initialId, {
          name: name || undefined,
          description: description || undefined,
          decision_fn: code,
          decision_config: config,
          evaluate_context: evaluateContext,
          on_error: onError,
          log_level: logLevel,
          is_enabled: isEnabled,
          version,
        })
        setVersion(updated.version)
        onSaved(updated)
      } else {
        const created = await createDecisionFunction({
          name: name || `${policyName || 'policy'} (decision)`,
          description: description || undefined,
          decision_fn: code,
          decision_config: config,
          evaluate_context: evaluateContext,
          on_error: onError,
          log_level: logLevel,
        })
        onSaved(created)
      }
    } catch (err) {
      const apiErr = err as { response?: { status?: number; data?: { error?: string } } }
      if (apiErr?.response?.status === 409) {
        setSaveError('This function was modified by someone else. Reload and try again.')
      } else {
        setSaveError(apiErr?.response?.data?.error ?? 'Failed to save decision function')
      }
    } finally {
      setSaving(false)
    }
  }

  async function handleTest() {
    setIsTesting(true)
    setTestResult(null)
    try {
      const context = JSON.parse(testContextStr)
      let config: Record<string, unknown> = {}
      try { config = JSON.parse(configStr) } catch { /* keep empty */ }

      // Client-side pre-check: parse + run in browser JS engine for instant feedback
      const clientResult = runClientTest(code, context, config)
      if (clientResult.error) {
        setTestResult({ type: 'error', message: `JS error: ${clientResult.error}` })
        return
      }

      // Client test passed — now run authoritative server test (WASM sandbox)
      const response = await testDecisionFn({ decision_fn: code, context, config })
      if (response.error) {
        setTestResult({ type: 'error', message: response.error })
      } else if (response.result?.fire) {
        setTestResult({
          type: 'fire',
          message: 'Policy will fire',
          details: JSON.stringify(response, null, 2),
        })
      } else {
        setTestResult({
          type: 'skip',
          message: 'Policy will be skipped',
          details: JSON.stringify(response, null, 2),
        })
      }
    } catch (err) {
      setTestResult({ type: 'error', message: err instanceof Error ? err.message : String(err) })
    } finally {
      setIsTesting(false)
    }
  }

  function handleTemplateSelect(e: React.ChangeEvent<HTMLSelectElement>) {
    const idx = parseInt(e.target.value, 10)
    if (isNaN(idx) || idx < 0) return
    const tpl = TEMPLATES[idx]
    setCode(tpl.code)
    setConfigStr(tpl.config)
    e.target.value = '' // reset dropdown
  }

  // Ref for linter to read latest test context + config without re-creating the extension
  const lintContextRef = useRef({ testContextStr, configStr })
  lintContextRef.current = { testContextStr, configStr }

  const jsLinter = useMemo(() => createDecisionFnLinter(() => {
    let context: Record<string, unknown> = {}
    let config: Record<string, unknown> = {}
    try { context = JSON.parse(lintContextRef.current.testContextStr) } catch { /* ignore */ }
    try { config = JSON.parse(lintContextRef.current.configStr) } catch { /* ignore */ }
    return { context, config }
  }), []) // eslint-disable-line react-hooks/exhaustive-deps

  if (!open) return null

  const completionItems = buildCtxCompletions(evaluateContext, configStr)

  return (
    <div
      ref={backdropRef}
      onClick={handleBackdropClick}
      className="fixed inset-0 bg-black/50 z-50 flex items-center justify-center p-4"
      aria-modal="true"
      role="dialog"
    >
      <div className="bg-white rounded-xl max-w-3xl w-full max-h-[90vh] overflow-y-auto shadow-xl">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200">
          <h2 className="text-lg font-semibold text-gray-900">
            {isEdit ? (name || 'Decision Function') : 'New Decision Function'}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600 text-xl leading-none"
            aria-label="Close"
          >
            &times;
          </button>
        </div>

        <div className="px-6 py-5 space-y-5">
          {loadingInitial && (
            <div className="flex items-center gap-2 text-sm text-gray-400 py-4">
              <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24" fill="none">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              Loading function…
            </div>
          )}

          {/* Shared function warning */}
          {!loadingInitial && isEdit && policyCount > 1 && (
            <div className="bg-amber-50 border border-amber-200 text-amber-800 text-sm rounded-lg px-4 py-3">
              This function is used by {policyCount} policies. Changes will affect all of them.
            </div>
          )}

          {/* Identity */}
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Name</label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={`${policyName || 'policy'} (decision)`}
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Description</label>
              <input
                type="text"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Brief description"
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
              />
            </div>
          </div>

          {/* Behavior */}
          <div className="grid grid-cols-3 gap-4">
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Evaluate Context</label>
              <select
                value={evaluateContext}
                onChange={(e) => setEvaluateContext(e.target.value as EvaluateContext)}
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
              >
                <option value="session">Session</option>
                <option value="query">Query</option>
              </select>
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">On Error</label>
              <select
                value={onError}
                onChange={(e) => setOnError(e.target.value as OnErrorBehavior)}
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
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
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
              >
                <option value="off">Off</option>
                <option value="error">Error only</option>
                <option value="info">Info</option>
              </select>
            </div>
          </div>

          {/* Enabled toggle */}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => setIsEnabled((v) => !v)}
              className={`relative inline-flex h-4 w-7 items-center rounded-full transition-colors focus:outline-none ${
                isEnabled ? 'bg-blue-600' : 'bg-gray-300'
              }`}
            >
              <span
                className={`inline-block h-3 w-3 transform rounded-full bg-white transition-transform ${
                  isEnabled ? 'translate-x-3.5' : 'translate-x-0.5'
                }`}
              />
            </button>
            <span className="text-xs text-gray-600">
              {isEnabled ? 'Enabled' : 'Disabled'}
            </span>
          </div>

          {/* Templates (create mode only) */}
          {!isEdit && (
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Start from template</label>
              <select
                onChange={handleTemplateSelect}
                defaultValue=""
                className="w-full border border-gray-300 rounded px-2 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
              >
                <option value="" disabled>Choose a template…</option>
                {TEMPLATES.map((t, i) => (
                  <option key={t.label} value={i}>{t.label}</option>
                ))}
              </select>
            </div>
          )}

          {/* Code Editor */}
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">Function Source</label>
            <CodeMirror
              ref={editorRef}
              value={code}
              onChange={setCode}
              height="280px"
              theme="dark"
              extensions={[
                javascript(),
                autocompletion({ override: [ctxCompletionSource(completionItems)] }),
                jsLinter,
                lintGutter(),
              ]}
              placeholder={`function evaluate(ctx, config) {\n  return { fire: true };\n}`}
              basicSetup={{
                lineNumbers: true,
                bracketMatching: true,
                foldGutter: false,
                autocompletion: false, // disabled — we provide our own with ctx.*/config.* completions in extensions
              }}
            />
          </div>

          {/* Config JSON */}
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">Config JSON</label>
            <CodeMirror
              value={configStr}
              onChange={setConfigStr}
              height="80px"
              extensions={[json(), jsonLinter, lintGutter()]}
              basicSetup={{
                lineNumbers: true,
                bracketMatching: true,
                foldGutter: false,
              }}
            />
            <p className="text-xs text-gray-400 mt-1">
              Parameters passed as the second argument. Discoverable via <code className="bg-gray-100 px-1 rounded">config.*</code> autocomplete in the editor above.
            </p>
          </div>

          {/* Test Panel */}
          <div className="border border-gray-200 rounded-lg p-4 space-y-3">
            <h4 className="text-xs font-semibold text-gray-700">Test</h4>
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Mock Context</label>
              <CodeMirror
                value={testContextStr}
                onChange={setTestContextStr}
                height="140px"
                extensions={[json(), jsonLinter, lintGutter()]}
                basicSetup={{
                  lineNumbers: true,
                  bracketMatching: true,
                  foldGutter: false,
                }}
              />
            </div>
            <button
              type="button"
              onClick={handleTest}
              disabled={isTesting || !code.trim()}
              className="bg-green-600 hover:bg-green-700 text-white text-xs font-medium rounded px-3 py-1.5 transition-colors disabled:opacity-50"
            >
              {isTesting ? 'Running…' : 'Run Test'}
            </button>
            {testResult && (
              <div>
                {testResult.type === 'fire' && (
                  <div className="flex items-center gap-2 mb-2">
                    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-green-100 text-green-800">
                      <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                      </svg>
                      Fire: Yes
                    </span>
                    <span className="text-xs text-gray-600">{testResult.message}</span>
                  </div>
                )}
                {testResult.type === 'skip' && (
                  <div className="flex items-center gap-2 mb-2">
                    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-amber-100 text-amber-800">
                      <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                      </svg>
                      Fire: No
                    </span>
                    <span className="text-xs text-gray-600">{testResult.message}</span>
                  </div>
                )}
                {testResult.type === 'error' && (
                  <div className="flex items-center gap-2 mb-2">
                    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-red-100 text-red-800">
                      Error
                    </span>
                    <span className="text-xs text-red-600">{testResult.message}</span>
                  </div>
                )}
                {testResult.details && (
                  <details className="text-xs">
                    <summary className="text-gray-500 cursor-pointer hover:text-gray-700">Full response</summary>
                    <pre className="bg-gray-900 text-green-300 rounded p-3 font-mono overflow-x-auto whitespace-pre-wrap max-h-48 overflow-y-auto mt-1">
                      {testResult.details}
                    </pre>
                  </details>
                )}
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 border-t border-gray-200 bg-gray-50 rounded-b-xl">
          <div>
            {saveError && <span className="text-xs text-red-600">{saveError}</span>}
          </div>
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={onClose}
              className="text-sm text-gray-600 hover:text-gray-800 px-3 py-1.5"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={handleSave}
              disabled={saving || !code.trim()}
              className="bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg px-4 py-1.5 transition-colors disabled:opacity-50"
            >
              {saving ? 'Saving…' : isEdit ? 'Update Function' : 'Create Function'}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
