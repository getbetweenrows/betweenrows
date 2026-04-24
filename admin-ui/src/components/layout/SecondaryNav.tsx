export type SectionIndicatorTone = 'red' | 'yellow'

export interface SectionIndicator {
  tone: SectionIndicatorTone
  /// Pill text. Use a small string (typically a number) — long labels will
  /// crowd the nav row.
  label: string
  /// Optional accessible description (e.g. "3 broken anchor coverage entries").
  /// Falls back to the visual label for screen readers.
  ariaLabel?: string
}

export interface SectionDef<Id extends string = string> {
  id: Id
  label: string
  group?: string
  indicator?: SectionIndicator
}

interface SecondaryNavProps<Id extends string> {
  ariaLabel: string
  sections: readonly SectionDef<Id>[]
  active: Id
  onSelect: (id: Id) => void
  testId?: string
}

const INDICATOR_TONE_CLASSES: Record<SectionIndicatorTone, string> = {
  red: 'bg-red-100 text-red-700',
  yellow: 'bg-yellow-100 text-yellow-800',
}

function groupSections<Id extends string>(
  sections: readonly SectionDef<Id>[],
): Array<{ group: string | undefined; items: SectionDef<Id>[] }> {
  const groups: Array<{ group: string | undefined; items: SectionDef<Id>[] }> = []
  for (const s of sections) {
    const last = groups[groups.length - 1]
    if (last && last.group === s.group) {
      last.items.push(s)
    } else {
      groups.push({ group: s.group, items: [s] })
    }
  }
  return groups
}

export function SecondaryNav<Id extends string>({
  ariaLabel,
  sections,
  active,
  onSelect,
  testId,
}: SecondaryNavProps<Id>) {
  const grouped = groupSections(sections)
  const hasAnyGroup = grouped.some((g) => g.group !== undefined)

  return (
    <nav
      aria-label={ariaLabel}
      className="w-56 shrink-0 sticky top-6 self-start"
      data-testid={testId}
    >
      {grouped.map(({ group, items }, i) => (
        <div key={group ?? `__ungrouped_${i}`} className="mb-5 last:mb-0">
          {hasAnyGroup && group && (
            <p className="px-3 mb-1 text-[10px] font-semibold text-gray-400 uppercase tracking-wider">
              {group}
            </p>
          )}
          <div className="space-y-0.5">
            {items.map((s) => {
              const isActive = active === s.id
              return (
                <button
                  key={s.id}
                  type="button"
                  onClick={() => onSelect(s.id)}
                  aria-current={isActive ? 'page' : undefined}
                  className={`w-full text-left px-3 py-1.5 rounded-md text-sm transition-colors flex items-center justify-between gap-2 ${
                    isActive
                      ? 'bg-blue-50 text-blue-700 font-medium'
                      : 'text-gray-600 hover:bg-gray-100 hover:text-gray-900'
                  }`}
                >
                  <span>{s.label}</span>
                  {s.indicator && (
                    <span
                      data-testid={`section-indicator-${s.id}`}
                      aria-label={s.indicator.ariaLabel ?? s.indicator.label}
                      className={`inline-flex items-center rounded-full px-1.5 text-[10px] font-semibold leading-4 ${INDICATOR_TONE_CLASSES[s.indicator.tone]}`}
                    >
                      {s.indicator.label}
                    </span>
                  )}
                </button>
              )
            })}
          </div>
        </div>
      ))}
    </nav>
  )
}
