import { describe, it, expect } from 'vitest'
import { effectiveSchemaName } from './schemaLabel'

describe('effectiveSchemaName', () => {
  it('returns alias when set', () => {
    expect(effectiveSchemaName('postgres', 'pg')).toBe('pg')
  })

  it('falls back to raw name when alias is null', () => {
    expect(effectiveSchemaName('public', null)).toBe('public')
  })

  it('falls back to raw name when alias is undefined', () => {
    expect(effectiveSchemaName('public')).toBe('public')
  })

  it('falls back to raw name when alias is empty string', () => {
    expect(effectiveSchemaName('public', '')).toBe('public')
  })

  it('falls back to raw name when alias is whitespace', () => {
    expect(effectiveSchemaName('public', '   ')).toBe('public')
  })
})
