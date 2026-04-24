import { useState, type ReactNode } from 'react'
import { ModalShell } from './ModalShell'

interface SoftDeleteAction {
  /** Button label, e.g. "Deactivate instead" / "Disable instead". */
  label: string
  /** Label while the mutation is in flight. */
  pendingLabel: string
  /** One-paragraph explanation shown above the button. */
  explanation: ReactNode
  onConfirm: () => void
  pending: boolean
}

interface ConfirmDeleteModalProps {
  /** The resource's human-readable name — users type it to confirm. */
  resourceName: string
  /** What will be deleted, rendered as bullet points. */
  consequences: ReactNode
  /** Optional soft-delete escape hatch (deactivate / disable). */
  softDelete?: SoftDeleteAction
  /** Delete handler. */
  onDelete: () => void
  deletePending: boolean
  onClose: () => void
  error?: string | null
}

export function ConfirmDeleteModal({
  resourceName,
  consequences,
  softDelete,
  onDelete,
  deletePending,
  onClose,
  error,
}: ConfirmDeleteModalProps) {
  const [confirmName, setConfirmName] = useState('')
  const canDelete = confirmName === resourceName

  return (
    <ModalShell title={`Delete ${resourceName}?`} onClose={onClose}>
      <p className="text-sm text-gray-700 mb-2">This will permanently remove:</p>
      <ul className="text-xs text-gray-600 list-disc list-inside space-y-0.5 mb-4">
        {consequences}
      </ul>

      {softDelete && (
        <div className="rounded-md bg-blue-50 border border-blue-200 px-3 py-3 mb-4">
          <p className="text-xs text-blue-900 mb-2">{softDelete.explanation}</p>
          <button
            type="button"
            onClick={softDelete.onConfirm}
            disabled={softDelete.pending}
            className="bg-blue-600 text-white text-xs font-medium rounded-lg px-3 py-1.5 hover:bg-blue-700 disabled:opacity-50"
          >
            {softDelete.pending ? softDelete.pendingLabel : softDelete.label}
          </button>
        </div>
      )}

      <div className="relative text-center my-4">
        <div className="absolute inset-0 flex items-center" aria-hidden>
          <div className="w-full border-t border-gray-200" />
        </div>
        <span className="relative bg-white px-2 text-[10px] uppercase tracking-wider text-gray-500">
          Or permanently delete
        </span>
      </div>

      <label className="block text-xs text-gray-600 mb-1">
        Type <code className="font-mono font-semibold text-gray-900">{resourceName}</code> to confirm:
      </label>
      <input
        autoFocus
        type="text"
        value={confirmName}
        onChange={(e) => setConfirmName(e.target.value)}
        className="w-full border border-gray-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-red-500 font-mono"
      />
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
          onClick={onDelete}
          disabled={!canDelete || deletePending}
          className="bg-red-600 text-white text-sm font-medium rounded-lg px-4 py-1.5 hover:bg-red-700 disabled:opacity-50"
        >
          {deletePending ? 'Deleting…' : 'Delete'}
        </button>
      </div>
    </ModalShell>
  )
}
