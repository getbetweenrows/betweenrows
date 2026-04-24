import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Route, Routes } from 'react-router-dom'
import { renderWithProviders } from '../test/test-utils'
import { ColumnAnchorsSection } from './RelationshipsPanel'
import type {
  CatalogResponse,
  ColumnAnchor,
  FkSuggestion,
  TableRelationship,
} from '../types/catalog'

vi.mock('../api/catalog', () => ({
  getCatalog: vi.fn(),
  listRelationships: vi.fn(),
  listColumnAnchors: vi.fn(),
  listFkSuggestions: vi.fn(),
  createColumnAnchor: vi.fn(),
  createRelationship: vi.fn(),
  deleteColumnAnchor: vi.fn(),
  deleteRelationship: vi.fn(),
}))

vi.mock('react-hot-toast', () => ({
  default: { success: vi.fn(), error: vi.fn() },
}))

import {
  getCatalog,
  listRelationships,
  listColumnAnchors,
  listFkSuggestions,
  createColumnAnchor,
  createRelationship,
} from '../api/catalog'

const mockGetCatalog = getCatalog as ReturnType<typeof vi.fn>
const mockListRelationships = listRelationships as ReturnType<typeof vi.fn>
const mockListColumnAnchors = listColumnAnchors as ReturnType<typeof vi.fn>
const mockListFkSuggestions = listFkSuggestions as ReturnType<typeof vi.fn>
const mockCreateColumnAnchor = createColumnAnchor as ReturnType<typeof vi.fn>
const mockCreateRelationship = createRelationship as ReturnType<typeof vi.fn>

function makeCatalog(): CatalogResponse {
  return {
    schemas: [
      {
        id: 'schema-pg',
        schema_name: 'postgres',
        schema_alias: 'pg',
        is_selected: true,
        tables: [
          {
            id: 't-payments',
            table_name: 'payments',
            table_type: 'table',
            is_selected: true,
            columns: [
              {
                id: 'c-payments-id',
                column_name: 'id',
                ordinal_position: 1,
                data_type: 'int',
                is_nullable: false,
                column_default: null,
                arrow_type: 'Int64',
                is_selected: true,
              },
              {
                id: 'c-payments-order',
                column_name: 'order_id',
                ordinal_position: 2,
                data_type: 'int',
                is_nullable: false,
                column_default: null,
                arrow_type: 'Int64',
                is_selected: true,
              },
            ],
          },
          {
            id: 't-orders',
            table_name: 'orders',
            table_type: 'table',
            is_selected: true,
            columns: [
              {
                id: 'c-orders-id',
                column_name: 'id',
                ordinal_position: 1,
                data_type: 'int',
                is_nullable: false,
                column_default: null,
                arrow_type: 'Int64',
                is_selected: true,
              },
              {
                id: 'c-orders-tenant',
                column_name: 'tenant_id',
                ordinal_position: 2,
                data_type: 'int',
                is_nullable: false,
                column_default: null,
                arrow_type: 'Int64',
                is_selected: true,
              },
            ],
          },
          {
            id: 't-customers',
            table_name: 'customers',
            table_type: 'table',
            is_selected: true,
            columns: [
              {
                id: 'c-customers-id',
                column_name: 'id',
                ordinal_position: 1,
                data_type: 'int',
                is_nullable: false,
                column_default: null,
                arrow_type: 'Int64',
                is_selected: true,
              },
              // Note: no tenant_id here — used for "non-viable candidate" test.
              {
                id: 'c-customers-region',
                column_name: 'region',
                ordinal_position: 2,
                data_type: 'text',
                is_nullable: false,
                column_default: null,
                arrow_type: 'Utf8',
                is_selected: true,
              },
            ],
          },
        ],
      },
    ],
  }
}

const REL_PAYMENTS_TO_ORDERS: TableRelationship = {
  id: 'rel-payments-orders',
  data_source_id: 'ds-1',
  child_table_id: 't-payments',
  child_table_name: 'payments',
  child_schema_name: 'postgres',
  child_column_name: 'order_id',
  parent_table_id: 't-orders',
  parent_table_name: 'orders',
  parent_schema_name: 'postgres',
  parent_column_name: 'id',
  created_at: '2026-01-01T00:00:00Z',
  created_by: null,
}

const REL_PAYMENTS_TO_CUSTOMERS: TableRelationship = {
  id: 'rel-payments-customers',
  data_source_id: 'ds-1',
  child_table_id: 't-payments',
  child_table_name: 'payments',
  child_schema_name: 'postgres',
  child_column_name: 'order_id',
  parent_table_id: 't-customers',
  parent_table_name: 'customers',
  parent_schema_name: 'postgres',
  parent_column_name: 'id',
  created_at: '2026-01-01T00:00:00Z',
  created_by: null,
}

beforeEach(() => {
  vi.clearAllMocks()
  mockGetCatalog.mockResolvedValue(makeCatalog())
  mockListRelationships.mockResolvedValue([])
  mockListColumnAnchors.mockResolvedValue([])
  mockListFkSuggestions.mockResolvedValue([])
})

