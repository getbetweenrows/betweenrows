import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Routes, Route, useLocation } from 'react-router-dom'
import { useSectionParam } from './useSectionParam'

const VALID = ['details', 'users', 'roles'] as const
type Id = (typeof VALID)[number]

function Harness() {
  const [active, select] = useSectionParam<Id>(VALID, 'details')
  const location = useLocation()
  return (
    <div>
      <p data-testid="active">{active}</p>
      <p data-testid="search">{location.search}</p>
      {VALID.map((id) => (
        <button key={id} onClick={() => select(id)}>
          {id}
        </button>
      ))}
    </div>
  )
}

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/page" element={<Harness />} />
      </Routes>
    </MemoryRouter>,
  )
}

describe('useSectionParam', () => {
  it('returns the fallback when no ?section is present', () => {
    renderAt('/page')
    expect(screen.getByTestId('active').textContent).toBe('details')
  })

  it('returns the parsed id when ?section matches a valid id', () => {
    renderAt('/page?section=users')
    expect(screen.getByTestId('active').textContent).toBe('users')
  })

  it('falls back when ?section is not in validIds', () => {
    renderAt('/page?section=bogus')
    expect(screen.getByTestId('active').textContent).toBe('details')
  })

  it('writes ?section=<id> to the URL on select', async () => {
    const user = userEvent.setup()
    renderAt('/page')
    await user.click(screen.getByRole('button', { name: 'users' }))
    expect(screen.getByTestId('active').textContent).toBe('users')
    expect(screen.getByTestId('search').textContent).toContain('section=users')
  })

  it('preserves other query params when switching sections', async () => {
    const user = userEvent.setup()
    renderAt('/page?focus=a.b.c')
    await user.click(screen.getByRole('button', { name: 'roles' }))
    const search = screen.getByTestId('search').textContent ?? ''
    expect(search).toContain('focus=a.b.c')
    expect(search).toContain('section=roles')
  })
})
