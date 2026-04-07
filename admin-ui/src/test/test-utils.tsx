import { type ReactElement } from 'react'
import { render, type RenderOptions } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router-dom'
import { AuthProvider } from '../auth/AuthContext'

interface RenderWithProvidersOptions extends Omit<RenderOptions, 'wrapper'> {
  /** Seed localStorage with a test token + user so AuthProvider picks them up */
  authenticated?: boolean
  /** Initial entries for MemoryRouter */
  routerEntries?: string[]
}

export function renderWithProviders(
  ui: ReactElement,
  { authenticated = false, routerEntries = ['/'], ...options }: RenderWithProvidersOptions = {},
) {
  if (authenticated) {
    localStorage.setItem('token', 'test-token')
    localStorage.setItem('user', JSON.stringify({
      id: 'user-1',
      username: 'admin',
      is_admin: true,
      is_active: true,
      email: null,
      display_name: null,
      last_login_at: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    }))
  }

  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  })

  function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={routerEntries}>
          <AuthProvider>{children}</AuthProvider>
        </MemoryRouter>
      </QueryClientProvider>
    )
  }

  return { ...render(ui, { wrapper: Wrapper, ...options }), queryClient }
}
