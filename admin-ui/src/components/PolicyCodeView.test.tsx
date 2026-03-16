import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, fireEvent } from '@testing-library/react'
import { renderWithProviders } from '../test/test-utils'
import { makePolicy, makePolicyAssignment } from '../test/factories'
import { PolicyCodeView } from './PolicyCodeView'

beforeEach(() => {
  vi.clearAllMocks()
  Object.assign(navigator, {
    clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
  })
})

function renderCodeView(
  policyOverrides = {},
  assignments: ReturnType<typeof makePolicyAssignment>[] = [],
) {
  const policy = makePolicy(policyOverrides)
  return { policy, ...renderWithProviders(<PolicyCodeView policy={policy} assignments={assignments} />) }
}

describe('PolicyCodeView', () => {
  it('starts collapsed — no <pre> visible', () => {
    renderCodeView()
    expect(document.querySelector('pre')).toBeNull()
  })

  it('expands when "View as code" is clicked', () => {
    renderCodeView()
    fireEvent.click(screen.getByText('View as code'))
    expect(document.querySelector('pre')).toBeInTheDocument()
  })

  it('defaults to YAML format', () => {
    renderCodeView()
    fireEvent.click(screen.getByText('View as code'))
    const pre = document.querySelector('pre')!
    expect(pre.textContent).toContain('name:')
    expect(pre.textContent).toContain('policy_type:')
  })

  it('shows valid JSON when JSON toggle is clicked', () => {
    renderCodeView({ name: 'test-policy', policy_type: 'row_filter' })
    fireEvent.click(screen.getByText('View as code'))
    fireEvent.click(screen.getByText('JSON'))
    const pre = document.querySelector('pre')!
    expect(() => JSON.parse(pre.textContent!)).not.toThrow()
    const parsed = JSON.parse(pre.textContent!)
    expect(parsed.name).toBe('test-policy')
    expect(parsed.policy_type).toBe('row_filter')
  })

  it('copy button calls navigator.clipboard.writeText', () => {
    renderCodeView()
    fireEvent.click(screen.getByText('View as code'))
    fireEvent.click(screen.getByText('Copy'))
    expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(1)
  })

  it('code includes policy name, policy_type, and version', () => {
    renderCodeView({ name: 'my-policy', policy_type: 'column_deny', version: 3 })
    fireEvent.click(screen.getByText('View as code'))
    fireEvent.click(screen.getByText('JSON'))
    const parsed = JSON.parse(document.querySelector('pre')!.textContent!)
    expect(parsed.name).toBe('my-policy')
    expect(parsed.policy_type).toBe('column_deny')
    expect(parsed.version).toBe(3)
  })

  it('code includes targets and definition', () => {
    const policy = makePolicy({
      policy_type: 'row_filter',
      targets: [{ schemas: ['public'], tables: ['orders'] }],
      definition: { filter_expression: 'tenant = 1' },
    })
    renderWithProviders(<PolicyCodeView policy={policy} assignments={[]} />)
    fireEvent.click(screen.getByText('View as code'))
    fireEvent.click(screen.getByText('JSON'))
    const parsed = JSON.parse(document.querySelector('pre')!.textContent!)
    expect(parsed.targets).toHaveLength(1)
    expect(parsed.definition.filter_expression).toBe('tenant = 1')
  })

  it('assignments show datasource and user names', () => {
    const a = makePolicyAssignment({
      data_source_id: 'ds-1',
      datasource_name: 'prod-db',
      username: 'alice',
    })
    const policy = makePolicy()
    renderWithProviders(<PolicyCodeView policy={policy} assignments={[a]} />)
    fireEvent.click(screen.getByText('View as code'))
    fireEvent.click(screen.getByText('JSON'))
    const parsed = JSON.parse(document.querySelector('pre')!.textContent!)
    expect(parsed.assignments).toHaveLength(1)
    expect(parsed.assignments[0].datasource).toBe('prod-db')
    expect(parsed.assignments[0].user).toBe('alice')
  })
})
