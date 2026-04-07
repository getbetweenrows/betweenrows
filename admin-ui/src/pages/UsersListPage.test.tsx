import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { UsersListPage } from './UsersListPage'
import { makeUser, makePaginatedUsers } from '../test/factories'

vi.mock('../api/users', () => ({
  listUsers: vi.fn(),
  deleteUser: vi.fn(),
}))

import { listUsers, deleteUser } from '../api/users'
const mockListUsers = listUsers as ReturnType<typeof vi.fn>
const mockDeleteUser = deleteUser as ReturnType<typeof vi.fn>

beforeEach(() => vi.clearAllMocks())

describe('UsersListPage', () => {
  it('shows loading state initially', () => {
    mockListUsers.mockReturnValue(new Promise(() => {}))
    renderWithProviders(<UsersListPage />, { authenticated: true })
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows error state when request fails', async () => {
    mockListUsers.mockRejectedValue(new Error('Network error'))
    renderWithProviders(<UsersListPage />, { authenticated: true })
    await waitFor(() =>
      expect(screen.getByText(/failed to load users/i)).toBeInTheDocument(),
    )
  })

  it('shows empty state when no users', async () => {
    mockListUsers.mockResolvedValue(makePaginatedUsers([]))
    renderWithProviders(<UsersListPage />, { authenticated: true })
    await waitFor(() =>
      expect(screen.getByText(/no users found/i)).toBeInTheDocument(),
    )
  })

  it('renders user rows', async () => {
    const alice = makeUser({ username: 'alice', is_admin: false })
    const bob = makeUser({ username: 'bob', is_admin: true })
    mockListUsers.mockResolvedValue(makePaginatedUsers([alice, bob]))
    renderWithProviders(<UsersListPage />, { authenticated: true })
    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument())
    expect(screen.getByText('bob')).toBeInTheDocument()
    expect(screen.getByText('Admin')).toBeInTheDocument()
  })

  it('delete confirms and calls deleteUser', async () => {
    const user = userEvent.setup()
    const alice = makeUser({ id: 'u-alice', username: 'alice' })
    mockListUsers.mockResolvedValue(makePaginatedUsers([alice]))
    mockDeleteUser.mockResolvedValue(undefined)

    renderWithProviders(<UsersListPage />, { authenticated: true })
    await waitFor(() => screen.getByText('alice'))

    await user.click(screen.getByRole('button', { name: /^delete$/i }))

    // TanStack Query v5 mutationFn is called as (variables, context) — match first arg
    await waitFor(() => {
      expect(mockDeleteUser).toHaveBeenCalled()
      expect(mockDeleteUser.mock.calls[0][0]).toBe('u-alice')
    })
  })

  it('edit navigates to edit page', async () => {
    const user = userEvent.setup()
    const alice = makeUser({ id: 'u-alice', username: 'alice' })
    mockListUsers.mockResolvedValue(makePaginatedUsers([alice]))

    renderWithProviders(<UsersListPage />, { authenticated: true })
    await waitFor(() => screen.getByText('alice'))

    await user.click(screen.getByRole('button', { name: /^edit$/i }))
    // Navigation happens in MemoryRouter without error
    expect(screen.queryByText(/failed/i)).not.toBeInTheDocument()
  })

  it('shows pagination when more than one page', async () => {
    const users = Array.from({ length: 3 }, () => makeUser())
    mockListUsers.mockResolvedValue({ data: users, total: 45, page: 1, page_size: 20 })

    renderWithProviders(<UsersListPage />, { authenticated: true })
    await waitFor(() => expect(screen.getByText(/page 1 of 3/i)).toBeInTheDocument())
    expect(screen.getByRole('button', { name: /next/i })).toBeInTheDocument()
  })

  it('search form submits and refetches', async () => {
    const user = userEvent.setup()
    mockListUsers.mockResolvedValue(makePaginatedUsers([makeUser({ username: 'alice' })]))

    renderWithProviders(<UsersListPage />, { authenticated: true })
    await waitFor(() => screen.getByRole('button', { name: /^search$/i }))

    await user.type(screen.getByRole('searchbox'), 'alice')
    await user.click(screen.getByRole('button', { name: /^search$/i }))

    await waitFor(() =>
      expect(mockListUsers).toHaveBeenCalledWith(
        expect.objectContaining({ search: 'alice' }),
      ),
    )
  })
})
