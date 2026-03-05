import { describe, it, expect, vi } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { UserForm } from './UserForm'

// UserForm's Field component does not associate labels via htmlFor.
// Use positional getByRole queries and container selectors.
function getPasswordInput(container: HTMLElement) {
  return container.querySelector('input[type="password"]') as HTMLInputElement
}

describe('UserForm – create mode', () => {
  it('shows username and password fields', () => {
    const { container } = renderWithProviders(
      <UserForm mode="create" onSubmit={vi.fn()} onCancel={vi.fn()} />,
    )
    // create mode: textboxes are [username, tenant, email, display_name]
    const textboxes = screen.getAllByRole('textbox')
    expect(textboxes.length).toBeGreaterThanOrEqual(2) // at least username + tenant
    expect(getPasswordInput(container)).toBeTruthy()
    expect(screen.getByRole('button', { name: /create user/i })).toBeInTheDocument()
  })

  it('does not show Active toggle in create mode', () => {
    renderWithProviders(
      <UserForm mode="create" onSubmit={vi.fn()} onCancel={vi.fn()} />,
    )
    // Only the Admin checkbox is shown — not Active
    const checkboxes = screen.getAllByRole('checkbox')
    expect(checkboxes).toHaveLength(1)
  })

  it('calls onSubmit with correct payload', async () => {
    const user = userEvent.setup()
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    const { container } = renderWithProviders(
      <UserForm mode="create" onSubmit={onSubmit} onCancel={vi.fn()} />,
    )

    // create mode textboxes: [0]=username, [1]=tenant, [2]=email, [3]=displayName
    const textboxes = screen.getAllByRole('textbox')
    await user.type(textboxes[0], 'alice')   // username
    await user.type(getPasswordInput(container), 'Test@123!') // password
    await user.type(textboxes[1], 'acme')   // tenant

    // Submit via button
    await user.click(screen.getByRole('button', { name: /create user/i }))

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        username: 'alice',
        password: 'Test@123!',
        tenant: 'acme',
        is_admin: false,
      }),
    )
  })

  it('fires onCancel when Cancel is clicked', async () => {
    const user = userEvent.setup()
    const onCancel = vi.fn()
    renderWithProviders(
      <UserForm mode="create" onSubmit={vi.fn()} onCancel={onCancel} />,
    )
    await user.click(screen.getByRole('button', { name: /cancel/i }))
    expect(onCancel).toHaveBeenCalledOnce()
  })
})

describe('UserForm – edit mode', () => {
  it('hides username and password fields', () => {
    const { container } = renderWithProviders(
      <UserForm
        mode="edit"
        initialValues={{ username: 'bob', tenant: 'acme', is_admin: false, is_active: true }}
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    // In edit mode, no password field
    expect(getPasswordInput(container)).toBeNull()
    // In edit mode, the username textbox is NOT rendered; textboxes are [tenant, email, displayName]
    const textboxes = screen.getAllByRole('textbox')
    // No input should have the username value in edit mode (username is not shown)
    expect(textboxes.some((t) => (t as HTMLInputElement).value === 'bob')).toBe(false)
  })

  it('shows Active toggle in edit mode', () => {
    renderWithProviders(
      <UserForm
        mode="edit"
        initialValues={{ tenant: 'acme', is_active: true }}
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    // Edit mode has Admin + Active checkboxes
    const checkboxes = screen.getAllByRole('checkbox')
    expect(checkboxes).toHaveLength(2)
  })

  it('calls onSubmit with UpdateUserPayload shape (no username/password)', async () => {
    const user = userEvent.setup()
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    const { container } = renderWithProviders(
      <UserForm
        mode="edit"
        initialValues={{ tenant: 'acme', is_admin: false, is_active: true }}
        onSubmit={onSubmit}
        onCancel={vi.fn()}
      />,
    )
    // No password field in edit mode
    expect(getPasswordInput(container)).toBeNull()

    await user.click(screen.getByRole('button', { name: /save changes/i }))

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ is_admin: false, is_active: true }),
    )
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty('username')
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty('password')
  })

  it('shows error message when error prop is set', () => {
    renderWithProviders(
      <UserForm
        mode="edit"
        initialValues={{}}
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
        error="Something went wrong"
      />,
    )
    expect(screen.getByText('Something went wrong')).toBeInTheDocument()
  })
})
