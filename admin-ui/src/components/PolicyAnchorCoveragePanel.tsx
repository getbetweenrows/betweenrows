import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import { getPolicyAnchorCoverage } from '../api/policies'
import type { AnchorCoverageVerdict } from '../types/policy'

interface Props {
  policyId: string
  policyType: string
  /// Bumped on save. Including it in the query key forces a fresh check
  /// after every policy update without coupling to PolicyForm.
  version: number
}

type BrokenVerdict = Extract<
  AnchorCoverageVerdict,
  { kind: 'missing_anchor' } | { kind: 'missing_column_on_alias_target' }
>

function isBroken(v: AnchorCoverageVerdict): v is BrokenVerdict {
  return v.kind === 'missing_anchor' || v.kind === 'missing_column_on_alias_target'
}

export function PolicyAnchorCoveragePanel({ policyId, policyType, version }: Props) {
  const enabled = policyType === 'row_filter'

  const { data, isLoading, isError } = useQuery({
    queryKey: ['policy-anchor-coverage', policyId, version],
    queryFn: () => getPolicyAnchorCoverage(policyId),
    enabled,
  })

  if (!enabled) return null

  if (isLoading) {
    return (
      <div className="bg-white rounded-xl border border-gray-200 p-6 mt-4">
        <div className="text-xs text-gray-400">Checking anchor coverage…</div>
      </div>
    )
  }

  if (isError || !data) {
    return (
      <div className="bg-white rounded-xl border border-gray-200 p-6 mt-4">
        <div className="text-xs text-red-500">Could not load anchor coverage.</div>
      </div>
    )
  }

  const totalTables = data.coverage.length
  const brokenEntries = data.coverage.filter((entry) =>
    entry.verdicts.some(isBroken),
  )

  if (totalTables === 0) {
    // No assigned tables (or no column refs in filter) — nothing to warn about.
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
      className="bg-red-50 border border-red-200 rounded-xl p-4 mt-4"
    >
      <h2 className="text-sm font-semibold text-red-800 mb-1">
        This row filter will silently deny on {brokenEntries.length} of {totalTables}{' '}
        {totalTables === 1 ? 'table' : 'tables'}
      </h2>
      <p className="text-xs text-red-700 mb-3">
        Each row below references a column that isn't on the target table and has no
        column anchor configured. Add an anchor on the data source page to fix this.
      </p>
      <ul className="space-y-2">
        {brokenEntries.map((entry) => {
          const broken = entry.verdicts.filter(isBroken)
          return (
            <li
              key={`${entry.data_source_id}|${entry.schema}|${entry.table}`}
              className="text-xs"
            >
              <div className="font-mono text-red-900">
                {entry.data_source_name} · {entry.schema}.{entry.table}
              </div>
              <ul className="ml-4 mt-1 space-y-1">
                {broken.map((v) => (
                  <li key={v.column} className="flex items-center gap-2">
                    <span className="font-mono text-red-800">{v.column}</span>
                    {v.kind === 'missing_anchor' ? (
                      <span className="text-red-600">— no anchor configured</span>
                    ) : (
                      <span className="text-red-600">
                        — alias points at missing column{' '}
                        <span className="font-mono">{v.actual_column_name}</span>
                      </span>
                    )}
                    <Link
                      to={`/datasources/${entry.data_source_id}/edit?section=anchors&focus=${encodeURIComponent(
                        `${entry.schema}.${entry.table}.${v.column}`,
                      )}`}
                      className="text-red-700 underline hover:text-red-900"
                    >
                      Add anchor
                    </Link>
                  </li>
                ))}
              </ul>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
