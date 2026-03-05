import { describe, it, expect } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { AuthProvider, useAuth } from './AuthContext'
import { makeUser } from '../test/factories'

function wrapper({ children }: { children: React.ReactNode }) {
  return <AuthProvider>{children}</AuthProvider>
}

describe('AuthProvider', () => {
  it('initializes token and user as null when localStorage is empty', () => {
    const { result } = renderHook(() => useAuth(), { wrapper })
    expect(result.current.token).toBeNull()
    expect(result.current.user).toBeNull()
  })

  it('initializes from localStorage when token+user present', () => {
    const user = makeUser()
    localStorage.setItem('token', 'saved-token')
    localStorage.setItem('user', JSON.stringify(user))

    const { result } = renderHook(() => useAuth(), { wrapper })
    expect(result.current.token).toBe('saved-token')
    expect(result.current.user).toEqual(user)
  })

  it('signIn stores token+user in localStorage and state', () => {
    const { result } = renderHook(() => useAuth(), { wrapper })
    const user = makeUser()

    act(() => result.current.signIn('new-token', user))

    expect(result.current.token).toBe('new-token')
    expect(result.current.user).toEqual(user)
    expect(localStorage.getItem('token')).toBe('new-token')
    expect(JSON.parse(localStorage.getItem('user') ?? 'null')).toEqual(user)
  })

  it('signOut clears token+user from localStorage and state', () => {
    const user = makeUser()
    localStorage.setItem('token', 'tok')
    localStorage.setItem('user', JSON.stringify(user))

    const { result } = renderHook(() => useAuth(), { wrapper })

    act(() => result.current.signOut())

    expect(result.current.token).toBeNull()
    expect(result.current.user).toBeNull()
    expect(localStorage.getItem('token')).toBeNull()
    expect(localStorage.getItem('user')).toBeNull()
  })
})

describe('useAuth', () => {
  it('throws when used outside AuthProvider', () => {
    expect(() => renderHook(() => useAuth())).toThrow('useAuth must be used within AuthProvider')
  })
})
