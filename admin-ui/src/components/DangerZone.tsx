import type { ReactNode } from 'react'

interface DangerZoneProps {
  title?: string
  children: ReactNode
  className?: string
}

export function DangerZone({
  title = 'Danger zone',
  children,
  className,
}: DangerZoneProps) {
  return (
    <div
      className={`mt-6 rounded-xl border border-red-200 bg-white overflow-hidden ${className ?? ''}`}
    >
      <div className="border-b border-red-200 bg-red-50 px-6 py-3">
        <h2 className="text-sm font-semibold text-red-800">{title}</h2>
      </div>
      {children}
    </div>
  )
}

interface DangerRowProps {
  title: string
  body: ReactNode
  action: ReactNode
}

export function DangerRow({ title, body, action }: DangerRowProps) {
  return (
    <div className="flex items-center justify-between gap-4 px-6 py-4 border-t border-red-100 first:border-t-0">
      <div className="min-w-0">
        <h3 className="text-sm font-semibold text-gray-900">{title}</h3>
        <p className="text-xs text-gray-600 mt-1">{body}</p>
      </div>
      <div className="shrink-0">{action}</div>
    </div>
  )
}
