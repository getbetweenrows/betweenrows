import '@testing-library/jest-dom/vitest'

// Stub window.confirm — returns true by default so delete handlers proceed
vi.stubGlobal('confirm', vi.fn(() => true))

// In-memory localStorage mock
const store: Record<string, string> = {}
const localStorageMock = {
  getItem: (key: string) => store[key] ?? null,
  setItem: (key: string, value: string) => { store[key] = value },
  removeItem: (key: string) => { delete store[key] },
  clear: () => { Object.keys(store).forEach((k) => delete store[k]) },
  get length() { return Object.keys(store).length },
  key: (index: number) => Object.keys(store)[index] ?? null,
}
vi.stubGlobal('localStorage', localStorageMock)

afterEach(() => {
  localStorageMock.clear()
  vi.clearAllMocks()
})
