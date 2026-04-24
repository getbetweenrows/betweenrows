import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { createRole } from '../api/roles'
import { RoleForm } from '../components/RoleForm'
import type { RoleFormValues } from '../components/RoleForm'
import { PageHeader } from '../components/layout/PageHeader'

export function RoleCreatePage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleSubmit(values: RoleFormValues) {
    setError(null)
    setIsSubmitting(true)
    try {
      const role = await createRole({
        name: values.name,
        description: values.description || undefined,
      })
      await queryClient.invalidateQueries({ queryKey: ['roles'] })
      navigate(`/roles/${role.id}`, { replace: true })
    } catch (err: unknown) {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to create role'
      setError(msg)
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <div className="p-6 max-w-2xl">
      <PageHeader
        breadcrumb={[
          { label: 'Roles', href: '/roles' },
          { label: 'New role' },
        ]}
        title="New role"
      />

      <div className="bg-white rounded-xl border border-gray-200 p-6">
        <RoleForm
          onSubmit={handleSubmit}
          onCancel={() => navigate('/roles')}
          submitLabel="Create role"
          isSubmitting={isSubmitting}
          error={error}
        />
      </div>
    </div>
  )
}
