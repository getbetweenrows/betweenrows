import type { ReactNode } from 'react'

interface StatusDotProps {
  active: boolean
  activeLabel?: string
  inactiveLabel?: string
}

export function StatusDot({
  active,
  activeLabel = 'Active',
  inactiveLabel = 'Inactive',
}: StatusDotProps) {
  return (
    <span className="inline-flex items-center gap-1.5 text-sm">
      <span
        aria-hidden
        className={`inline-block h-2 w-2 rounded-full ${
          active ? 'bg-green-500' : 'bg-gray-400'
        }`}
      />
      <span className={active ? 'text-green-700' : 'text-gray-500'}>
        {active ? activeLabel : inactiveLabel}
      </span>
    </span>
  )
}

export type ChipTone = 'gray' | 'blue' | 'green' | 'yellow' | 'red' | 'purple'

const TONE_CLASSES: Record<ChipTone, string> = {
  gray: 'bg-gray-100 text-gray-700 border-gray-200',
  blue: 'bg-blue-50 text-blue-700 border-blue-200',
  green: 'bg-green-50 text-green-700 border-green-200',
  yellow: 'bg-yellow-50 text-yellow-800 border-yellow-200',
  red: 'bg-red-50 text-red-700 border-red-200',
  purple: 'bg-purple-50 text-purple-700 border-purple-200',
}

interface StatusChipProps {
  label: ReactNode
  tone?: ChipTone
}

export function StatusChip({ label, tone = 'gray' }: StatusChipProps) {
  return (
    <span
      className={`inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium ${TONE_CLASSES[tone]}`}
    >
      {label}
    </span>
  )
}
