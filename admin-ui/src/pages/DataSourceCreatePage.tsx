import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { createDataSource } from '../api/datasources'
import { DataSourceForm } from '../components/DataSourceForm'

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
      await createDataSource({
        name: values.name,
        ds_type: values.ds_type,
        config: values.config,
      })
      queryClient.invalidateQueries({ queryKey: ['datasources'] })
      navigate('/datasources')
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <div className="p-6 max-w-2xl">
      <div className="mb-6">
        <button
          onClick={() => navigate('/datasources')}
          className="text-sm text-gray-500 hover:text-gray-700 mb-2"
        >
          ← Back to Data Sources
        </button>
        <h1 className="text-xl font-bold text-gray-900">New Data Source</h1>
        <p className="text-sm text-gray-500 mt-1">
          Configure a connection to your database backend.
        </p>
      </div>

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
