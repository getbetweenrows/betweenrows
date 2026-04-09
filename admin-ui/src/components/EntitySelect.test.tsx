import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { EntitySelect } from './EntitySelect'
import { renderWithProviders } from '../test/test-utils'
import type { EntityOption } from '../utils/entitySearchFns'

// Headless UI uses ResizeObserver internally
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver

const OPTIONS: EntityOption[] = [
  { id: 'aaa-111-bbb-222', label: 'alice' },
  { id: 'ccc-333-ddd-444', label: 'bob' },
]

function makeSearchFn(results: EntityOption[] = OPTIONS) {
  return vi.fn(async () => results)
}

function renderSelect(
  overrides: Partial<Parameters<typeof EntitySelect>[0]> = {},
) {
  const onChange = vi.fn()
  const searchFn = overrides.searchFn ?? makeSearchFn()
  const result = renderWithProviders(
    <EntitySelect
      label="User"
      value=""
      onChange={onChange}
      searchFn={searchFn as (s: string) => Promise<EntityOption[]>}
      placeholder="Search users…"
      {...overrides}
    />,
  )
  return { ...result, onChange, searchFn }
}

describe('EntitySelect', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('renders with placeholder', () => {
    renderSelect()
    expect(screen.getByPlaceholderText('Search users…')).toBeInTheDocument()
  })

  it('renders label', () => {
    renderSelect()
    expect(screen.getByText('User')).toBeInTheDocument()
  })

  it('debounces search', async () => {
    const searchFn = makeSearchFn()
    renderSelect({ searchFn })

    const input = screen.getByRole('combobox')
    await userEvent.type(input, 'ali')

    // The initial empty-query fetch may have fired, but the typed query shouldn't yet
    const callCountBefore = searchFn.mock.calls.filter(
      (c: string[]) => c[0] === 'ali',
    ).length
    expect(callCountBefore).toBe(0)

    // Advance past debounce
    await vi.advanceTimersByTimeAsync(350)

    await waitFor(() => {
      const callsWithQuery = searchFn.mock.calls.filter(
        (c: string[]) => c[0] === 'ali',
      )
      expect(callsWithQuery.length).toBeGreaterThan(0)
    })
  })

  it('displays results in dropdown', async () => {
    renderSelect()

    const input = screen.getByRole('combobox')
    await userEvent.type(input, 'a')
    await vi.advanceTimersByTimeAsync(350)

    await waitFor(() => {
      expect(screen.getByText('alice')).toBeInTheDocument()
    })
  })

  it('calls onChange with selected option id', async () => {
    const { onChange } = renderSelect()

    const input = screen.getByRole('combobox')
    await userEvent.type(input, 'a')
    await vi.advanceTimersByTimeAsync(350)

    await waitFor(() => {
      expect(screen.getByText('alice')).toBeInTheDocument()
    })

    await userEvent.click(screen.getByText('alice'))

    expect(onChange).toHaveBeenCalledWith('aaa-111-bbb-222')
  })

  it('shows clear button when value is set and clears on click', async () => {
    const { onChange } = renderSelect({ value: 'aaa-111-bbb-222' })

    const clearBtn = screen.getByLabelText('Clear User')
    expect(clearBtn).toBeInTheDocument()

    await userEvent.click(clearBtn)
    expect(onChange).toHaveBeenCalledWith('')
  })

  it('does not show clear button when no value', () => {
    renderSelect({ value: '' })
    expect(screen.queryByLabelText('Clear User')).not.toBeInTheDocument()
  })

  it('shows truncated UUID when showId is true', async () => {
    renderSelect({ showId: true })

    const input = screen.getByRole('combobox')
    await userEvent.type(input, 'a')
    await vi.advanceTimersByTimeAsync(350)

    await waitFor(() => {
      expect(document.body.textContent).toMatch(/aaa-111-/)
    })
  })

  it('shows no results message', async () => {
    const searchFn = makeSearchFn([])
    renderSelect({ searchFn })

    const input = screen.getByRole('combobox')
    await userEvent.type(input, 'xyz')
    await vi.advanceTimersByTimeAsync(350)

    await waitFor(() => {
      expect(screen.getByText('No results')).toBeInTheDocument()
    })
  })
})
