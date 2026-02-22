import { type FormEvent, useState } from 'react'
import type { CreateUserPayload, UpdateUserPayload } from '../types/user'

type Mode = 'create' | 'edit'

interface Props {
  mode: Mode
  initialValues?: {
    username?: string
    tenant?: string
    is_admin?: boolean
    is_active?: boolean
    email?: string
    display_name?: string
  }
  onSubmit: (data: CreateUserPayload | UpdateUserPayload) => Promise<void>
  onCancel: () => void
  loading?: boolean
  error?: string | null
}

export function UserForm({ mode, initialValues = {}, onSubmit, onCancel, loading, error }: Props) {
  const [username, setUsername] = useState(initialValues.username ?? '')
  const [password, setPassword] = useState('')
  const [tenant, setTenant] = useState(initialValues.tenant ?? '')
  const [isAdmin, setIsAdmin] = useState(initialValues.is_admin ?? false)
  const [isActive, setIsActive] = useState(initialValues.is_active ?? true)
  const [email, setEmail] = useState(initialValues.email ?? '')
  const [displayName, setDisplayName] = useState(initialValues.display_name ?? '')

  async function handleSubmit(e: FormEvent) {
    e.preventDefault()
    if (mode === 'create') {
      await onSubmit({
        username,
        password,
        tenant,
        is_admin: isAdmin,
        email: email || undefined,
        display_name: displayName || undefined,
      } satisfies CreateUserPayload)
    } else {
      await onSubmit({
        tenant: tenant || undefined,
        is_admin: isAdmin,
        is_active: isActive,
        email: email || undefined,
        display_name: displayName || undefined,
      } satisfies UpdateUserPayload)
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-5 max-w-lg">
      {/* Username (create only) */}
      {mode === 'create' && (
        <Field label="Username" required>
          <input
            type="text"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
            className={inputCls}
          />
        </Field>
      )}

      {/* Password (create only) */}
      {mode === 'create' && (
        <Field label="Password" required>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            className={inputCls}
          />
        </Field>
      )}

      <Field label="Tenant" required>
        <input
          type="text"
          value={tenant}
          onChange={(e) => setTenant(e.target.value)}
          required={mode === 'create'}
          className={inputCls}
        />
      </Field>

      <Field label="Email">
        <input
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className={inputCls}
        />
      </Field>

      <Field label="Display name">
        <input
          type="text"
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          className={inputCls}
        />
      </Field>

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

        {mode === 'edit' && (
          <label className="flex items-center gap-2 text-sm cursor-pointer">
            <input
              type="checkbox"
              checked={isActive}
              onChange={(e) => setIsActive(e.target.checked)}
              className="rounded"
            />
            Active
          </label>
        )}
      </div>

      {error && <p className="text-sm text-red-600">{error}</p>}

      <div className="flex gap-3 pt-2">
        <button
          type="submit"
          disabled={loading}
          className="bg-blue-600 hover:bg-blue-700 disabled:opacity-60 text-white font-medium rounded-lg px-5 py-2 text-sm transition-colors"
        >
          {loading ? 'Saving…' : mode === 'create' ? 'Create user' : 'Save changes'}
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="text-gray-600 hover:text-gray-900 font-medium text-sm px-3 py-2 transition-colors"
        >
          Cancel
        </button>
      </div>
    </form>
  )
}

const inputCls =
  'w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500'

function Field({ label, required, children }: { label: string; required?: boolean; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-sm font-medium text-gray-700 mb-1">
        {label}
        {required && <span className="text-red-500 ml-1">*</span>}
      </label>
      {children}
    </div>
  )
}
