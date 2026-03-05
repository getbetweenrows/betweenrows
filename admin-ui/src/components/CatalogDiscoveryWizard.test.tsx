import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, within, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { CatalogDiscoveryWizard } from './CatalogDiscoveryWizard'
import {
  makeEmptyCatalog,
  makeDiscoveredSchema,
  makeDiscoveredTable,
  makeDiscoveredColumn,
} from '../test/factories'

vi.mock('../api/catalog', () => ({
  submitAndStream: vi.fn(),
  cancelDiscovery: vi.fn(),
  getCatalog: vi.fn(),
}))

import { submitAndStream, cancelDiscovery, getCatalog } from '../api/catalog'
const mockSubmitAndStream = submitAndStream as ReturnType<typeof vi.fn>
const mockCancelDiscovery = cancelDiscovery as ReturnType<typeof vi.fn>
const mockGetCatalog = getCatalog as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockGetCatalog.mockResolvedValue(makeEmptyCatalog())
  mockCancelDiscovery.mockResolvedValue(undefined)
})

describe('CatalogDiscoveryWizard – idle state', () => {
  it('shows Discover Schemas button when catalog is empty', async () => {
    renderWithProviders(<CatalogDiscoveryWizard datasourceId="ds-1" />)
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /discover schemas/i })).toBeInTheDocument(),
    )
  })

  it('shows catalog summary when catalog has schemas', async () => {
    mockGetCatalog.mockResolvedValue({
      schemas: [
        {
          id: 's-1',
          schema_name: 'public',
          schema_alias: null,
          is_selected: true,
          tables: [
            { id: 't-1', table_name: 'users', table_type: 'TABLE', is_selected: true, columns: [] },
          ],
        },
      ],
    })

    renderWithProviders(<CatalogDiscoveryWizard datasourceId="ds-1" />)
    // The catalog summary renders text split by <strong> tags.
    // Check the container's combined text content instead.
    await waitFor(() => {
      const body = document.body.textContent ?? ''
      expect(body).toMatch(/1.*schema/i)
      expect(body).toMatch(/1.*table/i)
    })
  })
})

describe('CatalogDiscoveryWizard – Step 1 (schemas)', () => {
  const schema1 = makeDiscoveredSchema({ schema_name: 'public', is_already_selected: true })
  const schema2 = makeDiscoveredSchema({ schema_name: 'analytics', is_already_selected: false })

  beforeEach(() => {
    mockSubmitAndStream.mockResolvedValue([schema1, schema2])
  })

  async function goToSchemas() {
    const user = userEvent.setup()
    renderWithProviders(<CatalogDiscoveryWizard datasourceId="ds-1" />)
    await waitFor(() => screen.getByRole('button', { name: /discover schemas/i }))
    await user.click(screen.getByRole('button', { name: /discover schemas/i }))
    await waitFor(() => screen.getByText('public'))
    return user
  }

  it('displays discovered schemas', async () => {
    await goToSchemas()
    expect(screen.getByText('public')).toBeInTheDocument()
    expect(screen.getByText('analytics')).toBeInTheDocument()
  })

  it('pre-selects is_already_selected schemas', async () => {
    await goToSchemas()
    // Find schema rows by the schema name <span>, then get the checkbox within its parent row
    const publicSpan = screen.getByText('public')
    const publicRow = publicSpan.closest('div[class*="flex items-center gap-2 px-3 py-2"]')
    const publicCheckbox = within(publicRow as HTMLElement).getByRole('checkbox')
    expect(publicCheckbox).toBeChecked()

    const analyticsSpan = screen.getByText('analytics')
    const analyticsRow = analyticsSpan.closest('div[class*="flex items-center gap-2 px-3 py-2"]')
    const analyticsCheckbox = within(analyticsRow as HTMLElement).getByRole('checkbox')
    expect(analyticsCheckbox).not.toBeChecked()
  })

  it('Next button is disabled when no schemas selected', async () => {
    mockSubmitAndStream.mockResolvedValue([schema2]) // schema2 is NOT already selected
    const user = userEvent.setup()
    renderWithProviders(<CatalogDiscoveryWizard datasourceId="ds-1" />)
    await waitFor(() => screen.getByRole('button', { name: /discover schemas/i }))
    await user.click(screen.getByRole('button', { name: /discover schemas/i }))
    await waitFor(() => screen.getByText('analytics'))

    expect(screen.getByRole('button', { name: /next: discover tables/i })).toBeDisabled()
  })

  it('search filter narrows the list', async () => {
    const user = await goToSchemas()
    await user.type(screen.getByPlaceholderText(/search schemas/i), 'ana')

    expect(screen.queryByText('public')).not.toBeInTheDocument()
    expect(screen.getByText('analytics')).toBeInTheDocument()
  })

  it('alias input accepts text', async () => {
    const user = await goToSchemas()
    const aliasInputs = screen.getAllByPlaceholderText(/alias/i)
    await user.type(aliasInputs[0], 'pub')
    expect(aliasInputs[0]).toHaveValue('pub')
  })
})

