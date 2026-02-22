import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { getDataSource, updateDataSource } from '../api/datasources'
import { DataSourceForm } from '../components/DataSourceForm'
import { UserAssignmentPanel } from '../components/UserAssignmentPanel'

export function DataSourceEditPage() {
  const { id } = useParams<{ id: string }>()
  const dsId = id ?? ''
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)

  const { data: ds, isLoading, isError } = useQuery({
    queryKey: ['datasource', dsId],
    queryFn: () => getDataSource(dsId),
    enabled: !!dsId,
  })

  async function handleSubmit(values: {
    name: string
    ds_type: string
    config: Record<string, unknown>
    is_active: boolean
  }) {
    setIsSubmitting(true)
    try {
      await updateDataSource(dsId, {
        name: values.name,
        is_active: values.is_active,
        config: values.config,
      })
      queryClient.invalidateQueries({ queryKey: ['datasources'] })
      queryClient.invalidateQueries({ queryKey: ['datasource', dsId] })
      navigate('/datasources')
    } finally {
      setIsSubmitting(false)
    }
  }

  if (isLoading) {
    return <div className="p-6 text-sm text-gray-400">Loading…</div>
  }

  if (isError || !ds) {
    return (
      <div className="p-6 text-sm text-red-500">
        Data source not found.{' '}
        <button onClick={() => navigate('/datasources')} className="underline">
          Go back
        </button>
      </div>
    )
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
        <h1 className="text-xl font-bold text-gray-900">
          Edit: <span className="font-mono text-lg">{ds.name}</span>
        </h1>
        <p className="text-sm text-gray-500 mt-1">Type: {ds.ds_type}</p>
      </div>

      <div className="bg-white rounded-xl border border-gray-200 p-6 mb-6">
        <DataSourceForm
          datasourceId={dsId}
          initialValues={{
            name: ds.name,
            ds_type: ds.ds_type,
            config: ds.config as Record<string, unknown>,
            is_active: ds.is_active,
          }}
          onSubmit={handleSubmit}
          submitLabel="Save changes"
          isSubmitting={isSubmitting}
        />
      </div>

      <div className="bg-white rounded-xl border border-gray-200 p-6 mb-6">
        <UserAssignmentPanel datasourceId={dsId} />
      </div>

      <div className="bg-white rounded-xl border border-gray-200 p-6">
        <h2 className="text-base font-semibold text-gray-900 mb-2">Schema Catalog</h2>
        <p className="text-sm text-gray-500 mb-3">
          Discover and select which schemas and tables are accessible via the proxy.
        </p>
        <button
          onClick={() => navigate(`/datasources/${dsId}/catalog`)}
          className="px-3 py-1.5 text-sm bg-indigo-600 text-white rounded-lg hover:bg-indigo-700"
        >
          Manage Catalog
        </button>
      </div>
    </div>
  )
}
