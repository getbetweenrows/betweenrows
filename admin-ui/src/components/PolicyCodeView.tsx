import { useState, useMemo } from 'react'
import type { PolicyResponse, PolicyAssignmentResponse } from '../types/policy'

interface PolicyCodeViewProps {
  policy: PolicyResponse
  assignments: PolicyAssignmentResponse[]
}

function buildCodeObject(policy: PolicyResponse, assignments: PolicyAssignmentResponse[]) {
  return {
    id: policy.id,
    name: policy.name,
    policy_type: policy.policy_type,
    description: policy.description ?? null,
    is_enabled: policy.is_enabled,
    version: policy.version,
    targets: policy.targets,
    definition: policy.definition ?? null,
    assignments: assignments.map((a) => ({
      id: a.id,
      datasource_id: a.data_source_id,
      datasource: a.datasource_name,
      user_id: a.user_id ?? null,
      user: a.username ?? 'all',
      priority: a.priority,
    })),
  }
}

// Simple YAML serializer — always quotes strings for safety
function toYaml(val: unknown, indent = 0): string {
  const sp = '  '.repeat(indent)
  if (val === null || val === undefined) return 'null'
  if (typeof val === 'boolean') return String(val)
  if (typeof val === 'number') return String(val)
  if (typeof val === 'string') return JSON.stringify(val)
  if (Array.isArray(val)) {
    if (val.length === 0) return '[]'
    return val
      .map((item) => {
        if (typeof item === 'object' && item !== null && !Array.isArray(item)) {
          const lines = toYaml(item, indent + 1).split('\n')
          const [first, ...rest] = lines
          return `${sp}- ${first.trimStart()}${rest.length ? '\n' + rest.join('\n') : ''}`
        }
        return `${sp}- ${toYaml(item, indent)}`
      })
      .join('\n')
  }
  const obj = val as Record<string, unknown>
  const keys = Object.keys(obj)
  if (keys.length === 0) return '{}'
  return keys
    .map((k) => {
      const v = obj[k]
      if (typeof v === 'object' && v !== null) {
        if (Array.isArray(v)) {
          if (v.length === 0) return `${sp}${k}: []`
          return `${sp}${k}:\n${toYaml(v, indent + 1)}`
        }
        return `${sp}${k}:\n${toYaml(v, indent + 1)}`
      }
      return `${sp}${k}: ${toYaml(v, indent)}`
    })
    .join('\n')
}

export function PolicyCodeView({ policy, assignments }: PolicyCodeViewProps) {
  const [format, setFormat] = useState<'yaml' | 'json'>('yaml')
  const [copied, setCopied] = useState(false)

  const code = useMemo(() => buildCodeObject(policy, assignments), [policy, assignments])
  const text = useMemo(
    () => (format === 'json' ? JSON.stringify(code, null, 2) : toYaml(code)),
    [code, format],
  )

  function handleCopy() {
    navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div>
      <div className="flex items-center gap-2 mb-3">
        <h2 className="text-base font-semibold text-gray-900">View as code</h2>
        <div className="ml-auto flex items-center gap-2">
          <div className="flex rounded border border-gray-300 overflow-hidden text-xs font-medium">
            <button
              type="button"
              onClick={() => setFormat('yaml')}
              className={`px-3 py-1 transition-colors ${format === 'yaml' ? 'bg-gray-900 text-white' : 'bg-white text-gray-600 hover:bg-gray-50'}`}
            >
              YAML
            </button>
            <button
              type="button"
              onClick={() => setFormat('json')}
              className={`px-3 py-1 border-l border-gray-300 transition-colors ${format === 'json' ? 'bg-gray-900 text-white' : 'bg-white text-gray-600 hover:bg-gray-50'}`}
            >
              JSON
            </button>
          </div>
          <button
            type="button"
            onClick={handleCopy}
            className="text-xs text-gray-500 hover:text-gray-800 border border-gray-300 rounded px-2 py-1 transition-colors"
          >
            {copied ? 'Copied!' : 'Copy'}
          </button>
        </div>
      </div>
      <pre className="text-xs bg-gray-50 border border-gray-200 rounded-lg p-4 overflow-auto max-h-[32rem] font-mono leading-relaxed text-gray-800 whitespace-pre">
        {text}
      </pre>
    </div>
  )
}
