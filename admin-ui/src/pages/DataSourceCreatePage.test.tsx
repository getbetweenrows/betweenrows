import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders } from '../test/test-utils'
import { DataSourceCreatePage } from './DataSourceCreatePage'
import { makeDataSource, makeDataSourceType } from '../test/factories'

vi.mock('../api/datasources', () => ({
  createDataSource: vi.fn(),
  getDataSourceTypes: vi.fn(),
  testDataSource: vi.fn(),
}))

import { createDataSource, getDataSourceTypes } from '../api/datasources'
const mockCreateDataSource = createDataSource as ReturnType<typeof vi.fn>
const mockGetTypes = getDataSourceTypes as ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.clearAllMocks()
  mockGetTypes.mockResolvedValue([makeDataSourceType()])
})

describe('DataSourceCreatePage', () => {
  it('renders heading and form', async () => {
    renderWithProviders(<DataSourceCreatePage />, { authenticated: true })
    expect(screen.getByRole('heading', { name: /new data source/i })).toBeInTheDocument()
    await waitFor(() => expect(screen.getByRole('combobox')).toBeInTheDocument())
  })

  it('breadcrumb links back to /datasources', async () => {
    const user = userEvent.setup()
    renderWithProviders(<DataSourceCreatePage />, { authenticated: true })
    const link = await screen.findByRole('link', { name: 'Data Sources' })
    expect(link).toHaveAttribute('href', '/datasources')
    await user.click(link)
    expect(mockCreateDataSource).not.toHaveBeenCalled()
  })

  it('submits form and calls createDataSource', async () => {
    const user = userEvent.setup()
    mockCreateDataSource.mockResolvedValue(makeDataSource())

    const { container } = renderWithProviders(<DataSourceCreatePage />, { authenticated: true })
    await waitFor(() => screen.getByRole('combobox'))

    await user.type(screen.getByPlaceholderText(/production-db/i), 'my-db')
    await user.selectOptions(screen.getByRole('combobox'), 'postgres')
    // After type selection, required fields appear. Use fireEvent.submit to bypass
    // HTML5 required validation on empty fields (e.g. 'db').
    fireEvent.submit(container.querySelector('form')!)

    await waitFor(() =>
      expect(mockCreateDataSource).toHaveBeenCalledWith(
        expect.objectContaining({ name: 'my-db', ds_type: 'postgres' }),
      ),
    )
  })
})
