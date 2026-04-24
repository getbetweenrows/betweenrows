import { describe, it, expect, vi, afterEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { PolicyAnchorCoveragePanel } from './PolicyAnchorCoveragePanel'
import { renderWithProviders } from '../test/test-utils'
import * as policiesApi from '../api/policies'
import type { PolicyAnchorCoverageResponse } from '../types/policy'

afterEach(() => {
  vi.restoreAllMocks()
})

function mockCoverage(coverage: PolicyAnchorCoverageResponse) {
  vi.spyOn(policiesApi, 'getPolicyAnchorCoverage').mockResolvedValue(coverage)
}

describe('PolicyAnchorCoveragePanel', () => {
  it('renders nothing for non-row-filter policies', () => {
    const { container } = renderWithProviders(
      <PolicyAnchorCoveragePanel
        policyId="p-1"
        policyType="column_mask"
        version={1}
      />,
    )
    expect(container.firstChild).toBeNull()
  })

  it('shows green banner when all verdicts pass', async () => {
    mockCoverage({
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          table: 'orders',
          verdicts: [{ kind: 'on_table', column: 'tenant' }],
        },
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          table: 'payments',
          verdicts: [
            {
              kind: 'anchor_walk',
              column: 'tenant',
              via_relationship_id: 'rel-1',
              via_child_column: 'order_id',
              via_parent_column: 'id',
              parent_schema: 'public',
              parent_table: 'orders',
            },
          ],
        },
      ],
    })

    renderWithProviders(
      <PolicyAnchorCoveragePanel policyId="p-1" policyType="row_filter" version={1} />,
    )

    await waitFor(() =>
      expect(screen.getByTestId('anchor-coverage-clean')).toBeInTheDocument(),
    )
    expect(document.body.textContent).toMatch(/2 tables resolve cleanly/i)
  })

  it('shows red panel listing broken pairs with datasource link', async () => {
    mockCoverage({
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-42',
          data_source_name: 'prod',
          schema: 'public',
          table: 'invoices',
          verdicts: [{ kind: 'missing_anchor', column: 'tenant' }],
        },
        {
          data_source_id: 'ds-42',
          data_source_name: 'prod',
          schema: 'public',
          table: 'orders',
          verdicts: [{ kind: 'on_table', column: 'tenant' }],
        },
      ],
    })

    renderWithProviders(
      <PolicyAnchorCoveragePanel policyId="p-1" policyType="row_filter" version={3} />,
    )

    await waitFor(() =>
      expect(screen.getByTestId('anchor-coverage-broken')).toBeInTheDocument(),
    )
    expect(document.body.textContent).toMatch(/silently deny on 1 of 2 tables/i)
    expect(document.body.textContent).toMatch(/invoices/)
    expect(document.body.textContent).toMatch(/no anchor configured/i)

    const link = screen.getByRole('link', { name: /add anchor/i })
    expect(link).toHaveAttribute(
      'href',
      `/datasources/ds-42/edit?section=anchors&focus=${encodeURIComponent('public.invoices.tenant')}`,
    )
  })
})