function renderSection(initialEntries: string[] = ['/']) {
  return renderWithProviders(
    <Routes>
      <Route
        path="/"
        element={<ColumnAnchorsSection datasourceId="ds-1" />}
      />
    </Routes>,
    { routerEntries: initialEntries },
  )
}

describe('ColumnAnchorsSection — anchor row rendering', () => {
  it('renders the relationship cell with full child→parent path using effective schema name', async () => {
    mockListRelationships.mockResolvedValue([REL_PAYMENTS_TO_ORDERS])
    const anchor: ColumnAnchor = {
      id: 'anchor-1',
      data_source_id: 'ds-1',
      child_table_id: 't-payments',
      child_table_name: 'payments',
      resolved_column_name: 'tenant_id',
      relationship_id: 'rel-payments-orders',
      actual_column_name: null,
      designated_at: '2026-01-01T00:00:00Z',
      designated_by: 'admin',
    }
    mockListColumnAnchors.mockResolvedValue([anchor])

    renderSection()
    const row = await screen.findByTestId('anchor-row-anchor-1')
    // Anchor cell: schema is the alias (pg), table.column shown.
    expect(row.textContent).toMatch(/pg\.payments/)
    expect(row.textContent).toMatch(/tenant_id/)
    // Relationship cell: full child path → full parent path.
    expect(row.textContent).toMatch(/pg\.payments\.order_id/)
    expect(row.textContent).toMatch(/pg\.orders\.id/)
  })

  it('uses the renamed "Resolves via" header instead of "Via relationship"', async () => {
    renderSection()
    // Force the table to render by giving it an anchor.
    mockListColumnAnchors.mockResolvedValue([
      {
        id: 'anchor-x',
        data_source_id: 'ds-1',
        child_table_id: 't-payments',
        child_table_name: 'payments',
        resolved_column_name: 'tenant_id',
        relationship_id: null,
        actual_column_name: 'order_id',
        designated_at: '2026-01-01T00:00:00Z',
        designated_by: 'admin',
      },
    ])
    renderSection()
    await waitFor(() => expect(screen.getByText('Resolves via')).toBeInTheDocument())
    expect(screen.queryByText('Via relationship')).toBeNull()
  })
})

describe('ColumnAnchorsSection — child table dropdown alias parens', () => {
  it('shows muted upstream parens when a schema has an alias', async () => {
    const user = userEvent.setup()
    renderSection()
    await user.click(await screen.findByRole('button', { name: /\+ add column anchor/i }))
    // Child-table dropdown is a <select>; check option text.
    const childSelect = screen.getAllByRole('combobox')[0]
    expect(childSelect.textContent).toMatch(/pg\.payments/)
    expect(childSelect.textContent).toMatch(/\(postgres\.payments\)/)
  })
})

describe('ColumnAnchorsSection — Topic 4a: viable auto-selection', () => {
  it('auto-selects the relationship when there is exactly one viable candidate', async () => {
    const user = userEvent.setup()
    mockListRelationships.mockResolvedValue([
      REL_PAYMENTS_TO_ORDERS, // parent orders has tenant_id  → viable
      REL_PAYMENTS_TO_CUSTOMERS, // parent customers has no tenant_id → non-viable
    ])
    renderSection([
      '/?focus=' + encodeURIComponent('pg.payments.tenant_id'),
    ])

    // The deep link opens the form pre-filled. Wait for the relationship select to be populated.
    const select = await waitFor(() => {
      const combos = screen.getAllByRole('combobox')
      // Three combos: child table, mode dropdown? actually no — only child table + relationship.
      // Find the one whose options contain a viable match.
      const rel = combos.find((c) => c.textContent?.includes('orders.id'))
      expect(rel).toBeDefined()
      return rel as HTMLSelectElement
    })

    // The viable option (orders) should be auto-selected.
    await waitFor(() => expect(select.value).toBe('rel-payments-orders'))

    // Visually it's marked with the leading ✓.
    expect(select.textContent).toMatch(/✓ pg\.payments\.order_id → pg\.orders\.id/)

    // Fire the create — verify it sends the auto-selected relationship.
    mockCreateColumnAnchor.mockResolvedValue({})
    await user.click(screen.getByRole('button', { name: /save anchor/i }))
    await waitFor(() => expect(mockCreateColumnAnchor).toHaveBeenCalled())
    const [, payload] = mockCreateColumnAnchor.mock.calls[0]
    expect(payload.relationship_id).toBe('rel-payments-orders')
  })

  it('honors a deliberate clear after auto-selection', async () => {
    const user = userEvent.setup()
    mockListRelationships.mockResolvedValue([
      REL_PAYMENTS_TO_ORDERS,
      REL_PAYMENTS_TO_CUSTOMERS,
    ])
    renderSection([
      '/?focus=' + encodeURIComponent('pg.payments.tenant_id'),
    ])

    const select = await waitFor(() => {
      const combos = screen.getAllByRole('combobox')
      const rel = combos.find((c) => c.textContent?.includes('orders.id'))
      expect(rel).toBeDefined()
      return rel as HTMLSelectElement
    })

    await waitFor(() => expect(select.value).toBe('rel-payments-orders'))

    // User clears the selection — it must stay cleared, not silently re-populate.
    await user.selectOptions(select, '')
    expect(select.value).toBe('')
    // Give the effect a tick to run.
    await new Promise((r) => setTimeout(r, 0))
    expect(select.value).toBe('')
  })

  it('does not auto-select when no candidate is viable', async () => {
    mockListRelationships.mockResolvedValue([REL_PAYMENTS_TO_CUSTOMERS])
    renderSection([
      '/?focus=' + encodeURIComponent('pg.payments.tenant_id'),
    ])

    await waitFor(() => screen.getByText(/none of these parent tables/i))
    // The relationship select (the only combobox containing the arrow glyph in
    // its option labels) stays at its placeholder.
    const combos = screen.getAllByRole('combobox')
    const rel = combos.find((c) => c.textContent?.includes('→ pg.customers'))
    expect(rel).toBeDefined()
    expect((rel as HTMLSelectElement).value).toBe('')
  })
})

