import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'
import { deleteDataSource, updateDataSource } from '../api/datasources'
import type { DataSource } from '../types/datasource'
import { validateDatasourceName } from '../utils/nameValidation'
import { DangerZone, DangerRow } from './DangerZone'
import { ModalShell } from './ModalShell'
import { ConfirmDeleteModal } from './ConfirmDeleteModal'

interface Props {
  ds: DataSource
}

export function DataSourceDangerZone({ ds }: Props) {
  const [openModal, setOpenModal] = useState<'rename' | 'delete' | null>(null)

  return (
    <>
      <DangerZone>
        <DangerRow
          title="Rename data source"
          body={
            <>
              Renaming breaks external references: SQL three-part identifiers
              (<code className="font-mono">oldName.schema.table</code>), connection strings,
              and decision function scripts that match on the datasource name. Policy
              enforcement is unaffected (keyed on datasource ID).
            </>
          }
          action={
            <button
              type="button"
              onClick={() => setOpenModal('rename')}
              className="border border-red-300 text-red-700 text-sm font-medium rounded-lg px-4 py-1.5 hover:bg-red-50 transition-colors"
            >
              Rename…
            </button>
          }
        />

        <DangerRow
          title="Delete data source"
          body={
            <>
              Permanently removes the catalog (schemas, tables, columns), relationships,
              column anchors, and user / role / policy assignments for this datasource.
              Not recoverable.
            </>
          }
          action={
            <button
              type="button"
              onClick={() => setOpenModal('delete')}
              className="bg-red-600 text-white text-sm font-medium rounded-lg px-4 py-1.5 hover:bg-red-700 transition-colors"
            >
              Delete…
            </button>
          }
        />
      </DangerZone>

      {openModal === 'rename' && (
        <RenameModal ds={ds} onClose={() => setOpenModal(null)} />
      )}
      {openModal === 'delete' && (
        <DeleteModal ds={ds} onClose={() => setOpenModal(null)} />
      )}
    </>
  )
}

function RenameModal({ ds, onClose }: { ds: DataSource; onClose: () => void }) {
  const queryClient = useQueryClient()
  const [newName, setNewName] = useState(ds.name)
  const [error, setError] = useState<string | null>(null)

  const trimmed = newName.trim()
  const validationError = validateDatasourceName(trimmed)
  const changed = trimmed !== ds.name
  const canSubmit = changed && !validationError

  const mutation = useMutation({
    mutationFn: () => updateDataSource(ds.id, { name: trimmed }),
    onSuccess: () => {
      toast.success(`Renamed to ${trimmed}`)
      queryClient.invalidateQueries({ queryKey: ['datasources'] })
      queryClient.invalidateQueries({ queryKey: ['datasource', ds.id] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      onClose()
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to rename'
      setError(msg)
    },
  })

  return (
    <ModalShell title={`Rename ${ds.name}`} onClose={onClose}>
      <div className="rounded-md bg-amber-50 border border-amber-200 px-3 py-2 text-xs text-amber-800 mb-4">
        <p className="font-medium">This may break external references.</p>
        <ul className="mt-1 list-disc list-inside space-y-0.5">
          <li>SQL queries using the three-part name <code className="font-mono">{ds.name}.schema.table</code></li>
          <li>Connection strings referencing the current name</li>
          <li>Decision functions matching on <code className="font-mono">ctx.session.datasource.name</code></li>
        </ul>
        <p className="mt-1">Policies continue to enforce correctly (they key on datasource ID).</p>
      </div>

      <label className="block text-xs font-medium text-gray-600 mb-1">New name</label>
      <input
        autoFocus
        type="text"
        value={newName}
        onChange={(e) => {
          setNewName(e.target.value)
          setError(null)
        }}
        className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
      />
      {validationError && changed && (
        <p className="text-xs text-red-600 mt-1">{validationError}</p>
      )}
      {error && <p className="text-xs text-red-600 mt-2">{error}</p>}

      <div className="flex items-center justify-end gap-2 mt-5">
        <button
          type="button"
          onClick={onClose}
          className="text-sm text-gray-600 px-3 py-1.5 rounded-lg hover:bg-gray-50"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={() => mutation.mutate()}
          disabled={!canSubmit || mutation.isPending}
          className="bg-blue-600 text-white text-sm font-medium rounded-lg px-4 py-1.5 hover:bg-blue-700 disabled:opacity-50"
        >
          {mutation.isPending ? 'Renaming…' : 'Rename'}
        </button>
      </div>
    </ModalShell>
  )
}

function DeleteModal({ ds, onClose }: { ds: DataSource; onClose: () => void }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [error, setError] = useState<string | null>(null)

  const deactivateMutation = useMutation({
    mutationFn: () => updateDataSource(ds.id, { is_active: false }),
    onSuccess: () => {
      toast.success(`${ds.name} deactivated`)
      queryClient.invalidateQueries({ queryKey: ['datasources'] })
      queryClient.invalidateQueries({ queryKey: ['datasource', ds.id] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      onClose()
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to deactivate'
      setError(msg)
    },
  })

  const deleteMutation = useMutation({
    mutationFn: () => deleteDataSource(ds.id),
    onSuccess: () => {
      toast.success(`Deleted ${ds.name}`)
      queryClient.invalidateQueries({ queryKey: ['datasources'] })
      queryClient.invalidateQueries({ queryKey: ['admin-audit'] })
      navigate('/datasources')
    },
    onError: (err: unknown) => {
      const msg =
        (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
        'Failed to delete'
      setError(msg)
    },
  })

  return (
    <ConfirmDeleteModal
      resourceName={ds.name}
      consequences={
        <>
          <li>Catalog (schemas, tables, columns)</li>
          <li>Relationships and column anchors</li>
          <li>User, role, and policy assignments for this datasource</li>
        </>
      }
      softDelete={
        ds.is_active
          ? {
              label: 'Deactivate instead',
              pendingLabel: 'Deactivating…',
              explanation: (
                <>
                  <span className="font-medium">Consider deactivating instead.</span>{' '}
                  Deactivation preserves the configuration and catalog — users just can't
                  connect. You can reactivate it later.
                </>
              ),
              onConfirm: () => deactivateMutation.mutate(),
              pending: deactivateMutation.isPending,
            }
          : undefined
      }
      onDelete={() => deleteMutation.mutate()}
      deletePending={deleteMutation.isPending}
      onClose={onClose}
      error={error}
    />
  )
}
