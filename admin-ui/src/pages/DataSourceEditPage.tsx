import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'
import { getDataSource, updateDataSource } from '../api/datasources'
import { DataSourceForm } from '../components/DataSourceForm'
import { UserAssignmentPanel } from '../components/UserAssignmentPanel'
import { RoleAccessPanel } from '../components/RoleAccessPanel'
import {
  ColumnAnchorsSection,
  RelationshipsSection,
} from '../components/RelationshipsPanel'
import { DatasourceAssignmentsReadonly } from '../components/PolicyAssignmentPanel'
import { AuditTimeline } from '../components/AuditTimeline'
import { CopyableId } from '../components/CopyableId'
import { CatalogSection } from './DataSourceCatalogPage'
import { DataSourceDangerZone } from '../components/DataSourceDangerZone'
import { PageHeader } from '../components/layout/PageHeader'
import { SecondaryNav, type SectionDef } from '../components/layout/SecondaryNav'
import { SectionPane, type SectionWidth } from '../components/layout/SectionPane'
import { StatusDot } from '../components/Status'
import { useSectionParam } from '../hooks/useSectionParam'

type SectionId =
  | 'details'
  | 'users'
  | 'roles'
  | 'policies'
  | 'catalog'
  | 'relationships'
  | 'anchors'
  | 'activity'

interface DsSection extends SectionDef<SectionId> {
  width: SectionWidth
}

const SECTIONS: readonly DsSection[] = [
  { id: 'details', label: 'Details', group: 'Configuration', width: 'narrow' },
  { id: 'users', label: 'Users', group: 'Access', width: 'wide' },
  { id: 'roles', label: 'Roles', group: 'Access', width: 'wide' },
  { id: 'policies', label: 'Policies', group: 'Access', width: 'wide' },
  { id: 'catalog', label: 'Catalog', group: 'Schema', width: 'wide' },
  { id: 'relationships', label: 'Relationships', group: 'Schema', width: 'wide' },
  { id: 'anchors', label: 'Column anchors', group: 'Schema', width: 'wide' },
  { id: 'activity', label: 'Activity', group: 'History', width: 'wide' },
]

const VALID_IDS: readonly SectionId[] = SECTIONS.map((s) => s.id)

export function DataSourceEditPage() {
  const { id } = useParams<{ id: string }>()
  const dsId = id ?? ''
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [activeSection, selectSection] = useSectionParam<SectionId>(VALID_IDS, 'details')

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
    access_mode?: string
  }) {
    // Rename is a separate flow in the Danger Zone (see
    // `docs/permission-system.md` → "Rename fragility and label-based
    // identifiers"). Don't touch `name` in the default save path.
    setIsSubmitting(true)
    try {
      await updateDataSource(dsId, {
        is_active: values.is_active,
        config: values.config,
        access_mode: values.access_mode,
      })
      queryClient.invalidateQueries({ queryKey: ['datasources'] })
      queryClient.invalidateQueries({ queryKey: ['datasource', dsId] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      toast.success('Saved')
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
    <div className="p-6">
      <PageHeader
        breadcrumb={[
          { label: 'Data Sources', href: '/datasources' },
          { label: ds.name },
        ]}
        title={ds.name}
        status={<StatusDot active={ds.is_active} />}
        metadata={[<span key="type">{ds.ds_type}</span>, <CopyableId key="id" id={dsId} short />]}
      />

      <div className="flex items-start gap-6">
        <SecondaryNav
          ariaLabel="DataSource sections"
          sections={SECTIONS}
          active={activeSection}
          onSelect={selectSection}
        />

        <div className="flex-1 min-w-0">
          {SECTIONS.map((s) => (
            <SectionPane key={s.id} active={activeSection === s.id} width={s.width}>
              {s.id === 'details' && (
                <>
                  <div className="bg-white rounded-xl border border-gray-200 p-6">
                    <DataSourceForm
                      datasourceId={dsId}
                      initialValues={{
                        name: ds.name,
                        ds_type: ds.ds_type,
                        config: ds.config as Record<string, unknown>,
                        is_active: ds.is_active,
                        access_mode: ds.access_mode,
                      }}
                      onSubmit={handleSubmit}
                      submitLabel="Save changes"
                      isSubmitting={isSubmitting}
                    />
                  </div>
                  <DataSourceDangerZone ds={ds} />
                </>
              )}

              {s.id === 'users' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <UserAssignmentPanel datasourceId={dsId} />
                </div>
              )}

              {s.id === 'roles' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <RoleAccessPanel datasourceId={dsId} />
                </div>
              )}

              {s.id === 'policies' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <DatasourceAssignmentsReadonly datasourceId={dsId} />
                </div>
              )}

              {s.id === 'catalog' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <CatalogSection ds={ds} />
                </div>
              )}

              {s.id === 'relationships' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <RelationshipsSection datasourceId={dsId} />
                </div>
              )}

              {s.id === 'anchors' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <ColumnAnchorsSection datasourceId={dsId} />
                </div>
              )}

              {s.id === 'activity' && (
                <div className="bg-white rounded-xl border border-gray-200 p-6">
                  <h2 className="text-base font-semibold text-gray-900 mb-3">Activity</h2>
                  <AuditTimeline resourceType="datasource" resourceId={dsId} />
                </div>
              )}
            </SectionPane>
          ))}
        </div>
      </div>
    </div>
  )
}
