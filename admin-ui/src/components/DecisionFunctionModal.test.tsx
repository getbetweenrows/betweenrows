import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, fireEvent, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { DecisionFunctionModal, runClientTest } from './DecisionFunctionModal'
import { renderWithProviders } from '../test/test-utils'
import { makeDecisionFunction } from '../test/factories'

// Mock CodeMirror as a simple textarea (jsdom doesn't support Selection/Range)
vi.mock('@uiw/react-codemirror', () => ({
  default: ({ value, onChange, placeholder }: { value: string; onChange?: (v: string) => void; placeholder?: string }) => (
    <textarea
      data-testid="codemirror"
      value={value}
      onChange={(e) => onChange?.(e.target.value)}
      placeholder={placeholder}
    />
  ),
}))

const mockCreateDecisionFunction = vi.fn()
const mockUpdateDecisionFunction = vi.fn()
const mockTestDecisionFn = vi.fn()
const mockGetDecisionFunction = vi.fn()

vi.mock('../api/decisionFunctions', () => ({
  createDecisionFunction: (...args: unknown[]) => mockCreateDecisionFunction(...args),
  updateDecisionFunction: (...args: unknown[]) => mockUpdateDecisionFunction(...args),
  testDecisionFn: (...args: unknown[]) => mockTestDecisionFn(...args),
  listDecisionFunctions: vi.fn().mockResolvedValue([]),
  getDecisionFunction: (...args: unknown[]) => mockGetDecisionFunction(...args),
}))

function renderModal(props: Partial<Parameters<typeof DecisionFunctionModal>[0]> = {}) {
  return renderWithProviders(
    <DecisionFunctionModal
      open={true}
      onClose={vi.fn()}
      onSaved={vi.fn()}
      {...props}
    />,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
})

function renderEditModal(fn: ReturnType<typeof makeDecisionFunction>, props: Partial<Parameters<typeof DecisionFunctionModal>[0]> = {}) {
  mockGetDecisionFunction.mockResolvedValue(fn)
  return renderModal({ initialId: fn.id, ...props })
}

