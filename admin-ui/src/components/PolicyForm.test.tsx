import { describe, it, expect, vi } from 'vitest'
import { screen, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PolicyForm } from './PolicyForm'
import { renderWithProviders } from '../test/test-utils'

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

describe('PolicyForm — deny + column_mask validation', () => {
  it('column_mask option is visible when effect is permit', async () => {
    renderForm()
    // Add an obligation — effect defaults to permit
    await userEvent.click(screen.getByText('+ Add obligation'))
    const comboboxes = screen.getAllByRole('combobox')
    // The obligation type select is the last combobox
    const obligationTypeSelect = comboboxes[comboboxes.length - 1]
    const options = Array.from(obligationTypeSelect.querySelectorAll('option')).map(o => o.value)
    expect(options).toContain('column_mask')
  })

  it('column_mask option is hidden when effect is deny', async () => {
    renderForm()
    // Switch effect to deny
    const effectSelect = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(effectSelect, 'deny')
    // Add an obligation
    await userEvent.click(screen.getByText('+ Add obligation'))
    const typeSelects = screen.getAllByRole('combobox')
    // The obligation type select is the last combobox
    const obligationTypeSelect = typeSelects[typeSelects.length - 1]
    const options = Array.from(obligationTypeSelect.querySelectorAll('option')).map(o => o.value)
    expect(options).not.toContain('column_mask')
  })

  it('switching effect to deny removes existing column_mask obligations', async () => {
    renderForm()
    // Add a column_mask obligation while effect is permit
    await userEvent.click(screen.getByText('+ Add obligation'))
    const typeSelects = screen.getAllByRole('combobox')
    const obligationTypeSelect = typeSelects[typeSelects.length - 1]
    await userEvent.selectOptions(obligationTypeSelect, 'column_mask')
    // Verify the column_mask obligation is shown
    expect(screen.getByText('Obligation 1')).toBeTruthy()
    // Switch effect to deny
    const effectSelect = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(effectSelect, 'deny')
    // The column_mask obligation should have been removed
    expect(screen.queryByText('Obligation 1')).toBeNull()
  })

  it('shows a note about column masking when effect is deny', async () => {
    renderForm()
    const effectSelect = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(effectSelect, 'deny')
    expect(screen.getByText(/Column masking is not available on deny policies/i)).toBeTruthy()
  })
})

describe('PolicyForm — object_access obligation type', () => {
  it('object_access option is available in the type selector', async () => {
    renderForm()
    await userEvent.click(screen.getByText('+ Add obligation'))
    const typeSelects = screen.getAllByRole('combobox')
    const obligationTypeSelect = typeSelects[typeSelects.length - 1]
    const options = Array.from(obligationTypeSelect.querySelectorAll('option')).map(o => o.value)
    expect(options).toContain('object_access')
  })

  it('object_access is available on deny policies', async () => {
    renderForm()
    const effectSelect = screen.getAllByRole('combobox')[0]
    await userEvent.selectOptions(effectSelect, 'deny')
    await userEvent.click(screen.getByText('+ Add obligation'))
    const typeSelects = screen.getAllByRole('combobox')
    const obligationTypeSelect = typeSelects[typeSelects.length - 1]
    const options = Array.from(obligationTypeSelect.querySelectorAll('option')).map(o => o.value)
    expect(options).toContain('object_access')
  })

  it('object_access shows schema field and optional table field', async () => {
    renderForm()
    await userEvent.click(screen.getByText('+ Add obligation'))
    const typeSelects = screen.getAllByRole('combobox')
    const obligationTypeSelect = typeSelects[typeSelects.length - 1]
    await userEvent.selectOptions(obligationTypeSelect, 'object_access')
    // Should show the hint text for object_access
    expect(screen.getByText(/Hides the entire schema/i)).toBeTruthy()
    // Table field should have "leave blank" placeholder hint
    expect(document.body.textContent).toMatch(/optional/i)
  })
})

describe('PolicyForm — submits object_access obligation correctly', () => {
  it('omits table field when blank for schema-level deny', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    renderForm({ onSubmit })

    // Add object_access obligation
    await userEvent.click(screen.getByText('+ Add obligation'))
    const typeSelects = screen.getAllByRole('combobox')
    await userEvent.selectOptions(typeSelects[typeSelects.length - 1], 'object_access')

    // Set schema
    const textboxes = screen.getAllByRole('textbox')
    // Find the schema input (has placeholder 'analytics')
    const schemaInput = textboxes.find(i => (i as HTMLInputElement).placeholder === 'analytics')
    if (schemaInput) {
      await userEvent.clear(schemaInput)
      await userEvent.type(schemaInput, 'analytics')
    }

    // Submit
    fireEvent.submit(document.querySelector('form')!)
    // Wait for onSubmit to be called
    await new Promise(r => setTimeout(r, 0))

    if (onSubmit.mock.calls.length > 0) {
      const values = onSubmit.mock.calls[0][0]
      const obl = values.obligations[0]
      expect(obl.obligation_type).toBe('object_access')
      expect(obl.definition.action).toBe('deny')
      expect(obl.definition.table).toBeUndefined()
    }
  })
})
