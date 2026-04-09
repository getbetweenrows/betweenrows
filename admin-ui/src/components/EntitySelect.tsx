import { useState, useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  Combobox,
  ComboboxInput,
  ComboboxOption,
  ComboboxOptions,
} from '@headlessui/react'
import { useDebounce } from '../hooks/useDebounce'
import type { EntityOption } from '../utils/entitySearchFns'

interface EntitySelectProps {
  label: string
  value: string
  onChange: (id: string) => void
  searchFn: (search: string) => Promise<EntityOption[]>
  placeholder?: string
  showId?: boolean
}

export function EntitySelect({
  label,
  value,
  onChange,
  searchFn,
  placeholder = 'Search…',
  showId = false,
}: EntitySelectProps) {
  const [query, setQuery] = useState('')
  const debouncedQuery = useDebounce(query, 300)

  const { data: options = [], isFetching } = useQuery({
    queryKey: ['entity-select', label, debouncedQuery],
    queryFn: () => searchFn(debouncedQuery),
    staleTime: 30_000,
  })

  const selectedOption = options.find((o) => o.id === value) ?? null

  const handleChange = useCallback(
    (option: EntityOption | null) => {
      onChange(option?.id ?? '')
    },
    [onChange],
  )

  function handleClear() {
    onChange('')
    setQuery('')
  }

  return (
    <div>
      <label className="block text-xs font-medium text-gray-600 mb-1">{label}</label>
      <Combobox value={selectedOption} onChange={handleChange} by="id">
        <div className="relative">
          <ComboboxInput
            className="w-full border border-gray-300 rounded px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500 pr-14"
            displayValue={(option: EntityOption | null) => option?.label ?? ''}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={placeholder}
          />
          <div className="absolute inset-y-0 right-0 flex items-center pr-1.5 gap-0.5">
            {isFetching && (
              <svg
                className="animate-spin h-3 w-3 text-gray-400"
                viewBox="0 0 24 24"
                fill="none"
              >
                <circle
                  className="opacity-25"
                  cx="12"
                  cy="12"
                  r="10"
                  stroke="currentColor"
                  strokeWidth="4"
                />
                <path
                  className="opacity-75"
                  fill="currentColor"
                  d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
                />
              </svg>
            )}
            {value && (
              <button
                type="button"
                onClick={handleClear}
                className="text-gray-400 hover:text-gray-600 p-0.5"
                aria-label={`Clear ${label}`}
              >
                <svg className="h-3 w-3" viewBox="0 0 20 20" fill="currentColor">
                  <path
                    fillRule="evenodd"
                    d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z"
                    clipRule="evenodd"
                  />
                </svg>
              </button>
            )}
          </div>
          <ComboboxOptions
            anchor="bottom start"
            className="w-[var(--input-width)] mt-1 bg-white border border-gray-200 rounded shadow-lg max-h-60 overflow-auto z-50 empty:hidden"
          >
            {options.map((option) => (
              <ComboboxOption
                key={option.id}
                value={option}
                className="px-2 py-1.5 text-xs cursor-pointer data-[focus]:bg-blue-50 data-[selected]:font-medium"
              >
                <span>{option.label}</span>
                {showId && (
                  <span className="ml-2 text-gray-400 font-mono text-[10px]">
                    {option.id.slice(0, 8)}…
                  </span>
                )}
              </ComboboxOption>
            ))}
            {!isFetching && debouncedQuery && options.length === 0 && (
              <div className="px-2 py-1.5 text-xs text-gray-400">No results</div>
            )}
          </ComboboxOptions>
        </div>
      </Combobox>
    </div>
  )
}
