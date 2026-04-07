import { useState, useCallback, useMemo } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { autocompletion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete'
import { EditorView } from '@codemirror/view'

export interface TemplateItem {
  key: string
  valueType: string
}

interface ExpressionEditorProps {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  templateItems: TemplateItem[]
  onValidate?: (expression: string) => Promise<{ valid: boolean; error?: string }>
}

const BUILTIN_VARS = [
  { label: '{user.username}', detail: 'string', apply: '{user.username}' },
  { label: '{user.id}', detail: 'string', apply: '{user.id}' },
]

function templateCompletionSource(items: TemplateItem[]) {
  return (context: CompletionContext): CompletionResult | null => {
    // Match {user. followed by optional word chars and optional closing brace
    const match = context.matchBefore(/\{user\.[\w]*\}?/)
    if (!match) return null

    const custom = items.map((a) => ({
      label: `{user.${a.key}}`,
      detail: a.valueType === 'list' ? 'list (use with IN)' : a.valueType,
      apply: `{user.${a.key}}`,
    }))

    return {
      from: match.from,
      options: [...BUILTIN_VARS, ...custom],
      filter: true,
    }
  }
}

const compactTheme = EditorView.theme({
  '&': { fontSize: '14px' },
  '.cm-editor': { maxHeight: '120px', overflow: 'auto' },
  '.cm-content': { padding: '6px 10px', fontFamily: 'ui-monospace, monospace' },
  '.cm-line': { lineHeight: '1.5' },
  '.cm-placeholder': { color: '#9ca3af' },
  '.cm-focused': { outline: 'none' },
})

export function ExpressionEditor({
  value,
  onChange,
  placeholder,
  templateItems,
  onValidate,
}: ExpressionEditorProps) {
  const [validationState, setValidationState] = useState<
    'idle' | 'loading' | 'valid' | 'invalid'
  >('idle')
  const [validationError, setValidationError] = useState<string>('')

  const handleChange = useCallback(
    (val: string) => {
      onChange(val)
      // Reset validation state when expression changes
      if (validationState !== 'idle') {
        setValidationState('idle')
        setValidationError('')
      }
    },
    [onChange, validationState],
  )

  const handleValidate = useCallback(async () => {
    if (!onValidate || !value.trim()) return
    setValidationState('loading')
    try {
      const result = await onValidate(value)
      if (result.valid) {
        setValidationState('valid')
        setValidationError('')
      } else {
        setValidationState('invalid')
        setValidationError(result.error ?? 'Invalid expression')
      }
    } catch {
      setValidationState('invalid')
      setValidationError('Validation request failed')
    }
  }, [onValidate, value])

  const extensions = useMemo(
    () => [
      autocompletion({ override: [templateCompletionSource(templateItems)] }),
      compactTheme,
    ],
    [templateItems],
  )

  return (
    <div>
      <div className="flex items-center gap-2">
        <div
          className={`flex-1 border rounded-lg overflow-hidden ${
            validationState === 'invalid'
              ? 'border-red-400'
              : validationState === 'valid'
                ? 'border-green-400'
                : 'border-gray-300 focus-within:ring-2 focus-within:ring-blue-500 focus-within:border-blue-500'
          }`}
        >
          <CodeMirror
            value={value}
            onChange={handleChange}
            placeholder={placeholder}
            basicSetup={{
              lineNumbers: false,
              foldGutter: false,
              highlightActiveLine: false,
              autocompletion: false,
            }}
            extensions={extensions}
          />
        </div>
        {onValidate && (
          <button
            type="button"
            onClick={handleValidate}
            disabled={validationState === 'loading' || !value.trim()}
            className="shrink-0 px-2 py-1.5 text-xs font-medium rounded-md border border-gray-300 bg-white text-gray-600 hover:bg-gray-50 disabled:opacity-40 disabled:cursor-not-allowed"
            title="Validate expression syntax"
          >
            {validationState === 'loading' ? (
              <span className="inline-block w-4 h-4 border-2 border-gray-300 border-t-gray-600 rounded-full animate-spin" />
            ) : validationState === 'valid' ? (
              <span className="text-green-600">&#10003;</span>
            ) : validationState === 'invalid' ? (
              <span className="text-red-600">&#10007;</span>
            ) : (
              'Check'
            )}
          </button>
        )}
      </div>
      {validationState === 'invalid' && validationError && (
        <p className="text-xs text-red-500 mt-1">{validationError}</p>
      )}
    </div>
  )
}
