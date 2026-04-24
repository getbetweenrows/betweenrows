import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { DataSourceDangerZone } from './DataSourceDangerZone'
import { makeDataSource } from '../test/factories'

vi.mock('../api/datasources', () => ({
  updateDataSource: vi.fn(),
  deleteDataSource: vi.fn(),
}))

import { updateDataSource, deleteDataSource } from '../api/datasources'
const mockUpdate = updateDataSource as ReturnType<typeof vi.fn>
const mockDelete = deleteDataSource as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
})

describe('DataSourceDangerZone — rename', () => {
  it('Rename button is disabled until the new name changes and validates', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db' })

    renderWithProviders(<DataSourceDangerZone ds={ds} />, { authenticated: true })

    await user.click(screen.getByRole('button', { name: /rename…/i }))

    const input = screen.getByDisplayValue('prod-db')
    const submit = screen.getByRole('button', { name: /^rename$/i })
    expect(submit).toBeDisabled() // name unchanged

    await user.clear(input)
    await user.type(input, '123bad') // fails validation (must start with letter)
    expect(submit).toBeDisabled()

    await user.clear(input)
    await user.type(input, 'prod-db-v2')
    expect(submit).not.toBeDisabled()
  })

  it('submits the rename with the trimmed new name', async () => {
    const user = userEvent.setup()
    mockUpdate.mockResolvedValue(undefined)
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db' })

    renderWithProviders(<DataSourceDangerZone ds={ds} />, { authenticated: true })

    await user.click(screen.getByRole('button', { name: /rename…/i }))
    const input = screen.getByDisplayValue('prod-db')
    await user.clear(input)
    await user.type(input, 'prod-db-v2')
    await user.click(screen.getByRole('button', { name: /^rename$/i }))

    await waitFor(() => expect(mockUpdate).toHaveBeenCalled())
    expect(mockUpdate.mock.calls[0][0]).toBe('ds-1')
    expect(mockUpdate.mock.calls[0][1]).toEqual({ name: 'prod-db-v2' })
  })
})

describe('DataSourceDangerZone — delete', () => {
  it('shows "Deactivate instead" option when datasource is active', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', is_active: true })

    renderWithProviders(<DataSourceDangerZone ds={ds} />, { authenticated: true })
    await user.click(screen.getByRole('button', { name: /delete…/i }))

    expect(
      screen.getByRole('button', { name: /deactivate instead/i }),
    ).toBeInTheDocument()
  })

  it('hides "Deactivate instead" when datasource is already inactive', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', is_active: false })

    renderWithProviders(<DataSourceDangerZone ds={ds} />, { authenticated: true })
    await user.click(screen.getByRole('button', { name: /delete…/i }))

    expect(screen.queryByRole('button', { name: /deactivate instead/i })).toBeNull()
  })

  it('Deactivate-instead path calls updateDataSource with is_active: false', async () => {
    const user = userEvent.setup()
    mockUpdate.mockResolvedValue(undefined)
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', is_active: true })

    renderWithProviders(<DataSourceDangerZone ds={ds} />, { authenticated: true })
    await user.click(screen.getByRole('button', { name: /delete…/i }))
    await user.click(screen.getByRole('button', { name: /deactivate instead/i }))

    await waitFor(() => expect(mockUpdate).toHaveBeenCalled())
    expect(mockUpdate.mock.calls[0][0]).toBe('ds-1')
    expect(mockUpdate.mock.calls[0][1]).toEqual({ is_active: false })
    // Delete must NOT be called on the deactivate path.
    expect(mockDelete).not.toHaveBeenCalled()
  })

  it('Delete button is disabled until the typed name exactly matches', async () => {
    const user = userEvent.setup()
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db' })

    renderWithProviders(<DataSourceDangerZone ds={ds} />, { authenticated: true })
    await user.click(screen.getByRole('button', { name: /delete…/i }))

    const deleteButton = screen.getByRole('button', { name: /^delete$/i })
    expect(deleteButton).toBeDisabled()

    const input = screen.getByRole('textbox')
    await user.type(input, 'prod-DB') // wrong case
    expect(deleteButton).toBeDisabled()

    await user.clear(input)
    await user.type(input, 'prod-db')
    expect(deleteButton).not.toBeDisabled()
  })

  it('calls deleteDataSource when name matches and user confirms', async () => {
    const user = userEvent.setup()
    mockDelete.mockResolvedValue(undefined)
    const ds = makeDataSource({ id: 'ds-1', name: 'prod-db', is_active: false })

    renderWithProviders(<DataSourceDangerZone ds={ds} />, { authenticated: true })
    await user.click(screen.getByRole('button', { name: /delete…/i }))
    await user.type(screen.getByRole('textbox'), 'prod-db')
    await user.click(screen.getByRole('button', { name: /^delete$/i }))

    await waitFor(() => expect(mockDelete).toHaveBeenCalled())
    expect(mockDelete.mock.calls[0][0]).toBe('ds-1')
  })
})
