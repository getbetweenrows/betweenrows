import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { getCatalog } from '../api/catalog'
import type { CatalogHints } from '../components/PolicyForm'

export function useCatalogHints(datasourceId: string): CatalogHints | undefined {
  const { data: catalogData } = useQuery({
    queryKey: ['catalog', datasourceId],
    queryFn: () => getCatalog(datasourceId),
    enabled: !!datasourceId,
  })

  return useMemo((): CatalogHints | undefined => {
    if (!catalogData?.schemas?.length) return undefined

    const schemas: string[] = []
    const tables = new Map<string, string[]>()
    const columns = new Map<string, string[]>()

    for (const schema of catalogData.schemas) {
      if (!schema.is_selected) continue
      const effectiveName = schema.schema_alias || schema.schema_name
      schemas.push(effectiveName)
      for (const table of schema.tables) {
        if (!table.is_selected) continue
        const tableList = tables.get(effectiveName) ?? []
        tableList.push(table.table_name)
        tables.set(effectiveName, tableList)
        const colList = table.columns
          .filter((c) => c.is_selected && c.arrow_type !== null)
          .map((c) => c.column_name)
        if (colList.length > 0) {
          columns.set(`${effectiveName}.${table.table_name}`, colList)
        }
      }
    }

    if (schemas.length === 0) return undefined
    return { schemas, tables, columns }
  }, [catalogData])
}
