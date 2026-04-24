import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { StatusDot, StatusChip } from './Status'

describe('StatusDot', () => {
  it('renders the active label when active', () => {
    render(<StatusDot active />)
    expect(screen.getByText('Active')).toBeInTheDocument()
  })

  it('renders the inactive label when not active', () => {
    render(<StatusDot active={false} />)
    expect(screen.getByText('Inactive')).toBeInTheDocument()
  })

  it('supports custom labels for binary states other than active/inactive', () => {
    render(<StatusDot active activeLabel="Enabled" inactiveLabel="Disabled" />)
    expect(screen.getByText('Enabled')).toBeInTheDocument()
  })

  it('always pairs the dot with visible text for a11y (never color-only)', () => {
    const { container } = render(<StatusDot active />)
    const dot = container.querySelector('span[aria-hidden]')
    expect(dot).not.toBeNull()
    expect(screen.getByText('Active')).toBeInTheDocument()
  })
})

describe('StatusChip', () => {
  it('renders the label text', () => {
    render(<StatusChip label="row_filter" tone="blue" />)
    expect(screen.getByText('row_filter')).toBeInTheDocument()
  })

  it('accepts string and ReactNode labels', () => {
    render(<StatusChip label={<span data-testid="custom">custom</span>} />)
    expect(screen.getByTestId('custom')).toBeInTheDocument()
  })
})
