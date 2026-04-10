import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Route, Routes } from 'react-router-dom'
import { renderWithProviders } from '../test/test-utils'
import { Layout } from './Layout'
import { makeVersionInfo } from '../test/factories'

vi.mock('../api/version', () => ({
  getVersion: vi.fn(),
}))

import { getVersion } from '../api/version'
const mockGetVersion = getVersion as ReturnType<typeof vi.fn>

function WrappedLayout() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route path="/" element={<div>Users Page</div>} />
        <Route path="/datasources" element={<div>Data Sources Page</div>} />
        <Route path="/login" element={<div>Login Page</div>} />
      </Route>
    </Routes>
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  mockGetVersion.mockResolvedValue(makeVersionInfo())
})

describe('Layout', () => {
  it('renders nav links for Users and Data Sources', () => {
    renderWithProviders(<WrappedLayout />, { authenticated: true, routerEntries: ['/'] })
    expect(screen.getByRole('link', { name: /users/i })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /data sources/i })).toBeInTheDocument()
  })

  it('renders the outlet content', () => {
    renderWithProviders(<WrappedLayout />, { authenticated: true, routerEntries: ['/'] })
    expect(screen.getByText('Users Page')).toBeInTheDocument()
  })

  it('shows the logged-in username', () => {
    renderWithProviders(<WrappedLayout />, { authenticated: true, routerEntries: ['/'] })
    expect(screen.getByText('@admin')).toBeInTheDocument()
  })

  it('sign out button clears auth and navigates to /login', async () => {
    const user = userEvent.setup()
    renderWithProviders(<WrappedLayout />, { authenticated: true, routerEntries: ['/'] })

    await user.click(screen.getByRole('button', { name: /sign out/i }))

    expect(screen.getByText('Login Page')).toBeInTheDocument()
    expect(localStorage.getItem('token')).toBeNull()
  })

  it('displays current version in sidebar', async () => {
    mockGetVersion.mockResolvedValue(makeVersionInfo({ current: '0.13.0' }))
    renderWithProviders(<WrappedLayout />, { authenticated: true, routerEntries: ['/'] })
    await waitFor(() => expect(screen.getByText('v0.13.0')).toBeInTheDocument())
  })
})