describe('CatalogDiscoveryWizard – Step 2 (tables)', () => {
  const schema = makeDiscoveredSchema({ schema_name: 'public', is_already_selected: true })
  const table1 = makeDiscoveredTable({ schema_name: 'public', table_name: 'users', is_already_selected: true })
  const table2 = makeDiscoveredTable({ schema_name: 'public', table_name: 'orders', is_already_selected: false })

  async function goToTables() {
    const user = userEvent.setup()
    mockSubmitAndStream
      .mockResolvedValueOnce([schema])
      .mockResolvedValueOnce([table1, table2])
    renderWithProviders(<CatalogDiscoveryWizard datasourceId="ds-1" />)
    await waitFor(() => screen.getByRole('button', { name: /discover schemas/i }))
    await user.click(screen.getByRole('button', { name: /discover schemas/i }))
    await waitFor(() => screen.getByRole('button', { name: /next: discover tables/i }))
    await user.click(screen.getByRole('button', { name: /next: discover tables/i }))
    await waitFor(() => screen.getByText('users'))
    return user
  }

  it('shows tables grouped by schema', async () => {
    await goToTables()
    expect(screen.getByText('users')).toBeInTheDocument()
    expect(screen.getByText('orders')).toBeInTheDocument()
    // Schema header renders lowercase text (CSS uppercase is just a class)
    expect(screen.getByText('public')).toBeInTheDocument()
  })

  it('pre-selects is_already_selected tables', async () => {
    await goToTables()
    // users is pre-selected; find checkbox inside the users label
    const usersLabel = screen.getByText('users').closest('label')!
    expect(within(usersLabel).getByRole('checkbox')).toBeChecked()

    const ordersLabel = screen.getByText('orders').closest('label')!
    expect(within(ordersLabel).getByRole('checkbox')).not.toBeChecked()
  })

  it('back button returns to schema step', async () => {
    const user = await goToTables()
    await user.click(screen.getByRole('button', { name: /^back$/i }))
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /next: discover tables/i })).toBeInTheDocument(),
    )
  })

  it('table search filters visible tables', async () => {
    const user = await goToTables()
    await user.type(screen.getByPlaceholderText(/search tables/i), 'order')
    expect(screen.queryByText('users')).not.toBeInTheDocument()
    expect(screen.getByText('orders')).toBeInTheDocument()
  })
})

describe('CatalogDiscoveryWizard – Step 3 (columns)', () => {
  const schema = makeDiscoveredSchema({ schema_name: 'public', is_already_selected: true })
  const table = makeDiscoveredTable({ schema_name: 'public', table_name: 'users', is_already_selected: true })
  const colId = makeDiscoveredColumn({
    schema_name: 'public', table_name: 'users', column_name: 'id',
    ordinal_position: 1, arrow_type: 'Int64', is_already_selected: false,
  })
  const colName = makeDiscoveredColumn({
    schema_name: 'public', table_name: 'users', column_name: 'name',
    ordinal_position: 2, arrow_type: 'Utf8', is_already_selected: false,
  })
  const colJsonb = makeDiscoveredColumn({
    schema_name: 'public', table_name: 'users', column_name: 'meta',
    ordinal_position: 3, arrow_type: null, is_already_selected: false,
  })

  async function goToColumns(columns = [colId, colName, colJsonb]) {
    const user = userEvent.setup()
    mockSubmitAndStream
      .mockResolvedValueOnce([schema])
      .mockResolvedValueOnce([table])
      .mockResolvedValueOnce(columns)
    renderWithProviders(<CatalogDiscoveryWizard datasourceId="ds-1" />)
    await waitFor(() => screen.getByRole('button', { name: /discover schemas/i }))
    await user.click(screen.getByRole('button', { name: /discover schemas/i }))
    await waitFor(() => screen.getByRole('button', { name: /next: discover tables/i }))
    await user.click(screen.getByRole('button', { name: /next: discover tables/i }))
    await waitFor(() => screen.getByRole('button', { name: /next: discover columns/i }))
    await user.click(screen.getByRole('button', { name: /next: discover columns/i }))
    await waitFor(() => screen.getByRole('button', { name: /^select all$/i }))
    return user
  }

  it('first-time: selects all supported columns (arrow_type !== null)', async () => {
    await goToColumns()
    // 2 supported (id, name), 1 unsupported (meta) → "2/2 cols selected"
    await waitFor(() => {
      const body = document.body.textContent ?? ''
      expect(body).toMatch(/2\/2 cols selected/i)
    })
  })

  it('re-discovery: selects only is_already_selected columns', async () => {
    const col1 = { ...colId, is_already_selected: true }
    const col2 = { ...colName, is_already_selected: false }
    const col3 = { ...colJsonb, is_already_selected: false }
    await goToColumns([col1, col2, col3])
    // Only col1 selected (1), supported are col1+col2 (2), so "1/2 cols selected"
    await waitFor(() => {
      const body = document.body.textContent ?? ''
      expect(body).toMatch(/1\/2 cols selected/i)
    })
  })

  it('unsupported columns are rendered with disabled checkbox', async () => {
    await goToColumns()
    // In the right panel, meta column has arrow_type=null → checkbox disabled
    await waitFor(() => screen.getByText('meta'))
    const metaRow = screen.getByText('meta').closest('tr')!
    const checkbox = within(metaRow).getByRole('checkbox') as HTMLInputElement
    expect(checkbox.disabled).toBe(true)
  })

  it('Deselect All then Select All restores all supported columns', async () => {
    const user = await goToColumns([colId, colName])
    // Both supported, initially both selected → "2/2"
    await waitFor(() => {
      expect(document.body.textContent).toMatch(/2\/2 cols selected/i)
    })

    await user.click(screen.getByRole('button', { name: /^deselect all$/i }))
    await waitFor(() => {
      expect(document.body.textContent).toMatch(/0\/2 cols selected/i)
    })

    await user.click(screen.getByRole('button', { name: /^select all$/i }))
    await waitFor(() => {
      expect(document.body.textContent).toMatch(/2\/2 cols selected/i)
    })
  })
})

