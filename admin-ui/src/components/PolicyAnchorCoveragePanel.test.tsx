import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import { PolicyAnchorCoveragePanel } from './PolicyAnchorCoveragePanel'
import { renderWithProviders } from '../test/test-utils'
import type { PolicyAnchorCoverageResponse } from '../types/policy'

describe('PolicyAnchorCoveragePanel', () => {
  it('shows a loading shim when data is undefined', () => {
    renderWithProviders(<PolicyAnchorCoveragePanel data={undefined} />)
    expect(document.body.textContent).toMatch(/checking anchor coverage/i)
  })

  it('renders nothing when there are zero assigned tables', () => {
    const data: PolicyAnchorCoverageResponse = {
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [],
    }
    const { container } = renderWithProviders(<PolicyAnchorCoveragePanel data={data} />)
    expect(container.firstChild).toBeNull()
  })

  it('shows green banner when all verdicts pass', () => {
    const data: PolicyAnchorCoverageResponse = {
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'orders',
          verdicts: [{ kind: 'on_table', column: 'tenant' }],
        },
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
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
    }

    renderWithProviders(<PolicyAnchorCoveragePanel data={data} />)

    expect(screen.getByTestId('anchor-coverage-clean')).toBeInTheDocument()
    expect(document.body.textContent).toMatch(/2 tables resolve cleanly/i)
  })

  it('shows red panel listing broken pairs with datasource link', () => {
    const data: PolicyAnchorCoverageResponse = {
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-42',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'invoices',
          verdicts: [{ kind: 'missing_anchor', column: 'tenant' }],
        },
        {
          data_source_id: 'ds-42',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'orders',
          verdicts: [{ kind: 'on_table', column: 'tenant' }],
        },
      ],
    }

    renderWithProviders(<PolicyAnchorCoveragePanel data={data} />)

    expect(screen.getByTestId('anchor-coverage-broken')).toBeInTheDocument()
    expect(document.body.textContent).toMatch(/silently deny on 1 table/i)
    expect(document.body.textContent).not.toMatch(/of 2/)
    expect(document.body.textContent).toMatch(/invoices/)
    expect(document.body.textContent).toMatch(/no anchor configured/i)

    const link = screen.getByRole('link', { name: /add anchor/i })
    expect(link).toHaveAttribute(
      'href',
      `/datasources/ds-42/edit?section=anchors&focus=${encodeURIComponent('public.invoices.tenant')}`,
    )
  })

  it('shows the upstream schema in muted parens when an alias is set', () => {
    const data: PolicyAnchorCoverageResponse = {
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-99',
          data_source_name: 'staging',
          schema: 'pg',
          schema_upstream: 'postgres',
          table: 'payments',
          verdicts: [{ kind: 'missing_anchor', column: 'tenant_id' }],
        },
      ],
    }

    renderWithProviders(<PolicyAnchorCoveragePanel data={data} />)

    expect(document.body.textContent).toMatch(/pg\.payments/)
    expect(document.body.textContent).toMatch(/\(postgres\.payments\)/)

    const link = screen.getByRole('link', { name: /add anchor/i })
    expect(link).toHaveAttribute(
      'href',
      `/datasources/ds-99/edit?section=anchors&focus=${encodeURIComponent('pg.payments.tenant_id')}`,
    )
  })

  it('reports alias-target verdicts as their dedicated message', () => {
    const data: PolicyAnchorCoverageResponse = {
      policy_id: 'p-1',
      policy_type: 'row_filter',
      coverage: [
        {
          data_source_id: 'ds-1',
          data_source_name: 'prod',
          schema: 'public',
          schema_upstream: 'public',
          table: 'orders',
          verdicts: [
            {
              kind: 'missing_column_on_alias_target',
              column: 'tenant_id',
              actual_column_name: 'org_id',
            },
          ],
        },
      ],
    }
    renderWithProviders(<PolicyAnchorCoveragePanel data={data} />)
    expect(document.body.textContent).toMatch(/alias points at missing column/i)
    expect(document.body.textContent).toMatch(/org_id/)
  })
})
