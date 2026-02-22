import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { createUser } from '../api/users'
import { UserForm } from '../components/UserForm'
import type { CreateUserPayload } from '../types/user'

export function UserCreatePage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleSubmit(data: CreateUserPayload) {
    setError(null)
    setLoading(true)
    try {
      await createUser(data)
      await queryClient.invalidateQueries({ queryKey: ['users'] })
      navigate('/', { replace: true })
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to create user'
      setError(msg)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="p-6">
      <h1 className="text-xl font-bold text-gray-900 mb-6">New user</h1>
      <UserForm
        mode="create"
        onSubmit={(d) => handleSubmit(d as CreateUserPayload)}
        onCancel={() => navigate('/')}
        loading={loading}
        error={error}
      />
    </div>
  )
}
