import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { LoginPage } from './LoginPage'
import { makeLoginResponse } from '../test/factories'

vi.mock('../api/users', () => ({
  login: vi.fn(),
}))

import { login } from '../api/users'
const mockLogin = login as ReturnType<typeof vi.fn>

beforeEach(() => vi.clearAllMocks())

// LoginPage labels are not associated via htmlFor — use role/container queries.
function getPasswordInput(container: HTMLElement) {
  return container.querySelector('input[type="password"]') as HTMLInputElement
}

describe('LoginPage', () => {
  it('renders username and password fields', () => {
    const { container } = renderWithProviders(<LoginPage />, { routerEntries: ['/login'] })
    // username is the only textbox role input; password has no aria role
    expect(screen.getByRole('textbox')).toBeInTheDocument()
    expect(getPasswordInput(container)).toBeTruthy()
    expect(screen.getByRole('button', { name: /sign in/i })).toBeInTheDocument()
  })

  it('shows loading state during submit', async () => {
    const user = userEvent.setup()
    let resolve!: () => void
    mockLogin.mockReturnValue(new Promise((r) => { resolve = () => r(makeLoginResponse()) }))

    const { container } = renderWithProviders(<LoginPage />, { routerEntries: ['/login'] })

    await user.type(screen.getByRole('textbox'), 'admin')
    await user.type(getPasswordInput(container), 'secret')
    await user.click(screen.getByRole('button', { name: /sign in/i }))

    expect(screen.getByRole('button', { name: /signing in/i })).toBeDisabled()
    resolve()
  })

  it('calls login with credentials on success', async () => {
    const user = userEvent.setup()
    mockLogin.mockResolvedValue(makeLoginResponse())

    const { container } = renderWithProviders(<LoginPage />, { routerEntries: ['/login'] })

    await user.type(screen.getByRole('textbox'), 'admin')
    await user.type(getPasswordInput(container), 'secret')
    await user.click(screen.getByRole('button', { name: /sign in/i }))

    await waitFor(() => expect(mockLogin).toHaveBeenCalledWith('admin', 'secret'))
  })

  it('shows error message on login failure', async () => {
    const user = userEvent.setup()
    mockLogin.mockRejectedValue(new Error('Unauthorized'))

    const { container } = renderWithProviders(<LoginPage />, { routerEntries: ['/login'] })

    await user.type(screen.getByRole('textbox'), 'bad')
    await user.type(getPasswordInput(container), 'wrong')
    await user.click(screen.getByRole('button', { name: /sign in/i }))

    await waitFor(() =>
      expect(screen.getByText(/invalid username or password/i)).toBeInTheDocument(),
    )
  })
})
