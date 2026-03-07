/** Returns an error string, or null if valid. Mirrors backend dto.rs rules. */

// 3–50 chars, starts with a letter, only [a-zA-Z0-9_.-]
export function validateUsername(name: string): string | null {
  if (name.length < 3 || name.length > 50) {
    return 'Must be between 3 and 50 characters'
  }
  if (!/^[a-zA-Z]/.test(name)) {
    return 'Must start with a letter'
  }
  if (!/^[a-zA-Z][a-zA-Z0-9_.\-]*$/.test(name)) {
    return 'Only letters, digits, underscores, dots, and hyphens are allowed'
  }
  return null
}

// 1–64 chars, starts with a letter, only [a-zA-Z0-9_-] (no spaces — SQL identifier)
export function validateDatasourceName(name: string): string | null {
  if (name.length === 0 || name.length > 64) {
    return 'Must be between 1 and 64 characters'
  }
  if (!/^[a-zA-Z]/.test(name)) {
    return 'Must start with a letter'
  }
  if (!/^[a-zA-Z][a-zA-Z0-9_\-]*$/.test(name)) {
    return 'Only letters, digits, underscores, and hyphens are allowed (no spaces)'
  }
  return null
}

// 1–100 chars, no leading/trailing whitespace, only [a-zA-Z0-9 _\-.:()'"]
export function validatePolicyName(name: string): string | null {
  if (name !== name.trim()) {
    return 'Must not have leading or trailing whitespace'
  }
  if (name.trim().length === 0 || name.length > 100) {
    return 'Must be between 1 and 100 characters'
  }
  if (!/^[a-zA-Z0-9 _\-.:()'\"]+$/.test(name)) {
    return "Only letters, digits, spaces, and _ - . : ( ) ' \" are allowed"
  }
  return null
}
