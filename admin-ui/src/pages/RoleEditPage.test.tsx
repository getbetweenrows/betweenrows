import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, fireEvent, within } from '@testing-library/react'
import { Route, Routes } from 'react-router-dom'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { RoleEditPage } from './RoleEditPage'
import type { RoleDetail } from '../types/role'

vi.mock('../api/roles', () => ({
  getRole: vi.fn(),
  updateRole: vi.fn(),
  deleteRole: vi.fn(),
  getEffectiveMembers: vi.fn(),
  listRoleMembers: vi.fn(),
  addRoleMember: vi.fn(),
  removeRoleMember: vi.fn(),
  listRoleInheritance: vi.fn(),
  addRoleParent: vi.fn(),
  removeRoleParent: vi.fn(),
}))

vi.mock('../api/adminAudit', () => ({
  listAdminAudit: vi.fn().mockResolvedValue({
    data: [],
    total: 0,
    page: 1,
    page_size: 20,
  }),
}))

vi.mock('react-hot-toast', () => ({
  default: { success: vi.fn(), error: vi.fn() },
}))

import { deleteRole, getRole, updateRole } from '../api/roles'
import toast from 'react-hot-toast'

const mockGetRole = getRole as ReturnType<typeof vi.fn>
const mockUpdateRole = updateRole as ReturnType<typeof vi.fn>
const mockDeleteRole = deleteRole as ReturnType<typeof vi.fn>
const mockToastSuccess = toast.success as ReturnType<typeof vi.fn>

function makeRoleDetail(overrides: Partial<RoleDetail> = {}): RoleDetail {
  return {
    id: 'r-1',
    name: 'admin',
    description: 'Full admin access',
    is_active: true,
    direct_member_count: 3,
    effective_member_count: 5,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    members: [],
    parent_roles: [],
    child_roles: [],
    policy_assignments: [],
    datasource_access: [],
    ...overrides,
  }
}

beforeEach(() => {
  vi.clearAllMocks()
})

function renderEditPage(path = '/roles/r-1/edit') {
  return renderWithProviders(
    <Routes>
      <Route path="/roles/:id/edit" element={<RoleEditPage />} />
    </Routes>,
    { authenticated: true, routerEntries: [path] },
  )
}

describe('RoleEditPage', () => {
  it('shows loading state while fetching', () => {
    mockGetRole.mockReturnValue(new Promise(() => {}))
    renderEditPage()
    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('shows "Role not found" on error', async () => {
    mockGetRole.mockRejectedValue(new Error('nope'))
    renderEditPage()
    await waitFor(() => expect(screen.getByText(/role not found/i)).toBeInTheDocument())
  })

  it('renders header with breadcrumb, title, and member counts', async () => {
    mockGetRole.mockResolvedValue(makeRoleDetail({ name: 'admin' }))
    renderEditPage()
    await waitFor(() =>
      expect(screen.getByRole('heading', { name: 'admin' })).toBeInTheDocument(),
    )
    expect(screen.getByText(/3 direct \/ 5 effective members/)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Roles' })).toHaveAttribute('href', '/roles')
  })

  it('renders the Details form by default', async () => {
    mockGetRole.mockResolvedValue(makeRoleDetail({ name: 'admin' }))
    renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue('admin')).toBeInTheDocument())
    expect(screen.getByRole('button', { name: /^Details$/ })).toHaveAttribute(
      'aria-current',
      'page',
    )
  })

  it('honors ?section=membership on load', async () => {
    mockGetRole.mockResolvedValue(makeRoleDetail())
    renderEditPage('/roles/r-1/edit?section=membership')
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^Membership$/ })).toHaveAttribute(
        'aria-current',
        'page',
      ),
    )
  })

  it('switches to Access grants section and shows both datasource access and policy assignments', async () => {
    const user = userEvent.setup()
    mockGetRole.mockResolvedValue(
      makeRoleDetail({
        datasource_access: [
          { datasource_id: 'ds-1', datasource_name: 'prod-db', source: 'direct' },
        ],
        policy_assignments: [
          {
            policy_name: 'mask-ssn',
            datasource_name: 'prod-db',
            source: 'direct',
            priority: 100,
          },
        ],
      }),
    )
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue('admin'))

    await user.click(screen.getByRole('button', { name: /^Access grants$/ }))

    await waitFor(() =>
      expect(screen.getByRole('heading', { name: /data source access/i })).toBeInTheDocument(),
    )
    expect(screen.getByRole('heading', { name: /policy assignments/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'prod-db' })).toBeInTheDocument()
    expect(screen.getByText('mask-ssn')).toBeInTheDocument()
  })

  it('calls updateRole on Details save and shows a toast (no auto-navigate)', async () => {
    mockGetRole.mockResolvedValue(makeRoleDetail({ name: 'admin' }))
    mockUpdateRole.mockResolvedValue({})

    const { container } = renderEditPage()
    await waitFor(() => expect(screen.getByDisplayValue('admin')).toBeInTheDocument())

    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() => expect(mockUpdateRole).toHaveBeenCalled())
    expect(mockUpdateRole.mock.calls[0][0]).toBe('r-1')
    await waitFor(() => expect(mockToastSuccess).toHaveBeenCalledWith('Saved'))
    // Still on the edit page.
    expect(screen.getByDisplayValue('admin')).toBeInTheDocument()
  })
})

