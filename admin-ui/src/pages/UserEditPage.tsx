import { type FormEvent, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { changePassword, getUser, updateUser } from '../api/users'
import { PasswordInput } from '../components/PasswordInput'
import { PasswordStrengthIndicator } from '../components/PasswordStrengthIndicator'
import { validatePassword } from '../utils/passwordValidation'
import { AuditTimeline } from '../components/AuditTimeline'
import { UserAttributeEditor } from '../components/UserAttributeEditor'
import { CopyableId } from '../components/CopyableId'
import type { AttributeValue } from '../types/user'

const inputCls =
  'w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500'

export function UserEditPage() {
  const { id } = useParams<{ id: string }>()
  const userId = id ?? ''
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const { data: user, isLoading } = useQuery({
    queryKey: ['users', userId],
    queryFn: () => getUser(userId),
    enabled: !!userId,
  })

  // Profile fields
  const [email, setEmail] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [isAdmin, setIsAdmin] = useState(false)
  const [isActive, setIsActive] = useState(true)
  const [initialized, setInitialized] = useState(false)

  // Sync form state when user data loads
  if (user && !initialized) {
    setEmail(user.email ?? '')
    setDisplayName(user.display_name ?? '')
    setIsAdmin(user.is_admin)
    setIsActive(user.is_active)
    setInitialized(true)
  }

  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)

  const [pendingAttributes, setPendingAttributes] = useState<Record<string, AttributeValue> | null>(null)

  const [newPassword, setNewPassword] = useState('')
  const [changingPw, setChangingPw] = useState(false)
  const [pwError, setPwError] = useState<string | null>(null)
  const [pwSuccess, setPwSuccess] = useState(false)

  async function handleSubmit(e: FormEvent) {
    e.preventDefault()
    setSaveError(null)
    setSaving(true)
    try {
      await updateUser(userId, {
        is_admin: isAdmin,
        is_active: isActive,
        email: email || undefined,
        display_name: displayName || undefined,
        ...(pendingAttributes !== null ? { attributes: pendingAttributes } : {}),
      })
      await queryClient.invalidateQueries({ queryKey: ['users'] })
      await queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      navigate('/', { replace: true })
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to update user'
      setSaveError(msg)
    } finally {
      setSaving(false)
    }
  }

  async function handlePasswordChange(e: FormEvent) {
    e.preventDefault()
    if (!newPassword) return
    if (!validatePassword(newPassword).valid) {
      setPwError('Password does not meet the requirements below.')
      return
    }
    setPwError(null)
    setPwSuccess(false)
    setChangingPw(true)
    try {
      await changePassword(userId, newPassword)
      setNewPassword('')
      setPwSuccess(true)
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to change password'
      setPwError(msg)
    } finally {
      setChangingPw(false)
    }
  }

  if (isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading…</div>
  }

  if (!user) {
    return <div className="p-6 text-sm text-red-500">User not found.</div>
  }

  return (
    <div className="p-6 space-y-10">
      <section>
        <h1 className="text-xl font-bold text-gray-900 mb-1">Edit user</h1>
        <p className="text-sm text-gray-500">@{user.username}</p>
        <div className="mb-6">
          <CopyableId id={userId} />
        </div>

        <form onSubmit={handleSubmit} className="space-y-5 max-w-lg">
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">Email</label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              className={inputCls}
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">Display name</label>
            <input
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              className={inputCls}
            />
          </div>

          <div>
            <div className="flex gap-6">
              <label className="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  checked={isAdmin}
                  onChange={(e) => setIsAdmin(e.target.checked)}
                  className="rounded"
                />
                Admin
              </label>
              <label className="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  checked={isActive}
                  onChange={(e) => setIsActive(e.target.checked)}
                  className="rounded"
                />
                Active
              </label>
            </div>
            <p className="text-xs text-gray-500 mt-2">
              Admins can manage users, roles, policies, data sources, decision functions, and
              attribute definitions, and can view audit logs. Non-admin users can sign in and
              query data sources they have access to, but cannot access the admin UI.
            </p>
          </div>

          {/* Attributes */}
          <div className="border-t border-gray-200 pt-5">
            <h2 className="text-base font-semibold text-gray-900 mb-1">Attributes</h2>
            <p className="text-sm text-gray-500 mb-4">
              Custom key-value pairs available as {'{'}user.KEY{'}'} in filter and mask expressions,
              and as <code className="text-xs bg-gray-100 px-1 rounded">ctx.session.user.KEY</code> in decision functions.
            </p>
            <UserAttributeEditor
              attributes={pendingAttributes ?? user.attributes}
              onChange={(attrs) => setPendingAttributes(attrs)}
            />
          </div>

          {saveError && <p className="text-sm text-red-600">{saveError}</p>}

          <div className="flex gap-3 pt-2">
            <button
              type="submit"
              disabled={saving}
              className="bg-blue-600 hover:bg-blue-700 disabled:opacity-60 text-white font-medium rounded-lg px-5 py-2 text-sm transition-colors"
            >
              {saving ? 'Saving…' : 'Save changes'}
            </button>
            <button
              type="button"
              onClick={() => navigate('/')}
              className="text-gray-600 hover:text-gray-900 font-medium text-sm px-3 py-2 transition-colors"
            >
              Cancel
            </button>
          </div>
        </form>
      </section>

      {/* Change password */}
      <section className="border-t border-gray-200 pt-8">
        <h2 className="text-base font-semibold text-gray-900 mb-4">Change password</h2>
        <form onSubmit={handlePasswordChange} className="space-y-3">
          <div className="flex gap-3 items-start">
            <div>
              <PasswordInput
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                placeholder="New password"
                required
                className="border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 w-64"
              />
              <PasswordStrengthIndicator password={newPassword} />
            </div>
            <button
              type="submit"
              disabled={changingPw}
              className="bg-gray-800 hover:bg-gray-900 disabled:opacity-60 text-white text-sm font-medium rounded-lg px-4 py-2 transition-colors"
            >
              {changingPw ? 'Saving…' : 'Change'}
            </button>
          </div>
          {pwError && <p className="text-sm text-red-600">{pwError}</p>}
        </form>
        {pwSuccess && <p className="text-sm text-green-600 mt-2">Password updated.</p>}
      </section>

      {/* Activity */}
      <section className="border-t border-gray-200 pt-8">
        <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
        {/* Backend resource_type is "proxy_user" (DB table name) */}
        <AuditTimeline resourceType="proxy_user" resourceId={userId} />
      </section>
    </div>
  )
}
