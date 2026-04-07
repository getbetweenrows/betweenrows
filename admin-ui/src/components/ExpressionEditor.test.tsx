import { describe, it, expect, vi } from 'vitest'
import { screen, fireEvent, waitFor } from '@testing-library/react'
import { ExpressionEditor } from './ExpressionEditor'
import { renderWithProviders } from '../test/test-utils'

// Mock CodeMirror as a simple textarea (same pattern as PolicyForm tests)
vi.mock('@uiw/react-codemirror', () => ({
  default: ({
    value,
    onChange,
    placeholder,
  }: {
    value: string
    onChange?: (v: string) => void
    placeholder?: string
  }) => (
    <textarea
      data-testid="codemirror"
      value={value}
      onChange={(e) => onChange?.(e.target.value)}
      placeholder={placeholder}
    />
  ),
}))

describe('ExpressionEditor', () => {
  it('renders with placeholder', () => {
    renderWithProviders(
      <ExpressionEditor
        value=""
        onChange={() => {}}
        placeholder="organization_id = {user.username}"
        templateItems={[]}
      />,
    )
    expect(screen.getByPlaceholderText('organization_id = {user.username}')).toBeTruthy()
  })

  it('renders with initial value', () => {
    renderWithProviders(
      <ExpressionEditor
        value="owner = {user.username}"
        onChange={() => {}}
        templateItems={[]}
      />,
    )
    expect(screen.getByDisplayValue('owner = {user.username}')).toBeTruthy()
  })

  it('calls onChange when value changes', () => {
    const onChange = vi.fn()
    renderWithProviders(
      <ExpressionEditor value="" onChange={onChange} templateItems={[]} />,
    )
    fireEvent.change(screen.getByTestId('codemirror'), {
      target: { value: 'region = {user.region}' },
    })
    expect(onChange).toHaveBeenCalledWith('region = {user.region}')
  })

  it('shows Check button when onValidate is provided', () => {
    renderWithProviders(
      <ExpressionEditor
        value="test"
        onChange={() => {}}
        templateItems={[]}
        onValidate={vi.fn().mockResolvedValue({ valid: true })}
      />,
    )
    expect(screen.getByTitle('Validate expression syntax')).toBeTruthy()
  })

  it('does not show Check button when onValidate is not provided', () => {
    renderWithProviders(
      <ExpressionEditor value="test" onChange={() => {}} templateItems={[]} />,
    )
    expect(screen.queryByTitle('Validate expression syntax')).toBeNull()
  })

  it('shows green check on valid expression', async () => {
    const onValidate = vi.fn().mockResolvedValue({ valid: true })
    renderWithProviders(
      <ExpressionEditor
        value="region = {user.region}"
        onChange={() => {}}
        templateItems={[]}
        onValidate={onValidate}
      />,
    )
    fireEvent.click(screen.getByTitle('Validate expression syntax'))
    await waitFor(() => {
      expect(onValidate).toHaveBeenCalledWith('region = {user.region}')
      expect(screen.getByText('✓')).toBeTruthy()
    })
  })

  it('shows red X and error on invalid expression', async () => {
    const onValidate = vi
      .fn()
      .mockResolvedValue({ valid: false, error: 'Unsupported syntax: EXTRACT' })
    renderWithProviders(
      <ExpressionEditor
        value="EXTRACT(HOUR FROM col)"
        onChange={() => {}}
        templateItems={[]}
        onValidate={onValidate}
      />,
    )
    fireEvent.click(screen.getByTitle('Validate expression syntax'))
    await waitFor(() => {
      expect(screen.getByText('✗')).toBeTruthy()
      expect(screen.getByText('Unsupported syntax: EXTRACT')).toBeTruthy()
    })
  })

  it('resets validation state when expression changes', async () => {
    const onValidate = vi
      .fn()
      .mockResolvedValue({ valid: false, error: 'bad' })
    const onChange = vi.fn()
    renderWithProviders(
      <ExpressionEditor
        value="bad expr"
        onChange={onChange}
        templateItems={[]}
        onValidate={onValidate}
      />,
    )
    // Validate first
    fireEvent.click(screen.getByTitle('Validate expression syntax'))
    await waitFor(() => expect(screen.getByText('bad')).toBeTruthy())

    // Change expression — validation error should clear
    fireEvent.change(screen.getByTestId('codemirror'), {
      target: { value: 'new expr' },
    })
    expect(screen.queryByText('bad')).toBeNull()
  })

  it('disables Check button when expression is empty', () => {
    renderWithProviders(
      <ExpressionEditor
        value=""
        onChange={() => {}}
        templateItems={[]}
        onValidate={vi.fn()}
      />,
    )
    const btn = screen.getByTitle('Validate expression syntax')
    expect(btn).toHaveProperty('disabled', true)
  })
})
