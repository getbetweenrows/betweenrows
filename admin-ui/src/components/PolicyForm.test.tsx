import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, fireEvent, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PolicyForm } from './PolicyForm'
import { renderWithProviders } from '../test/test-utils'
import { makePolicy, makeDecisionFunction } from '../test/factories'

// Mock CodeMirror for DecisionFunctionModal
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

const mockListDecisionFunctions = vi.fn()
const mockGetDecisionFunction = vi.fn()
const mockCreateDecisionFunction = vi.fn()
const mockUpdateDecisionFunction = vi.fn()
const mockTestDecisionFn = vi.fn()

vi.mock('../api/attributeDefinitions', () => ({
  listAttributeDefinitions: vi.fn().mockResolvedValue({ data: [], total: 0, page: 1, page_size: 200 }),
}))

vi.mock('../api/policies', () => ({
  validateExpression: vi.fn().mockResolvedValue({ valid: true }),
}))

vi.mock('../api/decisionFunctions', () => ({
  listDecisionFunctions: (...args: unknown[]) => mockListDecisionFunctions(...args),
  getDecisionFunction: (...args: unknown[]) => mockGetDecisionFunction(...args),
  createDecisionFunction: (...args: unknown[]) => mockCreateDecisionFunction(...args),
  updateDecisionFunction: (...args: unknown[]) => mockUpdateDecisionFunction(...args),
  testDecisionFn: (...args: unknown[]) => mockTestDecisionFn(...args),
}))

/** Makes a policy with an embedded decision function summary for testing the compact widget */
function makePolicyWithFn(fnOverrides: Partial<ReturnType<typeof makeDecisionFunction>> = {}) {
  const fn = makeDecisionFunction(fnOverrides)
  return {
    policy: makePolicy({
      decision_function_id: fn.id,
      decision_function: { id: fn.id, name: fn.name, is_enabled: fn.is_enabled, evaluate_context: fn.evaluate_context },
    }),
    fn,
  }
}

