import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ModalShell } from './ModalShell'

describe('ModalShell', () => {
  it('renders the title in a dialog landmark', () => {
    render(
      <ModalShell title="Delete thing" onClose={() => {}}>
        <button>Confirm</button>
      </ModalShell>,
    )
    const dialog = screen.getByRole('dialog', { name: 'Delete thing' })
    expect(dialog).toBeInTheDocument()
    expect(dialog).toHaveAttribute('aria-modal', 'true')
  })

  it('calls onClose on Escape', () => {
    const onClose = vi.fn()
    render(
      <ModalShell title="t" onClose={onClose}>
        <button>ok</button>
      </ModalShell>,
    )
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalled()
  })

  it('calls onClose when the backdrop is clicked', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()
    render(
      <ModalShell title="t" onClose={onClose}>
        <button>ok</button>
      </ModalShell>,
    )
    await user.click(screen.getByRole('dialog'))
    expect(onClose).toHaveBeenCalled()
  })

  it('does not close when clicking inside the dialog content', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()
    render(
      <ModalShell title="t" onClose={onClose}>
        <button>ok</button>
      </ModalShell>,
    )
    await user.click(screen.getByRole('button', { name: 'ok' }))
    expect(onClose).not.toHaveBeenCalled()
  })

  it('focuses the first focusable element on mount', async () => {
    render(
      <ModalShell title="t" onClose={() => {}}>
        <input placeholder="first" />
        <button>second</button>
      </ModalShell>,
    )
    expect(screen.getByPlaceholderText('first')).toHaveFocus()
  })
})
