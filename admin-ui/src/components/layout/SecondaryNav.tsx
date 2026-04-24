export interface SectionDef<Id extends string = string> {
  id: Id
  label: string
  group?: string
}

interface SecondaryNavProps<Id extends string> {
  ariaLabel: string
  sections: readonly SectionDef<Id>[]
  active: Id
  onSelect: (id: Id) => void
  testId?: string
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
                  className={`w-full text-left px-3 py-1.5 rounded-md text-sm transition-colors ${
                    isActive
                      ? 'bg-blue-50 text-blue-700 font-medium'
                      : 'text-gray-600 hover:bg-gray-100 hover:text-gray-900'
                  }`}
                >
                  {s.label}
                </button>
              )
            })}
          </div>
        </div>
      ))}
    </nav>
  )
}