describe('CatalogDiscoveryWizard – Save Catalog', () => {
  it('assembles correct payload shape and resets to idle', async () => {
    const schema = makeDiscoveredSchema({ schema_name: 'public', is_already_selected: true })
    const table = makeDiscoveredTable({ schema_name: 'public', table_name: 'users', is_already_selected: true })
    const col = makeDiscoveredColumn({
      schema_name: 'public', table_name: 'users', column_name: 'id', arrow_type: 'Int64',
    })

    const user = userEvent.setup()
    mockSubmitAndStream
      .mockResolvedValueOnce([schema])
      .mockResolvedValueOnce([table])
      .mockResolvedValueOnce([col])
      .mockResolvedValueOnce(undefined) // save_catalog

    renderWithProviders(<CatalogDiscoveryWizard datasourceId="ds-1" />)
    await waitFor(() => screen.getByRole('button', { name: /discover schemas/i }))
    await user.click(screen.getByRole('button', { name: /discover schemas/i }))
    await waitFor(() => screen.getByRole('button', { name: /next: discover tables/i }))
    await user.click(screen.getByRole('button', { name: /next: discover tables/i }))
    await waitFor(() => screen.getByRole('button', { name: /next: discover columns/i }))
    await user.click(screen.getByRole('button', { name: /next: discover columns/i }))
    await waitFor(() => screen.getByRole('button', { name: /save catalog/i }))
    await user.click(screen.getByRole('button', { name: /save catalog/i }))

    // Should reset to idle
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /discover schemas/i })).toBeInTheDocument(),
    )

    const saveCatalogCall = mockSubmitAndStream.mock.calls.find(
      (call: unknown[]) => (call[1] as { action: string }).action === 'save_catalog',
    )
    expect(saveCatalogCall).toBeTruthy()
    const savePayload = (saveCatalogCall as unknown[])[1] as { action: string; schemas: unknown[] }
    expect(savePayload.schemas).toHaveLength(1)
  })
})

describe('CatalogDiscoveryWizard – Cancel', () => {
  it('shows Cancel button while working and resets to idle on click', async () => {
    let resolveStream!: (v: unknown) => void
    mockSubmitAndStream.mockReturnValue(new Promise((r) => { resolveStream = r }))

    const user = userEvent.setup()
    renderWithProviders(<CatalogDiscoveryWizard datasourceId="ds-1" />)
    await waitFor(() => screen.getByRole('button', { name: /discover schemas/i }))
    await user.click(screen.getByRole('button', { name: /discover schemas/i }))

    // While working, a Cancel button appears in the progress bar
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^cancel$/i })).toBeInTheDocument(),
    )
    await user.click(screen.getByRole('button', { name: /^cancel$/i }))

    // After cancel, wizard resets to idle
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /discover schemas/i })).toBeInTheDocument(),
    )

    // Resolve the pending promise inside act to suppress act() warnings
    await act(async () => { resolveStream([]) })
  })
})
