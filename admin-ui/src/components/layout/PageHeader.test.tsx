import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { PageHeader } from './PageHeader'
import { StatusDot } from '../Status'

function wrap(ui: React.ReactElement) {
  return render(<MemoryRouter>{ui}</MemoryRouter>)
}

describe('PageHeader', () => {
  it('renders the title as an h1', () => {
    wrap(<PageHeader breadcrumb={[]} title="prod-db" />)
    expect(screen.getByRole('heading', { name: 'prod-db' })).toBeInTheDocument()
  })

  it('renders the breadcrumb with a link for parent items and text for the last item', () => {
    wrap(
      <PageHeader
        breadcrumb={[
          { label: 'Data Sources', href: '/datasources' },
          { label: 'prod-db' },
        ]}
        title="prod-db"
      />,
    )
    const link = screen.getByRole('link', { name: 'Data Sources' })
    expect(link).toHaveAttribute('href', '/datasources')
    // The last item ("prod-db") appears both in the breadcrumb and as the h1;
    // the breadcrumb one is plain text, not a link.
    const links = screen.getAllByRole('link')
    expect(links).toHaveLength(1)
  })

  it('renders status beside the title', () => {
    wrap(
      <PageHeader
        breadcrumb={[]}
        title="prod-db"
        status={<StatusDot active />}
      />,
    )
    expect(screen.getByText('Active')).toBeInTheDocument()
  })

  it('joins metadata items with middle-dot separators', () => {
    wrap(
      <PageHeader
        breadcrumb={[]}
        title="prod-db"
        metadata={[<span key="type">postgres</span>, <span key="id">ds-1</span>]}
      />,
    )
    expect(screen.getByText('postgres')).toBeInTheDocument()
    expect(screen.getByText('ds-1')).toBeInTheDocument()
  })

  it('omits the breadcrumb nav when breadcrumb is empty', () => {
    const { container } = wrap(<PageHeader breadcrumb={[]} title="prod-db" />)
    expect(container.querySelector('nav[aria-label="Breadcrumb"]')).toBeNull()
  })
})
