import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { Route, Routes } from 'react-router-dom'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { UserEditPage } from './UserEditPage'
import { makeUser } from '../test/factories'

vi.mock('../api/users', () => ({
  getUser: vi.fn(),
  updateUser: vi.fn(),
  changePassword: vi.fn(),
}))

import { getUser, updateUser, changePassword } from '../api/users'
const mockGetUser = getUser as ReturnType<typeof vi.fn>
const mockUpdateUser = updateUser as ReturnType<typeof vi.fn>
const mockChangePassword = changePassword as ReturnType<typeof vi.fn>

beforeEach(() => vi.clearAllMocks())

// Wrap UserEditPage in a Route so useParams works correctly
function renderEditPage() {
  return renderWithProviders(
    <Routes>
      <Route path="/users/:id/edit" element={<UserEditPage />} />
    </Routes>,
    { authenticated: true, routerEntries: ['/users/u-1/edit'] },
  )
}

describe('UserEditPage', () => {
  it('shows loading state initially', () => {
    mockGetUser.mockReturnValue(new Promise(() => {}))
    renderEditPage()
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows not found when user is null', async () => {
    mockGetUser.mockResolvedValue(null)
    renderEditPage()
    await waitFor(() => expect(screen.getByText(/user not found/i)).toBeInTheDocument())
  })

  it('renders form with user data', async () => {
    const userObj = makeUser({ id: 'u-1', username: 'alice', tenant: 'acme' })
    mockGetUser.mockResolvedValue(userObj)

    renderEditPage()

    await waitFor(() => expect(screen.getByText(/@alice/i)).toBeInTheDocument())
    expect(screen.getByDisplayValue('acme')).toBeInTheDocument()
  })

  it('submits update on save', async () => {
    const user = userEvent.setup()
    const userObj = makeUser({ id: 'u-1', username: 'alice', tenant: 'acme' })
    mockGetUser.mockResolvedValue(userObj)
    mockUpdateUser.mockResolvedValue(userObj)

    renderEditPage()

    await waitFor(() => screen.getByText(/@alice/i))
    await user.click(screen.getByRole('button', { name: /save changes/i }))

    await waitFor(() => expect(mockUpdateUser).toHaveBeenCalled())
    expect(mockUpdateUser.mock.calls[0][0]).toBe('u-1')
  })

  it('shows password change section', async () => {
    const userObj = makeUser({ id: 'u-1', username: 'alice', tenant: 'acme' })
    mockGetUser.mockResolvedValue(userObj)

    renderEditPage()

    await waitFor(() => screen.getByText(/change password/i))
    expect(screen.getByRole('button', { name: /^change$/i })).toBeInTheDocument()
  })

  it('calls changePassword and shows success message', async () => {
    const user = userEvent.setup()
    const userObj = makeUser({ id: 'u-1', username: 'alice', tenant: 'acme' })
    mockGetUser.mockResolvedValue(userObj)
    mockChangePassword.mockResolvedValue(userObj)

    const { container } = renderEditPage()

    await waitFor(() => screen.getByRole('button', { name: /^change$/i }))

    // The password change input is the last password field in the page
    const pwInputs = container.querySelectorAll('input[type="password"]')
    await user.type(pwInputs[pwInputs.length - 1] as HTMLInputElement, 'NewPass1!')
    await user.click(screen.getByRole('button', { name: /^change$/i }))

    await waitFor(() => expect(mockChangePassword).toHaveBeenCalled())
    expect(mockChangePassword.mock.calls[0][0]).toBe('u-1')
    expect(mockChangePassword.mock.calls[0][1]).toBe('NewPass1!')
    await waitFor(() => expect(screen.getByText(/password updated/i)).toBeInTheDocument())
  })
})
