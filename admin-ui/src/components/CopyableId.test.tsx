import { describe, it, expect, vi, afterEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { CopyableId } from './CopyableId'
import { renderWithProviders } from '../test/test-utils'

const UUID = 'aaa-111-bbb-222-ccc-333'

describe('CopyableId', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('renders full UUID text', () => {
    renderWithProviders(<CopyableId id={UUID} />)
    expect(screen.getByText(UUID)).toBeInTheDocument()
  })

  it('renders truncated UUID when short=true', () => {
    renderWithProviders(<CopyableId id={UUID} short />)
    expect(document.body.textContent).toMatch(/aaa-111-/)
    // Full UUID should be in the title for hover
    expect(screen.getByTitle(UUID)).toBeInTheDocument()
  })

  it('copies to clipboard on click', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.assign(navigator, { clipboard: { writeText } })

    renderWithProviders(<CopyableId id={UUID} />)
    await userEvent.click(screen.getByText(UUID))

    expect(writeText).toHaveBeenCalledWith(UUID)
  })

  it('shows check icon after copy then reverts', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.assign(navigator, { clipboard: { writeText } })

    const { container } = renderWithProviders(<CopyableId id={UUID} />)

    // Initially shows copy icon (two paths)
    const svgsBefore = container.querySelectorAll('svg')
    expect(svgsBefore).toHaveLength(1)
    expect(svgsBefore[0].querySelectorAll('path')).toHaveLength(2)

    await userEvent.click(screen.getByText(UUID))

    // After click shows check icon (one path)
    await waitFor(() => {
      const svgsAfter = container.querySelectorAll('svg')
      expect(svgsAfter[0].querySelectorAll('path')).toHaveLength(1)
    })

    await vi.advanceTimersByTimeAsync(2100)

    // Reverts to copy icon
    await waitFor(() => {
      const svgsReverted = container.querySelectorAll('svg')
      expect(svgsReverted[0].querySelectorAll('path')).toHaveLength(2)
    })

    vi.useRealTimers()
  })
})
