import { useQuery } from '@tanstack/react-query'
import { getVersion } from '../api/version'

export function useVersion() {
  return useQuery({
    queryKey: ['version'],
    queryFn: getVersion,
    staleTime: 24 * 60 * 60 * 1000,
    refetchOnWindowFocus: false,
  })
}
