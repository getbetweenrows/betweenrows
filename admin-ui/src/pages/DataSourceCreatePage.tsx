import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { createDataSource } from '../api/datasources'
import { DataSourceForm } from '../components/DataSourceForm'
import { PageHeader } from '../components/layout/PageHeader'

export function DataSourceCreatePage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)

  async function handleSubmit(values: {
    name: string
    ds_type: string
    config: Record<string, unknown>
    is_active: boolean
  }) {
    setIsSubmitting(true)
    try {
      const ds = await createDataSource({
        name: values.name,
        ds_type: values.ds_type,
        config: values.config,
      })
      queryClient.invalidateQueries({ queryKey: ['datasources'] })
      navigate(`/datasources/${ds.id}/edit`)
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <div className="p-6 max-w-2xl">
      <PageHeader
        breadcrumb={[
          { label: 'Data Sources', href: '/datasources' },
          { label: 'New data source' },
        ]}
        title="New data source"
        metadata={[
          <span key="desc">Configure a connection to your database backend.</span>,
        ]}
      />

      <div className="bg-white rounded-xl border border-gray-200 p-6">
        <DataSourceForm
          onSubmit={handleSubmit}
          submitLabel="Create"
          isSubmitting={isSubmitting}
        />
      </div>
    </div>
  )
}
