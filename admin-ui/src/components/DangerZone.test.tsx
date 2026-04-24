import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { DangerZone, DangerRow } from './DangerZone'

describe('DangerZone', () => {
  it('renders the default title', () => {
    render(
      <DangerZone>
        <p>content</p>
      </DangerZone>,
    )
    expect(screen.getByRole('heading', { name: 'Danger zone' })).toBeInTheDocument()
  })

  it('allows a custom title', () => {
    render(
      <DangerZone title="Danger!">
        <p>content</p>
      </DangerZone>,
    )
    expect(screen.getByRole('heading', { name: 'Danger!' })).toBeInTheDocument()
  })

  it('renders child rows', () => {
    render(
      <DangerZone>
        <DangerRow title="Delete" body="Permanently remove" action={<button>Delete…</button>} />
      </DangerZone>,
    )
    expect(screen.getByRole('heading', { name: 'Delete' })).toBeInTheDocument()
    expect(screen.getByText(/permanently remove/i)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Delete…' })).toBeInTheDocument()
  })
})