describe('ColumnAnchorsSection — Topic 4b/4c: empty-state guidance', () => {
  it('renders the guidance block when the prefilled child has zero relationships', async () => {
    renderSection([
      '/?focus=' + encodeURIComponent('pg.payments.tenant_id'),
    ])
    const guidance = await screen.findByTestId('anchor-empty-state-guidance')
    expect(guidance.textContent).toMatch(/no relationships from/i)
    expect(guidance.textContent).toMatch(/pg\.payments/)
    expect(
      screen.getByRole('button', { name: /switch to same-table alias/i }),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('button', { name: /add a relationship/i }),
    ).toBeInTheDocument()
  })

  it('switches to alias mode when "Switch to Same-table alias" is clicked', async () => {
    const user = userEvent.setup()
    renderSection([
      '/?focus=' + encodeURIComponent('pg.payments.tenant_id'),
    ])
    await screen.findByTestId('anchor-empty-state-guidance')
    await user.click(screen.getByRole('button', { name: /switch to same-table alias/i }))
    // The "Actual column on this table" label appears in alias mode.
    await waitFor(() =>
      expect(screen.getByText(/actual column on this table/i)).toBeInTheDocument(),
    )
  })

  it('expands FK suggestions filtered to the child table; accepting one auto-selects it', async () => {
    const user = userEvent.setup()
    const fk: FkSuggestion = {
      child_table_id: 't-payments',
      child_schema_name: 'postgres',
      child_table_name: 'payments',
      child_column_name: 'order_id',
      parent_table_id: 't-orders',
      parent_schema_name: 'postgres',
      parent_table_name: 'orders',
      parent_column_name: 'id',
      fk_constraint_name: 'fk_payments_orders',
      already_added: false,
    }
    const otherFk: FkSuggestion = {
      ...fk,
      child_table_id: 't-orders',
      child_table_name: 'orders',
      fk_constraint_name: 'fk_other',
    }
    mockListFkSuggestions.mockResolvedValue([fk, otherFk])
    mockCreateRelationship.mockImplementation((_dsId, _body) =>
      Promise.resolve({
        ...REL_PAYMENTS_TO_ORDERS,
        id: 'rel-newly-created',
      }),
    )

    renderSection([
      '/?focus=' + encodeURIComponent('pg.payments.tenant_id'),
    ])
    await screen.findByTestId('anchor-empty-state-guidance')
    await user.click(screen.getByRole('button', { name: /add a relationship/i }))

    // Suggestion table appears; only the payments-scoped suggestion is rendered.
    await waitFor(() => expect(screen.getByText('fk_payments_orders')).toBeInTheDocument())
    expect(screen.queryByText('fk_other')).toBeNull()

    // Accept it — once relationships re-fetch with the new rel, it should be auto-set.
    mockListRelationships.mockResolvedValue([
      { ...REL_PAYMENTS_TO_ORDERS, id: 'rel-newly-created' },
    ])
    await user.click(screen.getByRole('button', { name: /^add$/i }))
    await waitFor(() => expect(mockCreateRelationship).toHaveBeenCalled())

    // The relationship dropdown should now be populated and auto-selected to the new rel.
    await waitFor(() => {
      const combos = screen.getAllByRole('combobox')
      const rel = combos.find((c) => c.textContent?.includes('orders.id'))
      expect((rel as HTMLSelectElement).value).toBe('rel-newly-created')
    })
  })

  it('renders the inline manual relationship form alongside FK suggestions', async () => {
    const user = userEvent.setup()
    renderSection([
      '/?focus=' + encodeURIComponent('pg.payments.tenant_id'),
    ])
    await screen.findByTestId('anchor-empty-state-guidance')
    await user.click(screen.getByRole('button', { name: /add a relationship/i }))
    expect(screen.getByTestId('anchor-inline-relationship-form')).toBeInTheDocument()
    expect(
      screen.getByRole('button', { name: /add and use/i }),
    ).toBeInTheDocument()
  })
})
