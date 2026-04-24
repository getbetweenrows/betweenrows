import { Link } from 'react-router-dom'
import type {
  AnchorCoverageTableEntry,
  AnchorCoverageVerdict,
  PolicyAnchorCoverageResponse,
} from '../types/policy'

interface Props {
  /// Coverage data fetched at the page level. `undefined` while loading or
  /// for non-row-filter policies (the page hides the section nav for those).
  data: PolicyAnchorCoverageResponse | undefined
}

type BrokenVerdict = Extract<
  AnchorCoverageVerdict,
  { kind: 'missing_anchor' } | { kind: 'missing_column_on_alias_target' }
>

function isBroken(v: AnchorCoverageVerdict): v is BrokenVerdict {
  return v.kind === 'missing_anchor' || v.kind === 'missing_column_on_alias_target'
}

export function PolicyAnchorCoveragePanel({ data }: Props) {
  if (!data) {
    return (
      <div className="bg-white rounded-xl border border-gray-200 p-6 mt-4">
        <div className="text-xs text-gray-400">Checking anchor coverage…</div>
      </div>
    )
  }

  const totalTables = data.coverage.length
  const brokenEntries = data.coverage.filter((entry) => entry.verdicts.some(isBroken))

  if (totalTables === 0) {
    return null
  }

  if (brokenEntries.length === 0) {
    return (
      <div
        data-testid="anchor-coverage-clean"
        className="bg-green-50 border border-green-200 rounded-xl p-4 mt-4"
      >
        <div className="text-sm text-green-700">
          All {totalTables} {totalTables === 1 ? 'table' : 'tables'} resolve cleanly.
        </div>
      </div>
    )
  }

  return (
    <div
      data-testid="anchor-coverage-broken"
      className="bg-white border border-gray-200 rounded-xl p-4 mt-4"
    >
      <h2 className="text-sm font-semibold text-red-800 mb-1">
        This row filter will silently deny on {brokenEntries.length}{' '}
        {brokenEntries.length === 1 ? 'table' : 'tables'}
      </h2>
      <p className="text-xs text-gray-600 mb-3">
        Each row below references a column that isn't on the target table and has no column anchor
        configured. Add an anchor on the data source page to fix this.
      </p>
      <div className="space-y-3">
        {brokenEntries.map((entry) => (
          <BrokenTableGroup
            key={`${entry.data_source_id}|${entry.schema}|${entry.table}`}
            entry={entry}
          />
        ))}
      </div>
    </div>
  )
}

function BrokenTableGroup({ entry }: { entry: AnchorCoverageTableEntry }) {
  const broken = entry.verdicts.filter(isBroken)
  const showUpstream = entry.schema_upstream && entry.schema_upstream !== entry.schema
  return (
    <div className="bg-white rounded-lg border border-red-200 overflow-hidden">
      <div className="px-3 py-2 bg-red-100/50 border-b border-red-200">
        <div className="text-[10px] uppercase tracking-wide text-red-700/80">
          {entry.data_source_name}
        </div>
        <div className="font-mono text-sm text-red-900">
          {entry.schema}.{entry.table}
          {showUpstream && (
            <span className="text-xs text-red-700/60">
              {' '}
              ({entry.schema_upstream}.{entry.table})
            </span>
          )}
        </div>
      </div>
      <ul className="divide-y divide-red-100">
        {broken.map((v) => (
          <li
            key={v.column}
            className="px-3 py-2 flex items-center justify-between gap-3"
          >
            <div className="min-w-0 text-xs">
              <div className="font-mono text-red-900">{v.column}</div>
              <div className="text-red-600">
                {v.kind === 'missing_anchor' ? (
                  'No anchor configured — runtime substitutes Filter(false).'
                ) : (
                  <>
                    Alias points at missing column{' '}
                    <span className="font-mono">{v.actual_column_name}</span>.
                  </>
                )}
              </div>
            </div>
            <Link
              to={`/datasources/${entry.data_source_id}/edit?section=anchors&focus=${encodeURIComponent(
                `${entry.schema}.${entry.table}.${v.column}`,
              )}`}
              className="shrink-0 text-xs font-medium px-2 py-1 rounded border border-red-300 text-red-700 bg-white hover:bg-red-100"
            >
              Add anchor →
            </Link>
          </li>
        ))}
      </ul>
    </div>
  )
}
