import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { AuthProvider } from './auth/AuthContext'
import { ProtectedRoute } from './auth/ProtectedRoute'
import { LoginPage } from './auth/LoginPage'
import { Layout } from './components/Layout'
import { UsersListPage } from './pages/UsersListPage'
import { UserCreatePage } from './pages/UserCreatePage'
import { UserEditPage } from './pages/UserEditPage'
import { DataSourcesListPage } from './pages/DataSourcesListPage'
import { DataSourceCreatePage } from './pages/DataSourceCreatePage'
import { DataSourceEditPage } from './pages/DataSourceEditPage'
import { DataSourceCatalogPage } from './pages/DataSourceCatalogPage'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 30_000,
    },
  },
})

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <BrowserRouter>
          <Routes>
            <Route path="/login" element={<LoginPage />} />

            <Route element={<ProtectedRoute />}>
              <Route element={<Layout />}>
                <Route index element={<UsersListPage />} />
                <Route path="users/create" element={<UserCreatePage />} />
                <Route path="users/:id/edit" element={<UserEditPage />} />
                <Route path="datasources" element={<DataSourcesListPage />} />
                <Route path="datasources/create" element={<DataSourceCreatePage />} />
                <Route path="datasources/:id/edit" element={<DataSourceEditPage />} />
                <Route path="datasources/:id/catalog" element={<DataSourceCatalogPage />} />
              </Route>
            </Route>

            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </BrowserRouter>
      </AuthProvider>
    </QueryClientProvider>
  )
}
