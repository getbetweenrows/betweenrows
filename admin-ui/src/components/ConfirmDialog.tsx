import type { ReactNode } from 'react'
import { ModalShell } from './ModalShell'

interface ConfirmDialogProps {
  title: string
  message: ReactNode
  confirmLabel?: string
  confirmPendingLabel?: string
  /** Red destructive styling when true (default); neutral blue when false. */
  destructive?: boolean
  pending?: boolean
  onConfirm: () => void
  onCancel: () => void
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel = 'Confirm',
  confirmPendingLabel,
  destructive = true,
  pending = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const buttonTone = destructive
    ? 'bg-red-600 hover:bg-red-700'
    : 'bg-blue-600 hover:bg-blue-700'

  return (
    <ModalShell title={title} onClose={onCancel} size="sm">
      <div className="text-sm text-gray-700 mb-5">{message}</div>
      <div className="flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="text-sm text-gray-600 px-3 py-1.5 rounded-lg hover:bg-gray-50"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={onConfirm}
          disabled={pending}
          className={`${buttonTone} text-white text-sm font-medium rounded-lg px-4 py-1.5 disabled:opacity-50 transition-colors`}
        >
          {pending && confirmPendingLabel ? confirmPendingLabel : confirmLabel}
        </button>
      </div>
    </ModalShell>
  )
}
