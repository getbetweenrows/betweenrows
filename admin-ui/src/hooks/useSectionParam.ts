import { useCallback } from 'react'
import { useSearchParams } from 'react-router-dom'

/**
 * URL-driven section state for T1 (sidebar) pages.
 *
 * Reads `?section=<id>`, falling back to `fallback` if the param is missing
 * or not in `validIds`. Internal clicks use `replace: true` so section
 * switching doesn't bloat browser history.
 */
export function useSectionParam<Id extends string>(
  validIds: readonly Id[],
  fallback: Id,
): [Id, (next: Id) => void] {
  const [searchParams, setSearchParams] = useSearchParams()
  const raw = searchParams.get('section')
  const active: Id = raw && (validIds as readonly string[]).includes(raw) ? (raw as Id) : fallback

  const select = useCallback(
    (next: Id) => {
      const params = new URLSearchParams(searchParams)
      params.set('section', next)
      setSearchParams(params, { replace: true })
    },
    [searchParams, setSearchParams],
  )

  return [active, select]
}
