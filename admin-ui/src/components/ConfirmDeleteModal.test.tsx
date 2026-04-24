import { describe, it, expect, vi } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { ConfirmDeleteModal } from './ConfirmDeleteModal'

describe('ConfirmDeleteModal', () => {
  it('renders a dialog titled "Delete <resourceName>?"', () => {
    renderWithProviders(
      <ConfirmDeleteModal
        resourceName="my-thing"
        consequences={<li>everything</li>}
        onDelete={() => {}}
        deletePending={false}
        onClose={() => {}}
      />,
    )
    expect(screen.getByRole('dialog', { name: /delete my-thing\?/i })).toBeInTheDocument()
  })

  it('Delete button is disabled until the typed name exactly matches', async () => {
    const user = userEvent.setup()
    const onDelete = vi.fn()
    renderWithProviders(
      <ConfirmDeleteModal
        resourceName="exact-name"
        consequences={<li>everything</li>}
        onDelete={onDelete}
        deletePending={false}
        onClose={() => {}}
      />,
    )
    const submit = screen.getByRole('button', { name: /^delete$/i })
    expect(submit).toBeDisabled()

    const input = screen.getByRole('textbox')
    await user.type(input, 'wrong')
    expect(submit).toBeDisabled()

    await user.clear(input)
    await user.type(input, 'exact-name')
    expect(submit).not.toBeDisabled()

    await user.click(submit)
    expect(onDelete).toHaveBeenCalled()
  })

  it('renders the soft-delete escape hatch when provided', async () => {
    const user = userEvent.setup()
    const softConfirm = vi.fn()
    renderWithProviders(
      <ConfirmDeleteModal
        resourceName="x"
        consequences={<li>everything</li>}
        softDelete={{
          label: 'Deactivate instead',
          pendingLabel: 'Deactivating…',
          explanation: 'Preserves the resource',
          onConfirm: softConfirm,
          pending: false,
        }}
        onDelete={() => {}}
        deletePending={false}
        onClose={() => {}}
      />,
    )
    const btn = screen.getByRole('button', { name: /deactivate instead/i })
    expect(btn).toBeInTheDocument()
    await user.click(btn)
    expect(softConfirm).toHaveBeenCalled()
  })

  it('omits the soft-delete escape hatch when not provided', () => {
    renderWithProviders(
      <ConfirmDeleteModal
        resourceName="x"
        consequences={<li>everything</li>}
        onDelete={() => {}}
        deletePending={false}
        onClose={() => {}}
      />,
    )
    expect(screen.queryByRole('button', { name: /instead/i })).toBeNull()
  })

  it('shows an error message when provided', () => {
    renderWithProviders(
      <ConfirmDeleteModal
        resourceName="x"
        consequences={<li>everything</li>}
        onDelete={() => {}}
        deletePending={false}
        onClose={() => {}}
        error="Something went wrong"
      />,
    )
    expect(screen.getByText(/something went wrong/i)).toBeInTheDocument()
  })
})