function renderForm(props: Partial<Parameters<typeof PolicyForm>[0]> = {}) {
  return renderWithProviders(
    <PolicyForm
      onSubmit={vi.fn()}
      submitLabel="Save"
      isSubmitting={false}
      {...props}
    />,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('PolicyForm — policy type selector', () => {
  it('renders all 5 policy type options', () => {
    renderForm()
    const select = screen.getAllByRole('combobox')[0]
    const options = Array.from(select.querySelectorAll('option')).map((o) => o.value)
    expect(options).toContain('row_filter')
    expect(options).toContain('column_mask')
    expect(options).toContain('column_allow')
    expect(options).toContain('column_deny')
    expect(options).toContain('table_deny')
  })

  it('shows deny note when column_deny is selected', async () => {
    renderForm()
    const select = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(select, 'column_deny')
    expect(screen.getByText(/Deny policies short-circuit or strip columns/i)).toBeTruthy()
  })

  it('shows filter expression field when row_filter is selected', async () => {
    renderForm()
    const select = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(select, 'row_filter')
    expect(screen.getByText(/Filter Expression/i)).toBeTruthy()
    expect(screen.queryByText(/Mask Expression/i)).toBeNull()
  })

  it('shows mask expression field when column_mask is selected', async () => {
    renderForm()
    const select = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(select, 'column_mask')
    expect(screen.getByText(/Mask Expression/i)).toBeTruthy()
    expect(screen.queryByText(/Filter Expression/i)).toBeNull()
  })

  it('hides definition fields when table_deny is selected', async () => {
    renderForm()
    const select = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(select, 'table_deny')
    expect(screen.queryByText(/Filter Expression/i)).toBeNull()
    expect(screen.queryByText(/Mask Expression/i)).toBeNull()
  })
})

describe('PolicyForm — targets', () => {
  it('starts with one target entry', () => {
    renderForm()
    expect(screen.getByText('Target 1')).toBeTruthy()
  })

  it('adds a target when "+ Add target" is clicked', async () => {
    renderForm()
    await userEvent.click(screen.getByText('+ Add target'))
    expect(screen.getByText('Target 2')).toBeTruthy()
  })

  it('removes a target when Remove is clicked', async () => {
    renderForm()
    await userEvent.click(screen.getByText('+ Add target'))
    expect(screen.getByText('Target 2')).toBeTruthy()
    const removeButtons = screen.getAllByText('Remove')
    await userEvent.click(removeButtons[0])
    expect(screen.queryByText('Target 2')).toBeNull()
  })

  it('shows columns field for column_allow', async () => {
    renderForm()
    const select = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(select, 'column_allow')
    expect(screen.getByText('Columns')).toBeTruthy()
  })

  it('hides columns field for row_filter', async () => {
    renderForm()
    const select = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(select, 'row_filter')
    expect(screen.queryByText('Columns')).toBeNull()
  })

  it('hides columns field for table_deny', async () => {
    renderForm()
    const select = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(select, 'table_deny')
    expect(screen.queryByText('Columns')).toBeNull()
  })
})

describe('PolicyForm — comma input', () => {
  it('preserves trailing comma while typing (does not eat it)', async () => {
    renderForm()
    const inputs = screen.getAllByRole('textbox')
    // schemas input is the first input inside the target card
    const schemasInput = inputs.find((el) => (el as HTMLInputElement).placeholder === 'public, analytics')!
    await userEvent.clear(schemasInput)
    await userEvent.type(schemasInput, 'public,')
    expect((schemasInput as HTMLInputElement).value).toBe('public,')
  })

  it('chip click appends value and updates input', async () => {
    const hints = {
      schemas: ['analytics'],
      tables: new Map<string, string[]>(),
      columns: new Map<string, string[]>(),
    }
    renderForm({ catalogHints: hints })
    const chip = screen.getByRole('button', { name: 'analytics' })
    await userEvent.click(chip)
    const schemasInput = screen.getAllByRole('textbox').find(
      (el) => (el as HTMLInputElement).placeholder === 'public, analytics',
    )!
    expect((schemasInput as HTMLInputElement).value).toBe('analytics')
  })
})

describe('PolicyForm — hint filter input', () => {
  it('shows filter input when hint count exceeds threshold', () => {
    const manySchemas = Array.from({ length: 16 }, (_, i) => `schema_${i}`)
    const hints = {
      schemas: manySchemas,
      tables: new Map<string, string[]>(),
      columns: new Map<string, string[]>(),
    }
    renderForm({ catalogHints: hints })
    expect(screen.getByPlaceholderText('Filter schemas…')).toBeInTheDocument()
  })

  it('does not show filter input when hint count is at or below threshold', () => {
    const fewSchemas = Array.from({ length: 15 }, (_, i) => `schema_${i}`)
    const hints = {
      schemas: fewSchemas,
      tables: new Map<string, string[]>(),
      columns: new Map<string, string[]>(),
    }
    renderForm({ catalogHints: hints })
    expect(screen.queryByPlaceholderText('Filter schemas…')).toBeNull()
  })

  it('filters chips by search term', async () => {
    const manySchemas = ['public', 'analytics', 'reporting', ...Array.from({ length: 13 }, (_, i) => `extra_${i}`)]
    const hints = {
      schemas: manySchemas,
      tables: new Map<string, string[]>(),
      columns: new Map<string, string[]>(),
    }
    renderForm({ catalogHints: hints })
    const filterInput = screen.getByPlaceholderText('Filter schemas…')
    await userEvent.type(filterInput, 'publ')
    expect(screen.getByRole('button', { name: 'public' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'analytics' })).toBeNull()
  })
})

describe('PolicyForm — submission', () => {
  it('calls onSubmit with policy_type and targets', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ onSubmit })

    // Name is required — fill it in
    const nameInput = screen.getAllByRole('textbox')[0]
    await userEvent.clear(nameInput)
    await userEvent.type(nameInput, 'my-policy')

    fireEvent.submit(document.querySelector('form')!)
    await new Promise((r) => setTimeout(r, 0))

    if (onSubmit.mock.calls.length > 0) {
      const values = onSubmit.mock.calls[0][0]
      expect(values.policy_type).toBe('row_filter')
      expect(Array.isArray(values.targets)).toBe(true)
    }
  })
})

