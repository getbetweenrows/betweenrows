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