describe('RoleEditPage — danger zone', () => {
  it('opens a typed-name delete modal (no native confirm)', async () => {
    const user = userEvent.setup()
    const confirmSpy = vi.spyOn(window, 'confirm')
    mockGetRole.mockResolvedValue(makeRoleDetail({ name: 'admin' }))
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue('admin'))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    expect(confirmSpy).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: /delete admin\?/i })).toBeInTheDocument()
    confirmSpy.mockRestore()
  })

  it('offers "Deactivate instead" when the role is active', async () => {
    const user = userEvent.setup()
    mockGetRole.mockResolvedValue(makeRoleDetail({ name: 'admin', is_active: true }))
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue('admin'))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    expect(
      screen.getByRole('button', { name: /deactivate instead/i }),
    ).toBeInTheDocument()
  })

  it('hides "Deactivate instead" when the role is already inactive', async () => {
    const user = userEvent.setup()
    mockGetRole.mockResolvedValue(makeRoleDetail({ is_active: false }))
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue('admin'))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    expect(screen.queryByRole('button', { name: /deactivate instead/i })).toBeNull()
  })

  it('Deactivate-instead path calls updateRole with is_active: false (no delete)', async () => {
    const user = userEvent.setup()
    mockGetRole.mockResolvedValue(makeRoleDetail({ name: 'admin', is_active: true }))
    mockUpdateRole.mockResolvedValue({})

    renderEditPage()
    await waitFor(() => screen.getByDisplayValue('admin'))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    await user.click(screen.getByRole('button', { name: /deactivate instead/i }))

    await waitFor(() => expect(mockUpdateRole).toHaveBeenCalled())
    expect(mockUpdateRole.mock.calls[0][0]).toBe('r-1')
    expect(mockUpdateRole.mock.calls[0][1]).toEqual({ is_active: false })
    expect(mockDeleteRole).not.toHaveBeenCalled()
  })

  it('Delete button is disabled until the typed name matches', async () => {
    const user = userEvent.setup()
    mockGetRole.mockResolvedValue(makeRoleDetail({ name: 'admin', is_active: false }))
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue('admin'))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    const dialog = screen.getByRole('dialog')
    const submit = within(dialog).getByRole('button', { name: /^delete$/i })
    expect(submit).toBeDisabled()

    await user.type(within(dialog).getByRole('textbox'), 'admin')
    expect(submit).not.toBeDisabled()
  })

  it('typed-name delete calls deleteRole', async () => {
    const user = userEvent.setup()
    mockGetRole.mockResolvedValue(makeRoleDetail({ name: 'admin', is_active: false }))
    mockDeleteRole.mockResolvedValue(undefined)
    renderEditPage()
    await waitFor(() => screen.getByDisplayValue('admin'))

    await user.click(screen.getByRole('button', { name: /delete…/i }))
    const dialog = screen.getByRole('dialog')
    await user.type(within(dialog).getByRole('textbox'), 'admin')
    await user.click(within(dialog).getByRole('button', { name: /^delete$/i }))

    await waitFor(() => expect(mockDeleteRole).toHaveBeenCalled())
    expect(mockDeleteRole.mock.calls[0][0]).toBe('r-1')
  })
})