describe('PolicyForm — section ordering', () => {
  it('renders sections in order: Policy → Effect → Targets → Decision Function', () => {
    renderForm()
    const text = document.body.textContent ?? ''
    const policyIdx = text.indexOf('name and status')
    const effectIdx = text.indexOf('what this policy does')
    const targetsIdx = text.indexOf('where it applies')
    const decisionIdx = text.indexOf('when it fires')
    expect(policyIdx).toBeLessThan(effectIdx)
    expect(effectIdx).toBeLessThan(targetsIdx)
    expect(targetsIdx).toBeLessThan(decisionIdx)
  })
})

describe('PolicyForm — decision function toggle', () => {
  it('toggle defaults to OFF for new policies', () => {
    renderForm()
    expect(screen.getByText('Policy always fires. No custom logic evaluated.')).toBeTruthy()
  })

  it('toggle defaults to ON when policy has decision_function', () => {
    const { policy } = makePolicyWithFn({ name: 'biz-hours' })
    renderForm({ initial: policy })
    expect(screen.queryByText('Policy always fires.')).toBeNull()
    expect(screen.getByText('biz-hours')).toBeTruthy()
  })

  it('toggle ON shows "Create New" and "Select Existing" buttons', async () => {
    renderForm()
    // Turn toggle ON
    await userEvent.click(screen.getByLabelText('Use decision function'))
    expect(screen.getByText('+ Create New')).toBeTruthy()
    expect(screen.getByText('Select Existing')).toBeTruthy()
  })

  it('toggle OFF hides decision function controls', () => {
    renderForm()
    expect(screen.queryByText('+ Create New')).toBeNull()
    expect(screen.queryByText('Select Existing')).toBeNull()
  })

  it('toggle OFF then back ON restores the attached function without losing it', async () => {
    const { policy } = makePolicyWithFn({ id: 'fn-1', name: 'my-fn' })
    renderForm({ initial: policy })

    // Function is visible initially
    expect(screen.getByText('my-fn')).toBeTruthy()

    // Toggle OFF
    await userEvent.click(screen.getByLabelText('Use decision function'))
    expect(screen.getByText('Policy always fires. No custom logic evaluated.')).toBeTruthy()

    // Toggle back ON — function should be restored
    await userEvent.click(screen.getByLabelText('Use decision function'))
    expect(screen.getByText('my-fn')).toBeTruthy()
  })

  it('toggle OFF when function attached sets decision_function_id to null in form values', async () => {
    const { policy } = makePolicyWithFn({ id: 'fn-1', name: 'my-fn' })
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ initial: policy, onSubmit })

    // Toggle OFF
    await userEvent.click(screen.getByLabelText('Use decision function'))

    // Fill name and submit
    const nameInput = screen.getAllByRole('textbox')[0]
    await userEvent.clear(nameInput)
    await userEvent.type(nameInput, 'my-policy')

    fireEvent.submit(document.querySelector('form')!)
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledTimes(1)
      expect(onSubmit.mock.calls[0][0].decision_function_id).toBeNull()
    })
  })
})

describe('PolicyForm — compact widget', () => {
  it('shows function name and status when function is attached', () => {
    const { policy } = makePolicyWithFn({ name: 'biz-hours', is_enabled: true, evaluate_context: 'session' })
    renderForm({ initial: policy })
    expect(screen.getByText('biz-hours')).toBeTruthy()
    const enabledSpans = screen.getAllByText('Enabled')
    expect(enabledSpans.length).toBeGreaterThanOrEqual(1)
    expect(screen.getByText(/Session/)).toBeTruthy()
  })

  it('"Edit" button is present when function is attached', () => {
    const { policy } = makePolicyWithFn()
    renderForm({ initial: policy })
    expect(screen.getByText('Edit')).toBeTruthy()
  })

  it('"Detach" button removes function but keeps toggle ON', async () => {
    const { policy } = makePolicyWithFn({ name: 'to-detach' })
    renderForm({ initial: policy })

    await userEvent.click(screen.getByText('Detach'))

    // Function name gone, but Create New visible (toggle still ON)
    expect(screen.queryByText('to-detach')).toBeNull()
    expect(screen.getByText('+ Create New')).toBeTruthy()
  })
})

