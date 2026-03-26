export type EvaluateContext = 'session' | 'query'
export type OnErrorBehavior = 'deny' | 'skip'
export type LogLevel = 'off' | 'error' | 'info'

export interface DecisionResult {
  fire: boolean
  logs: string[]
  fuel_consumed: number
  time_us: number
  error: string | null
}

export interface DecisionFunctionResponse {
  id: string
  name: string
  description: string | null
  language: string
  decision_fn: string
  decision_config: Record<string, unknown> | null
  evaluate_context: EvaluateContext
  on_error: OnErrorBehavior
  log_level: LogLevel
  is_enabled: boolean
  version: number
  policy_count: number
  created_by: string
  updated_by: string
  created_at: string
  updated_at: string
}

export interface DecisionFunctionSummary {
  id: string
  name: string
  is_enabled: boolean
  evaluate_context: EvaluateContext
}

export interface CreateDecisionFunctionPayload {
  name: string
  description?: string
  language?: string
  decision_fn: string
  decision_config?: Record<string, unknown>
  evaluate_context: EvaluateContext
  on_error?: OnErrorBehavior
  log_level?: LogLevel
}

export interface UpdateDecisionFunctionPayload {
  name?: string
  description?: string
  language?: string
  decision_fn?: string
  decision_config?: Record<string, unknown> | null
  evaluate_context?: EvaluateContext
  on_error?: OnErrorBehavior
  log_level?: LogLevel
  is_enabled?: boolean
  version: number
}

export interface TestDecisionFnPayload {
  decision_fn: string
  context: Record<string, unknown>
  config: Record<string, unknown>
}

export interface TestDecisionFnResponse {
  success: boolean
  result?: DecisionResult
  error?: string
}
