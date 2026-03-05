export interface PasswordValidation {
  valid: boolean
  checks: { label: string; passed: boolean }[]
}

export function validatePassword(pw: string): PasswordValidation {
  const checks = [
    { label: 'At least 8 characters', passed: pw.length >= 8 },
    { label: 'One uppercase letter', passed: /[A-Z]/.test(pw) },
    { label: 'One lowercase letter', passed: /[a-z]/.test(pw) },
    { label: 'One digit', passed: /[0-9]/.test(pw) },
    { label: 'One special character', passed: /[^A-Za-z0-9]/.test(pw) },
  ]
  return { valid: checks.every((c) => c.passed), checks }
}