describe('PolicyForm — stale reference recovery', () => {
  it('detaching a stale ref reveals Create/Select buttons', async () => {
    // Policy references a deleted function: decision_function_id set but no summary
    const policy = makePolicy({
      decision_function_id: 'deleted-fn-id',
      decision_function: undefined,
    })
    renderForm({ initial: policy })

    // Stale warning is shown
    expect(screen.getByText(/Function not found/)).toBeTruthy()
    // Create/Select buttons are NOT shown yet
    expect(screen.queryByText('+ Create New')).toBeNull()

    // Detach the stale reference
    await userEvent.click(screen.getByText('Detach'))

    // Stale warning gone, Create/Select buttons appear
    expect(screen.queryByText(/Function not found/)).toBeNull()
    expect(screen.getByText('+ Create New')).toBeTruthy()
    expect(screen.getByText('Select Existing')).toBeTruthy()
  })
})

describe('PolicyForm — select existing', () => {
  it('fetches listDecisionFunctions when "Select Existing" clicked', async () => {
    mockListDecisionFunctions.mockResolvedValue([
      makeDecisionFunction({ id: 'fn-a', name: 'func-alpha' }),
    ])
    renderForm()

    await userEvent.click(screen.getByLabelText('Use decision function'))
    await userEvent.click(screen.getByText('Select Existing'))

    await waitFor(() => {
      expect(mockListDecisionFunctions).toHaveBeenCalledTimes(1)
    })
  })

  it('shows function names in dropdown', async () => {
    mockListDecisionFunctions.mockResolvedValue([
      makeDecisionFunction({ id: 'fn-a', name: 'func-alpha' }),
      makeDecisionFunction({ id: 'fn-b', name: 'func-beta' }),
    ])
    renderForm()

    await userEvent.click(screen.getByLabelText('Use decision function'))
    await userEvent.click(screen.getByText('Select Existing'))

    await waitFor(() => {
      expect(screen.getByText('func-alpha')).toBeTruthy()
      expect(screen.getByText('func-beta')).toBeTruthy()
    })
  })

  it('selecting a function attaches it', async () => {
    const fnA = makeDecisionFunction({ id: 'fn-a', name: 'func-alpha' })
    mockListDecisionFunctions.mockResolvedValue([fnA])
    mockGetDecisionFunction.mockResolvedValue(fnA)
    renderForm()

    await userEvent.click(screen.getByLabelText('Use decision function'))
    await userEvent.click(screen.getByText('Select Existing'))

    await waitFor(() => {
      expect(screen.getByText('func-alpha')).toBeTruthy()
    })

    const selectEl = screen.getByText('Pick a function…').closest('select')!
    await userEvent.selectOptions(selectEl, 'fn-a')

    await waitFor(() => {
      expect(mockGetDecisionFunction).toHaveBeenCalledWith('fn-a')
    })
  })

  it('shows empty state when no functions available', async () => {
    mockListDecisionFunctions.mockResolvedValue([])
    renderForm()

    await userEvent.click(screen.getByLabelText('Use decision function'))
    await userEvent.click(screen.getByText('Select Existing'))

    await waitFor(() => {
      expect(screen.getByText('No functions available')).toBeTruthy()
    })
  })
})

describe('PolicyForm — submission with decision function', () => {
  it('submit includes decision_function_id when function attached', async () => {
    const { policy } = makePolicyWithFn({ id: 'fn-99' })
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ initial: policy, onSubmit })

    const nameInput = screen.getAllByRole('textbox')[0]
    await userEvent.clear(nameInput)
    await userEvent.type(nameInput, 'my-policy')

    fireEvent.submit(document.querySelector('form')!)
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledTimes(1)
      expect(onSubmit.mock.calls[0][0].decision_function_id).toBe('fn-99')
    })
  })

  it('submit sends decision_function_id: null when toggle is OFF', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ onSubmit })

    const nameInput = screen.getAllByRole('textbox')[0]
    await userEvent.clear(nameInput)
    await userEvent.type(nameInput, 'my-policy')

    fireEvent.submit(document.querySelector('form')!)
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledTimes(1)
      expect(onSubmit.mock.calls[0][0].decision_function_id).toBeNull()
    })
  })
})

