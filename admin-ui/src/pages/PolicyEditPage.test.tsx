import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, fireEvent } from '@testing-library/react'
import { Route, Routes } from 'react-router-dom'
import { renderWithProviders } from '../test/test-utils'
import { PolicyEditPage } from './PolicyEditPage'
import { makePolicy } from '../test/factories'

vi.mock('../api/policies', () => ({
  getPolicy: vi.fn(),
  updatePolicy: vi.fn(),
}))

import { getPolicy, updatePolicy } from '../api/policies'

const mockGetPolicy = getPolicy as ReturnType<typeof vi.fn>
const mockUpdatePolicy = updatePolicy as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
})

function renderEditPage(policyId = 'p-1') {
  return renderWithProviders(
    <Routes>
      <Route path="/policies/:id/edit" element={<PolicyEditPage />} />
    </Routes>,
    { authenticated: true, routerEntries: [`/policies/${policyId}/edit`] },
  )
}

describe('PolicyEditPage', () => {
  it('shows loading state while fetching', () => {
    mockGetPolicy.mockReturnValue(new Promise(() => {})) // never resolves
    renderEditPage()
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows error with "Go back" when policy not found', async () => {
    mockGetPolicy.mockRejectedValue(new Error('not found'))
    renderEditPage()
    await waitFor(() => expect(screen.getByText(/policy not found/i)).toBeInTheDocument())
    expect(screen.getByText(/go back/i)).toBeInTheDocument()
  })

  it('renders form pre-populated with policy data', async () => {
    const policy = makePolicy({ id: 'p-1', name: 'row-filter', policy_type: 'row_filter', version: 2 })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue('row-filter')).toBeInTheDocument())
    expect(screen.getByText(/version 2/i)).toBeInTheDocument()
  })

  it('renders the read-only assignments section', async () => {
    const policy = makePolicy({ id: 'p-1', assignments: [] })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => expect(screen.getByText('Assignments')).toBeInTheDocument())
    expect(screen.getByText('No assignments yet.')).toBeInTheDocument()
  })

  it('renders the "View as code" section', async () => {
    const policy = makePolicy({ id: 'p-1' })
    mockGetPolicy.mockResolvedValue(policy)
    renderEditPage()
    await waitFor(() => expect(screen.getByText('View as code')).toBeInTheDocument())
  })

  it('calls updatePolicy with correct version on submit', async () => {
    const policy = makePolicy({ id: 'p-1', name: 'my-policy', policy_type: 'row_filter', version: 3 })
    mockGetPolicy.mockResolvedValue(policy)
    mockUpdatePolicy.mockResolvedValue({ ...policy, version: 4 })

    const { container } = renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue('my-policy')).toBeInTheDocument())

    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() => expect(mockUpdatePolicy).toHaveBeenCalled())
    expect(mockUpdatePolicy.mock.calls[0][0]).toBe('p-1')
    expect(mockUpdatePolicy.mock.calls[0][1].version).toBe(3)
    expect(mockUpdatePolicy.mock.calls[0][1].policy_type).toBe('row_filter')
  })

  it('shows conflict message on 409 response', async () => {
    const policy = makePolicy({ id: 'p-1', version: 1 })
    mockGetPolicy.mockResolvedValue(policy)
    mockUpdatePolicy.mockRejectedValue({ response: { status: 409, data: { error: 'conflict' } } })

    const { container } = renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue(policy.name)).toBeInTheDocument())

    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() =>
      expect(screen.getByText(/modified by someone else/i)).toBeInTheDocument(),
    )
  })
})
