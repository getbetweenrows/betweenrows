import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { changePassword, getUser, updateUser } from '../api/users'
import { UserForm } from '../components/UserForm'
import { PasswordInput } from '../components/PasswordInput'
import { PasswordStrengthIndicator } from '../components/PasswordStrengthIndicator'
import { validatePassword } from '../utils/passwordValidation'
import { AuditTimeline } from '../components/AuditTimeline'
import { UserAttributeEditor } from '../components/UserAttributeEditor'
import type { AttributeValue, UpdateUserPayload } from '../types/user'

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

  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)

  const [pendingAttributes, setPendingAttributes] = useState<Record<string, AttributeValue> | null>(null)
  const [attrSaving, setAttrSaving] = useState(false)
  const [attrError, setAttrError] = useState<string | null>(null)
  const [attrSuccess, setAttrSuccess] = useState(false)

  const [newPassword, setNewPassword] = useState('')
  const [changingPw, setChangingPw] = useState(false)
  const [pwError, setPwError] = useState<string | null>(null)
  const [pwSuccess, setPwSuccess] = useState(false)

  async function handleUpdate(data: UpdateUserPayload) {
    setSaveError(null)
    setSaving(true)
    try {
      await updateUser(userId, data)
      await queryClient.invalidateQueries({ queryKey: ['users'] })
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

  async function handlePasswordChange(e: React.FormEvent) {
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

  async function handleSaveAttributes() {
    if (pendingAttributes === null) return
    setAttrError(null)
    setAttrSuccess(false)
    setAttrSaving(true)
    try {
      await updateUser(userId, { attributes: pendingAttributes })
      await queryClient.invalidateQueries({ queryKey: ['users'] })
      setPendingAttributes(null)
      setAttrSuccess(true)
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to update attributes'
      setAttrError(msg)
    } finally {
      setAttrSaving(false)
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
      {/* Edit profile */}
      <section>
        <h1 className="text-xl font-bold text-gray-900 mb-1">Edit user</h1>
        <p className="text-sm text-gray-500 mb-6">@{user.username}</p>

        <UserForm
          mode="edit"
          initialValues={{
            username: user.username,
            tenant: user.tenant,
            is_admin: user.is_admin,
            is_active: user.is_active,
            email: user.email ?? '',
            display_name: user.display_name ?? '',
          }}
          onSubmit={(d) => handleUpdate(d as UpdateUserPayload)}
          onCancel={() => navigate('/')}
          loading={saving}
          error={saveError}
        />
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

      {/* User Attributes */}
      <section className="border-t border-gray-200 pt-8">
        <h2 className="text-base font-semibold text-gray-900 mb-1">Attributes</h2>
        <p className="text-sm text-gray-500 mb-4">
          Custom key-value pairs available as {'{'}user.KEY{'}'} in filter and mask expressions,
          and as <code className="text-xs bg-gray-100 px-1 rounded">ctx.session.user.attributes</code> in decision functions.
        </p>
        <UserAttributeEditor
          attributes={pendingAttributes ?? user.attributes}
          onChange={(attrs) => {
            setPendingAttributes(attrs)
            setAttrSuccess(false)
          }}
        />
        <div className="mt-4 flex items-center gap-3">
          <button
            type="button"
            onClick={handleSaveAttributes}
            disabled={attrSaving || pendingAttributes === null}
            className="bg-gray-800 hover:bg-gray-900 disabled:opacity-60 text-white text-sm font-medium rounded-lg px-4 py-2 transition-colors"
          >
            {attrSaving ? 'Saving...' : 'Save attributes'}
          </button>
          {attrSuccess && <span className="text-sm text-green-600">Attributes saved.</span>}
        </div>
        {attrError && <p className="text-sm text-red-600 mt-2">{attrError}</p>}
      </section>

      {/* Activity */}
      <section className="border-t border-gray-200 pt-8">
        <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
        <AuditTimeline resourceType="user" resourceId={userId} />
      </section>
    </div>
  )
}
