import { validatePassword } from '../utils/passwordValidation'

export function PasswordStrengthIndicator({ password }: { password: string }) {
  if (!password) return null

  const { checks } = validatePassword(password)

  return (
    <ul className="mt-1.5 space-y-0.5">
      {checks.map((check) => (
        <li
          key={check.label}
          className={`flex items-center gap-1.5 text-xs ${
            check.passed ? 'text-green-600' : 'text-gray-400'
          }`}
        >
          <svg viewBox="0 0 12 12" fill="currentColor" className="w-3 h-3 shrink-0">
            {check.passed ? (
              <path d="M10.293 2.293a1 1 0 011.414 1.414l-6 6a1 1 0 01-1.414 0l-3-3a1 1 0 111.414-1.414L5 7.586l5.293-5.293z" />
            ) : (
              <circle cx="6" cy="6" r="5" stroke="currentColor" strokeWidth="1.5" fill="none" />
            )}
          </svg>
          {check.label}
        </li>
      ))}
    </ul>
  )
}
