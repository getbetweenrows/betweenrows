import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { SectionPane } from './SectionPane'

describe('SectionPane', () => {
  it('hides children with Tailwind hidden when inactive', () => {
    const { container } = render(
      <SectionPane active={false} testId="pane">
        <p>body</p>
      </SectionPane>,
    )
    const pane = container.querySelector('[data-testid="pane"]')
    expect(pane).toHaveClass('hidden')
    expect(pane).toHaveAttribute('aria-hidden', 'true')
  })

  it('applies the configured width when active', () => {
    const { container } = render(
      <SectionPane active width="narrow" testId="pane">
        <p>body</p>
      </SectionPane>,
    )
    const pane = container.querySelector('[data-testid="pane"]')
    expect(pane).toHaveClass('max-w-2xl')
    expect(pane).not.toHaveClass('hidden')
    expect(pane).toHaveAttribute('aria-hidden', 'false')
  })

  it('defaults width to wide (max-w-5xl)', () => {
    const { container } = render(
      <SectionPane active testId="pane">
        <p>body</p>
      </SectionPane>,
    )
    expect(container.querySelector('[data-testid="pane"]')).toHaveClass('max-w-5xl')
  })

  it('keeps children mounted even when hidden (draft preservation)', () => {
    render(
      <SectionPane active={false}>
        <input defaultValue="draft-value" />
      </SectionPane>,
    )
    expect(screen.getByDisplayValue('draft-value')).toBeInTheDocument()
  })
})
