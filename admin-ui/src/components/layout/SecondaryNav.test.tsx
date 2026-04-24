import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SecondaryNav, type SectionDef } from './SecondaryNav'

const SECTIONS: SectionDef[] = [
  { id: 'details', label: 'Details', group: 'Configuration' },
  { id: 'users', label: 'Users', group: 'Access' },
  { id: 'roles', label: 'Roles', group: 'Access' },
  { id: 'activity', label: 'Activity', group: 'History' },
]

describe('SecondaryNav', () => {
  it('renders every section as a button', () => {
    render(
      <SecondaryNav
        ariaLabel="Test sections"
        sections={SECTIONS}
        active="details"
        onSelect={() => {}}
      />,
    )
    for (const s of SECTIONS) {
      expect(screen.getByRole('button', { name: s.label })).toBeInTheDocument()
    }
  })

  it('marks the active section with aria-current="page"', () => {
    render(
      <SecondaryNav
        ariaLabel="Test sections"
        sections={SECTIONS}
        active="users"
        onSelect={() => {}}
      />,
    )
    expect(screen.getByRole('button', { name: 'Users' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(screen.getByRole('button', { name: 'Details' })).not.toHaveAttribute(
      'aria-current',
    )
  })

  it('renders group labels when ≥1 group is defined', () => {
    render(
      <SecondaryNav
        ariaLabel="Test sections"
        sections={SECTIONS}
        active="details"
        onSelect={() => {}}
      />,
    )
    expect(screen.getByText('Configuration')).toBeInTheDocument()
    expect(screen.getByText('Access')).toBeInTheDocument()
    expect(screen.getByText('History')).toBeInTheDocument()
  })

  it('renders a flat list when no groups are defined', () => {
    const flat: SectionDef[] = [
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
    ]
    render(
      <SecondaryNav ariaLabel="flat" sections={flat} active="a" onSelect={() => {}} />,
    )
    expect(screen.queryByText('Configuration')).toBeNull()
    expect(screen.getByRole('button', { name: 'A' })).toBeInTheDocument()
  })

  it('calls onSelect with the clicked section id', async () => {
    const user = userEvent.setup()
    const onSelect = vi.fn()
    render(
      <SecondaryNav
        ariaLabel="Test sections"
        sections={SECTIONS}
        active="details"
        onSelect={onSelect}
      />,
    )
    await user.click(screen.getByRole('button', { name: 'Users' }))
    expect(onSelect).toHaveBeenCalledWith('users')
  })

  it('uses a <nav> landmark with the provided aria-label', () => {
    render(
      <SecondaryNav
        ariaLabel="My Sections"
        sections={SECTIONS}
        active="details"
        onSelect={() => {}}
      />,
    )
    expect(screen.getByRole('navigation', { name: 'My Sections' })).toBeInTheDocument()
  })

  it('renders no indicator when none is provided', () => {
    render(
      <SecondaryNav
        ariaLabel="Test sections"
        sections={SECTIONS}
        active="details"
        onSelect={() => {}}
      />,
    )
    for (const s of SECTIONS) {
      expect(screen.queryByTestId(`section-indicator-${s.id}`)).toBeNull()
    }
  })

  it('renders a red count pill when a section has an indicator', () => {
    const sections: SectionDef[] = [
      { id: 'details', label: 'Details' },
      {
        id: 'coverage',
        label: 'Anchor coverage',
        indicator: { tone: 'red', label: '3', ariaLabel: '3 broken anchor entries' },
      },
    ]
    render(
      <SecondaryNav
        ariaLabel="Test sections"
        sections={sections}
        active="details"
        onSelect={() => {}}
      />,
    )
    const indicator = screen.getByTestId('section-indicator-coverage')
    expect(indicator).toBeInTheDocument()
    expect(indicator.textContent).toBe('3')
    expect(indicator).toHaveAttribute('aria-label', '3 broken anchor entries')
    // Red tone class is applied.
    expect(indicator.className).toMatch(/bg-red-100/)
  })
})
