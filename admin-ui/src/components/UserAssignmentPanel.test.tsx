import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { UserAssignmentPanel } from './UserAssignmentPanel'
import { makeUser } from '../test/factories'

vi.mock('../api/datasources', () => ({
  getDataSourceUsers: vi.fn(),
  setDataSourceUsers: vi.fn(),
}))

vi.mock('../api/users', () => ({
  listUsers: vi.fn(),
}))

import { getDataSourceUsers, setDataSourceUsers } from '../api/datasources'
import { listUsers } from '../api/users'

const mockGetDsUsers = getDataSourceUsers as ReturnType<typeof vi.fn>
const mockSetDsUsers = setDataSourceUsers as ReturnType<typeof vi.fn>
const mockListUsers = listUsers as ReturnType<typeof vi.fn>

beforeEach(() => vi.clearAllMocks())

describe('UserAssignmentPanel', () => {
  const alice = makeUser({ id: 'u-alice', username: 'alice' })
  const bob = makeUser({ id: 'u-bob', username: 'bob', is_admin: true })

  it('renders loading state initially', () => {
    mockListUsers.mockReturnValue(new Promise(() => {}))
    mockGetDsUsers.mockReturnValue(new Promise(() => {}))

    renderWithProviders(<UserAssignmentPanel datasourceId="ds-1" />)
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('renders users with checkboxes after load', async () => {
    mockListUsers.mockResolvedValue({ data: [alice, bob], total: 2, page: 1, page_size: 100 })
    mockGetDsUsers.mockResolvedValue([alice])

    renderWithProviders(<UserAssignmentPanel datasourceId="ds-1" />)

    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument())
    expect(screen.getByText('bob')).toBeInTheDocument()
  })

  it('pre-checks assigned users', async () => {
    mockListUsers.mockResolvedValue({ data: [alice, bob], total: 2, page: 1, page_size: 100 })
    mockGetDsUsers.mockResolvedValue([alice])

    renderWithProviders(<UserAssignmentPanel datasourceId="ds-1" />)

    await waitFor(() => screen.getByText('alice'))

    const checkboxes = screen.getAllByRole('checkbox')
    // alice is assigned → checked; bob is not → unchecked
    expect(checkboxes[0]).toBeChecked()   // alice
    expect(checkboxes[1]).not.toBeChecked() // bob
  })

  it('calls setDataSourceUsers with selected IDs on Save', async () => {
    const user = userEvent.setup()
    mockListUsers.mockResolvedValue({ data: [alice, bob], total: 2, page: 1, page_size: 100 })
    mockGetDsUsers.mockResolvedValue([alice])
    mockSetDsUsers.mockResolvedValue(undefined)

    renderWithProviders(<UserAssignmentPanel datasourceId="ds-1" />)

    await waitFor(() => screen.getByText('alice'))

    // Toggle bob on
    const checkboxes = screen.getAllByRole('checkbox')
    await user.click(checkboxes[1])

    await user.click(screen.getByRole('button', { name: /save assignments/i }))

    await waitFor(() =>
      expect(mockSetDsUsers).toHaveBeenCalledWith('ds-1', expect.arrayContaining(['u-alice', 'u-bob'])),
    )
  })

  it('shows empty state when no users exist', async () => {
    mockListUsers.mockResolvedValue({ data: [], total: 0, page: 1, page_size: 100 })
    mockGetDsUsers.mockResolvedValue([])

    renderWithProviders(<UserAssignmentPanel datasourceId="ds-1" />)

    await waitFor(() => expect(screen.getByText(/no users found/i)).toBeInTheDocument())
  })
})