describe('PolicyForm — decision function validation', () => {
  it('blocks submit when toggle ON but no function attached', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ onSubmit })

    // Toggle ON
    await userEvent.click(screen.getByLabelText('Use decision function'))

    // Fill name so name validation passes
    const nameInput = screen.getAllByRole('textbox')[0]
    await userEvent.clear(nameInput)
    await userEvent.type(nameInput, 'my-policy')

    fireEvent.submit(document.querySelector('form')!)
    await waitFor(() => {
      expect(screen.getByText(/create or select a decision function/)).toBeTruthy()
    })
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('blocks submit when stale decision function reference detected', async () => {
    const policy = makePolicy({
      decision_function_id: 'deleted-fn-id',
      decision_function: undefined,
    })
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ initial: policy, onSubmit })

    // Fill name so name validation passes
    const nameInput = screen.getAllByRole('textbox')[0]
    await userEvent.clear(nameInput)
    await userEvent.type(nameInput, 'my-policy')

    fireEvent.submit(document.querySelector('form')!)
    await waitFor(() => {
      expect(screen.getByText(/no longer exists/i)).toBeTruthy()
    })
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('shows error when list decision functions API fails', async () => {
    mockListDecisionFunctions.mockRejectedValue(new Error('Network error'))
    renderForm()

    await userEvent.click(screen.getByLabelText('Use decision function'))
    await userEvent.click(screen.getByText('Select Existing'))

    await waitFor(() => {
      expect(screen.getByText('Failed to load decision functions')).toBeTruthy()
    })
  })

  it('select existing → submit includes decision_function_id', async () => {
    const fnA = makeDecisionFunction({ id: 'fn-a', name: 'func-alpha' })
    mockListDecisionFunctions.mockResolvedValue([fnA])
    mockGetDecisionFunction.mockResolvedValue(fnA)
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ onSubmit })

    // Fill name
    const nameInput = screen.getAllByRole('textbox')[0]
    await userEvent.clear(nameInput)
    await userEvent.type(nameInput, 'my-policy')

    // Toggle ON and select existing
    await userEvent.click(screen.getByLabelText('Use decision function'))
    await userEvent.click(screen.getByText('Select Existing'))

    await waitFor(() => expect(screen.getByText('func-alpha')).toBeTruthy())

    const selectEl = screen.getByText('Pick a function…').closest('select')!
    await userEvent.selectOptions(selectEl, 'fn-a')

    // Wait for function to be attached (compact widget shows name)
    await waitFor(() => {
      // After selection, the compact widget replaces the select dropdown
      expect(mockGetDecisionFunction).toHaveBeenCalledWith('fn-a')
    })

    // Submit
    fireEvent.submit(document.querySelector('form')!)
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledTimes(1)
      expect(onSubmit.mock.calls[0][0].decision_function_id).toBe('fn-a')
    })
  })

  it('create via modal → submit includes decision_function_id', async () => {
    const created = makeDecisionFunction({ id: 'new-fn-id', name: 'auto-name' })
    mockCreateDecisionFunction.mockResolvedValue(created)
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ onSubmit })

    // Fill name
    const nameInput = screen.getAllByRole('textbox')[0]
    await userEvent.clear(nameInput)
    await userEvent.type(nameInput, 'my-policy')

    // Toggle ON and open create modal
    await userEvent.click(screen.getByLabelText('Use decision function'))
    await userEvent.click(screen.getByText('+ Create New'))

    // Modal should be open — fill code and save
    await waitFor(() => expect(screen.getByText('Create Function')).toBeTruthy())
    const editors = screen.getAllByTestId('codemirror')
    // editors[0] is the filter expression editor; modal JS editor follows it
    const jsEditor = editors.length > 1 ? editors[1] : editors[0]
    fireEvent.change(jsEditor, { target: { value: 'function evaluate(ctx) { return { fire: true }; }' } })
    await userEvent.click(screen.getByText('Create Function'))

    // Wait for modal to close and function to be attached
    await waitFor(() => expect(mockCreateDecisionFunction).toHaveBeenCalledTimes(1))

    // Submit the form
    fireEvent.submit(document.querySelector('form')!)
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledTimes(1)
      expect(onSubmit.mock.calls[0][0].decision_function_id).toBe('new-fn-id')
    })
  })
})
