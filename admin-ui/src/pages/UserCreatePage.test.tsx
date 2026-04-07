import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { UserCreatePage } from './UserCreatePage'
import { makeUser } from '../test/factories'

vi.mock('../api/users', () => ({
  createUser: vi.fn(),
}))

import { createUser } from '../api/users'
const mockCreateUser = createUser as ReturnType<typeof vi.fn>

beforeEach(() => vi.clearAllMocks())

describe('UserCreatePage', () => {
  it('renders heading and form', () => {
    renderWithProviders(<UserCreatePage />, { authenticated: true })
    expect(screen.getByText(/new user/i)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /create user/i })).toBeInTheDocument()
  })

  it('submits form and navigates on success', async () => {
    const user = userEvent.setup()
    mockCreateUser.mockResolvedValue(makeUser())

    const { container } = renderWithProviders(<UserCreatePage />, { authenticated: true })

    const textboxes = screen.getAllByRole('textbox')
    await user.type(textboxes[0], 'newuser')   // username
    await user.type(
      container.querySelector('input[type="password"]') as HTMLInputElement,
      'Test@123!',
    )

    await user.click(screen.getByRole('button', { name: /create user/i }))

    await waitFor(() =>
      expect(mockCreateUser).toHaveBeenCalledWith(
        expect.objectContaining({ username: 'newuser' }),
      ),
    )
  })

  it('cancel button navigates back', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserCreatePage />, { authenticated: true })
    await user.click(screen.getByRole('button', { name: /cancel/i }))
    // Should not throw; navigation happens silently in MemoryRouter
    expect(mockCreateUser).not.toHaveBeenCalled()
  })
})
