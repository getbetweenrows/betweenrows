/// Display rule for schema names across the admin UI.
///
/// When a schema has a non-empty alias, the alias is the user-facing name
/// (it's what `useCatalogHints` already uses for policy targets, and what
/// the proxy keys columns by in the resolution graph). The raw upstream
/// `schema_name` is shown in muted parentheses next to the table label so
/// admins can verify the mapping without bouncing to the discovery wizard.
export function effectiveSchemaName(
  schemaName: string,
  schemaAlias?: string | null,
): string {
  if (schemaAlias && schemaAlias.trim()) return schemaAlias
  return schemaName
}