describe('DecisionFunctionModal — happy path', () => {
  it('renders in create mode with empty fields and "Create Function" button', () => {
    renderModal()
    expect(screen.getByText('New Decision Function')).toBeTruthy()
    expect(screen.getByText('Create Function')).toBeTruthy()
  })

  it('renders in edit mode with pre-populated fields and "Update Function" button', async () => {
    const fn = makeDecisionFunction({ name: 'my-func', description: 'my desc' })
    renderEditModal(fn)
    expect(screen.getByText('Update Function')).toBeTruthy()
    await waitFor(() => {
      expect(screen.getByDisplayValue('my-func')).toBeTruthy()
      expect(screen.getByDisplayValue('my desc')).toBeTruthy()
    })
  })

  it('saves new function via createDecisionFunction API on "Create Function" click', async () => {
    const created = makeDecisionFunction({ id: 'new-id', name: 'auto-name' })
    mockCreateDecisionFunction.mockResolvedValue(created)
    const onSaved = vi.fn()
    renderModal({ onSaved })

    const editors = screen.getAllByTestId('codemirror')
    const codeEditor = editors[0]
    fireEvent.change(codeEditor, { target: { value: 'function evaluate(ctx) { return { fire: true }; }' } })

    await userEvent.click(screen.getByText('Create Function'))

    await waitFor(() => {
      expect(mockCreateDecisionFunction).toHaveBeenCalledTimes(1)
    })
    await waitFor(() => {
      expect(onSaved).toHaveBeenCalledWith(created)
    })
  })

  it('saves existing function via updateDecisionFunction API on "Update Function" click', async () => {
    const fn = makeDecisionFunction({ id: 'fn-1', name: 'existing', version: 3 })
    const updated = { ...fn, version: 4 }
    mockUpdateDecisionFunction.mockResolvedValue(updated)
    const onSaved = vi.fn()
    renderEditModal(fn, { onSaved })

    // Wait for fetch to populate the form
    await waitFor(() => expect(screen.getByDisplayValue('existing')).toBeTruthy())

    await userEvent.click(screen.getByText('Update Function'))

    await waitFor(() => {
      expect(mockUpdateDecisionFunction).toHaveBeenCalledTimes(1)
      expect(mockUpdateDecisionFunction.mock.calls[0][0]).toBe('fn-1')
      expect(mockUpdateDecisionFunction.mock.calls[0][1].version).toBe(3)
    })
    await waitFor(() => {
      expect(onSaved).toHaveBeenCalledWith(updated)
    })
  })

  it('calls onClose on Cancel click', async () => {
    const onClose = vi.fn()
    renderModal({ onClose })
    await userEvent.click(screen.getByText('Cancel'))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('calls onClose on backdrop click', async () => {
    const onClose = vi.fn()
    renderModal({ onClose })
    const backdrop = document.querySelector('[aria-modal="true"]')!
    fireEvent.click(backdrop)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('calls onClose on Escape key', async () => {
    const onClose = vi.fn()
    renderModal({ onClose })
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('does not render when open is false', () => {
    renderModal({ open: false })
    expect(screen.queryByText('New Decision Function')).toBeNull()
  })
})

describe('DecisionFunctionModal — shared function warning', () => {
  it('shows amber warning when policy_count > 1', async () => {
    const fn = makeDecisionFunction({ policy_count: 3 })
    renderEditModal(fn)
    await waitFor(() => {
      expect(screen.getByText(/used by 3 policies/)).toBeTruthy()
    })
  })

  it('hides warning when policy_count <= 1', async () => {
    const fn = makeDecisionFunction({ policy_count: 1 })
    renderEditModal(fn)
    await waitFor(() => expect(screen.getByDisplayValue(fn.name)).toBeTruthy())
    expect(screen.queryByText(/used by/)).toBeNull()
  })

  it('hides warning in create mode', () => {
    renderModal()
    expect(screen.queryByText(/used by/)).toBeNull()
  })
})

describe('DecisionFunctionModal — templates (create mode only)', () => {
  it('shows template dropdown in create mode', () => {
    renderModal()
    expect(screen.getByText('Start from template')).toBeTruthy()
    expect(screen.getByText('Choose a template…')).toBeTruthy()
  })

  it('hides template dropdown in edit mode', () => {
    const fn = makeDecisionFunction()
    renderEditModal(fn)
    expect(screen.queryByText('Start from template')).toBeNull()
  })

  it('selecting "Business hours only" template fills editor', async () => {
    renderModal()
    const templateSelect = screen.getByText('Choose a template…').closest('select')!
    await userEvent.selectOptions(templateSelect, '0')
    const editors = screen.getAllByTestId('codemirror')
    const codeEditor = editors[0] as HTMLTextAreaElement
    expect(codeEditor.value).toContain('ctx.session.time.hour')
  })

  it('selecting "Role-based" template fills editor and config', async () => {
    renderModal()
    const templateSelect = screen.getByText('Choose a template…').closest('select')!
    await userEvent.selectOptions(templateSelect, '1')
    const editors = screen.getAllByTestId('codemirror')
    const codeEditor = editors[0] as HTMLTextAreaElement
    const configEditor = editors[1] as HTMLTextAreaElement
    expect(codeEditor.value).toContain('roles.includes')
    expect(configEditor.value).toContain('required_role')
  })
})

const VALID_FN_TRUE = 'function evaluate(ctx) { return { fire: true }; }'
const VALID_FN_FALSE = 'function evaluate(ctx) { return { fire: false }; }'

describe('DecisionFunctionModal — test panel', () => {
  it('calls testDecisionFn API with function source and mock context', async () => {
    mockTestDecisionFn.mockResolvedValue({
      success: true,
      result: { fire: true, logs: [], fuel_consumed: 10, time_us: 100, error: null },
    })
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    fireEvent.change(editors[0], { target: { value: VALID_FN_TRUE } })

    await userEvent.click(screen.getByText('Run Test'))

    await waitFor(() => {
      expect(mockTestDecisionFn).toHaveBeenCalledTimes(1)
    })
  })

  it('shows green "Fire: Yes" badge when result is { fire: true }', async () => {
    mockTestDecisionFn.mockResolvedValue({
      success: true,
      result: { fire: true, logs: [], fuel_consumed: 10, time_us: 100, error: null },
    })
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    fireEvent.change(editors[0], { target: { value: VALID_FN_TRUE } })
    await userEvent.click(screen.getByText('Run Test'))

    await waitFor(() => {
      expect(screen.getByText('Fire: Yes')).toBeTruthy()
      expect(screen.getByText('Policy will fire')).toBeTruthy()
    })
  })

  it('shows amber "Fire: No" badge when result is { fire: false }', async () => {
    mockTestDecisionFn.mockResolvedValue({
      success: true,
      result: { fire: false, logs: [], fuel_consumed: 10, time_us: 100, error: null },
    })
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    fireEvent.change(editors[0], { target: { value: VALID_FN_FALSE } })
    await userEvent.click(screen.getByText('Run Test'))

    await waitFor(() => {
      expect(screen.getByText('Fire: No')).toBeTruthy()
      expect(screen.getByText('Policy will be skipped')).toBeTruthy()
    })
  })

  it('shows red error badge when test API call fails (client passes, server fails)', async () => {
    mockTestDecisionFn.mockRejectedValue(new Error('Sandbox timeout'))
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    fireEvent.change(editors[0], { target: { value: VALID_FN_TRUE } })
    await userEvent.click(screen.getByText('Run Test'))

    await waitFor(() => {
      expect(screen.getByText('Error')).toBeTruthy()
      expect(screen.getByText('Sandbox timeout')).toBeTruthy()
    })
  })

  it('catches syntax errors client-side without calling server', async () => {
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    fireEvent.change(editors[0], { target: { value: 'function evaluate(ctx { return { fire: true }; }' } })
    await userEvent.click(screen.getByText('Run Test'))

    await waitFor(() => {
      expect(screen.getByText('Error')).toBeTruthy()
      expect(screen.getByText(/JS error:/)).toBeTruthy()
    })
    expect(mockTestDecisionFn).not.toHaveBeenCalled()
  })

  it('catches missing evaluate function client-side', async () => {
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    fireEvent.change(editors[0], { target: { value: 'function notEvaluate() { return { fire: true }; }' } })
    await userEvent.click(screen.getByText('Run Test'))

    await waitFor(() => {
      expect(screen.getByText(/evaluate.*not defined/i)).toBeTruthy()
    })
    expect(mockTestDecisionFn).not.toHaveBeenCalled()
  })

  it('catches non-boolean fire return client-side', async () => {
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    fireEvent.change(editors[0], { target: { value: 'function evaluate() { return { fire: 1 }; }' } })
    await userEvent.click(screen.getByText('Run Test'))

    await waitFor(() => {
      expect(screen.getByText(/Expected "fire" to be boolean/)).toBeTruthy()
    })
    expect(mockTestDecisionFn).not.toHaveBeenCalled()
  })

  it('disables "Run Test" button when function source is empty', () => {
    renderModal()
    const runBtn = screen.getByText('Run Test')
    expect(runBtn).toBeDisabled()
  })
})

describe('DecisionFunctionModal — error handling', () => {
  it('shows error message when save fails (network error)', async () => {
    mockCreateDecisionFunction.mockRejectedValue(new Error('Network error'))
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    await userEvent.type(editors[0], 'fn code')
    await userEvent.click(screen.getByText('Create Function'))

    await waitFor(() => {
      expect(screen.getByText('Failed to save decision function')).toBeTruthy()
    })
  })

  it('shows conflict message on 409 response', async () => {
    mockUpdateDecisionFunction.mockRejectedValue({
      response: { status: 409, data: { error: 'version conflict' } },
    })
    const fn = makeDecisionFunction()
    renderEditModal(fn)

    await waitFor(() => expect(screen.getByDisplayValue(fn.name)).toBeTruthy())
    await userEvent.click(screen.getByText('Update Function'))

    await waitFor(() => {
      expect(screen.getByText(/modified by someone else/)).toBeTruthy()
    })
  })

  it('disables Save button while saving', async () => {
    mockCreateDecisionFunction.mockImplementation(() => new Promise(() => {})) // never resolves
    renderModal()

    const editors = screen.getAllByTestId('codemirror')
    fireEvent.change(editors[0], { target: { value: VALID_FN_TRUE } })
    await userEvent.click(screen.getByText('Create Function'))

    await waitFor(() => {
      expect(screen.getByText('Saving…')).toBeDisabled()
    })
  })
})

describe('runClientTest — unit tests', () => {
  const ctx = { session: { user: { roles: ['analyst'] } } }

  it('returns fire: true for a valid function that fires', () => {
    const result = runClientTest('function evaluate() { return { fire: true }; }', ctx, {})
    expect(result).toEqual({ fire: true })
  })

  it('returns fire: false for a valid function that skips', () => {
    const result = runClientTest('function evaluate() { return { fire: false }; }', ctx, {})
    expect(result).toEqual({ fire: false })
  })

  it('passes ctx and config to the function', () => {
    const code = 'function evaluate(ctx, config) { return { fire: ctx.session.user.roles.includes(config.role) }; }'
    expect(runClientTest(code, ctx, { role: 'analyst' })).toEqual({ fire: true })
    expect(runClientTest(code, ctx, { role: 'admin' })).toEqual({ fire: false })
  })

  it('returns error for syntax errors', () => {
    const result = runClientTest('function evaluate(ctx { return { fire: true }; }', ctx, {})
    expect(result.error).toBeTruthy()
    expect(result.fire).toBeUndefined()
  })

  it('returns error when evaluate is not defined', () => {
    const result = runClientTest('function notEvaluate() { return { fire: true }; }', ctx, {})
    expect(result.error).toMatch(/evaluate.*not defined/i)
  })

  it('returns error when fire is not boolean', () => {
    const result = runClientTest('function evaluate() { return { fire: 1 }; }', ctx, {})
    expect(result.error).toMatch(/Expected "fire" to be boolean/)
  })

  it('returns error when function returns null', () => {
    const result = runClientTest('function evaluate() { return null; }', ctx, {})
    expect(result.error).toMatch(/must return an object/)
  })

  it('returns error when function throws at runtime', () => {
    const result = runClientTest('function evaluate(ctx) { return { fire: ctx.missing.deep.path }; }', ctx, {})
    expect(result.error).toBeTruthy()
  })
})
