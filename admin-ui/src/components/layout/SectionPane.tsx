import type { ReactNode } from 'react'

export type SectionWidth = 'narrow' | 'wide' | 'full'

const WIDTH_CLASSES: Record<SectionWidth, string> = {
  narrow: 'max-w-2xl',
  wide: 'max-w-5xl',
  full: 'max-w-none',
}

interface SectionPaneProps {
  active: boolean
  width?: SectionWidth
  testId?: string
  children: ReactNode
}

export function SectionPane({
  active,
  width = 'wide',
  testId,
  children,
}: SectionPaneProps) {
  return (
    <div
      className={active ? WIDTH_CLASSES[width] : 'hidden'}
      aria-hidden={!active}
      data-testid={testId}
    >
      {children}
    </div>
  )
}
