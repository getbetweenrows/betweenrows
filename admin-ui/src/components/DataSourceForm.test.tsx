import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { DataSourceForm } from './DataSourceForm'
import { makeDataSourceType } from '../test/factories'

vi.mock('../api/datasources', () => ({
  getDataSourceTypes: vi.fn(),
  testDataSource: vi.fn(),
}))

import { getDataSourceTypes, testDataSource } from '../api/datasources'
const mockGetTypes = getDataSourceTypes as ReturnType<typeof vi.fn>
const mockTestDataSource = testDataSource as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockGetTypes.mockResolvedValue([makeDataSourceType()])
})

describe('DataSourceForm – create mode', () => {
  it('renders name input and type selector', async () => {
    renderWithProviders(<DataSourceForm onSubmit={vi.fn()} />)
    await waitFor(() => expect(screen.getByPlaceholderText(/production-db/i)).toBeInTheDocument())
    expect(screen.getByRole('combobox')).toBeInTheDocument()
  })

  it('populates dynamic fields with defaults when a type is selected', async () => {
    const user = userEvent.setup()
    renderWithProviders(<DataSourceForm onSubmit={vi.fn()} />)
    await waitFor(() => screen.getByRole('combobox'))

    await user.selectOptions(screen.getByRole('combobox'), 'postgres')

    // Host field gets default_value 'localhost'; check by display value
    await waitFor(() =>
      expect(screen.getByDisplayValue('localhost')).toBeInTheDocument(),
    )
    // Port field gets default_value '5432'
    expect(screen.getByDisplayValue('5432')).toBeInTheDocument()
  })

  it('shows error when name is empty on submit (bypass HTML5 validation)', async () => {
    const { container } = renderWithProviders(<DataSourceForm onSubmit={vi.fn()} />)
    await waitFor(() => screen.getByRole('combobox'))

    // Select a type first so dsType is set
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'postgres' } })

    // Submit form directly to bypass HTML5 required validation
    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() =>
      expect(screen.getByText(/name is required/i)).toBeInTheDocument(),
    )
  })

  it('shows error when no type selected (bypass HTML5 validation)', async () => {
    const { container } = renderWithProviders(<DataSourceForm onSubmit={vi.fn()} />)
    await waitFor(() => screen.getByRole('combobox'))

    // Fill name but leave type empty
    await userEvent.setup().type(screen.getByPlaceholderText(/production-db/i), 'mydb')

    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() =>
      expect(screen.getByText(/please select a data source type/i)).toBeInTheDocument(),
    )
  })

  it('does not show Test Connection button without datasourceId', async () => {
    renderWithProviders(<DataSourceForm onSubmit={vi.fn()} />)
    await waitFor(() => screen.getByRole('combobox'))
    expect(screen.queryByRole('button', { name: /test connection/i })).not.toBeInTheDocument()
  })
})

describe('DataSourceForm – edit mode', () => {
  it('disables the type selector', async () => {
    renderWithProviders(
      <DataSourceForm
        datasourceId="ds-1"
        initialValues={{ name: 'prod', ds_type: 'postgres', config: { host: 'localhost' } }}
        onSubmit={vi.fn()}
      />,
    )
    await waitFor(() => screen.getByRole('combobox'))
    expect(screen.getByRole('combobox')).toBeDisabled()
  })

  it('shows Test Connection button', async () => {
    renderWithProviders(
      <DataSourceForm
        datasourceId="ds-1"
        initialValues={{ ds_type: 'postgres' }}
        onSubmit={vi.fn()}
      />,
    )
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /test connection/i })).toBeInTheDocument(),
    )
  })

  it('calls testDataSource and shows success result', async () => {
    const user = userEvent.setup()
    mockTestDataSource.mockResolvedValue({ success: true, message: 'OK' })

    renderWithProviders(
      <DataSourceForm
        datasourceId="ds-1"
        initialValues={{ ds_type: 'postgres' }}
        onSubmit={vi.fn()}
      />,
    )
    await waitFor(() => screen.getByRole('button', { name: /test connection/i }))

    await user.click(screen.getByRole('button', { name: /test connection/i }))

    await waitFor(() => expect(screen.getByText(/✓ connected/i)).toBeInTheDocument())
    expect(mockTestDataSource).toHaveBeenCalledWith('ds-1')
  })
})
