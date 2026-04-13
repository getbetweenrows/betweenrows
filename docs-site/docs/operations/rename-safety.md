---
title: Rename Safety
description: What breaks when you rename a data source or schema alias, what doesn't, and the safe rename procedure.
---

# Rename Safety

BetweenRows uses user-facing names — data source names and schema aliases — as identifiers in connection strings, SQL queries, and decision function context. Renaming them is a **breaking change** for anything that references the old name. This page explains what breaks, what doesn't, and how to rename safely.

## What breaks on a data source rename

The data source `name` is what users put in their connection string as the database name:

```
postgresql://alice:secret@proxy:5434/<datasource-name>
```

Changing it breaks:

| What breaks | Why |
|---|---|
| **Connection strings** | Every user and application must update `dbname=<new-name>` |
| **Stored SQL in applications** | Any code that hardcodes the database name in connection logic |
| **BI tool configurations** | Tableau, Metabase, DBeaver data sources reference the old name |
| **Decision function JS** | Functions that check `ctx.session.datasource.name` or `ctx.query.tables[*].datasource` |
| **Audit log continuity** | Old audit entries have the old name (denormalized at write time); new entries have the new name. Queries spanning the rename show both. |

## What breaks on a schema alias rename

If you change how an upstream schema is aliased in the catalog, it breaks:

| What breaks | Why |
|---|---|
| **User SQL** | `SELECT * FROM old_schema.orders` → "schema not found" |
| **Policy targets** | Policies using `schemas: ["old_name"]` stop matching (but `schemas: ["*"]` still works) |
| **Decision function JS** | Functions checking `ctx.query.tables[*].schema` |

## What does NOT break

| What survives | Why |
|---|---|
| **Policy enforcement** | Policies are assigned by data source UUID and match via the catalog, not the name. A rename does not affect which policies fire. |
| **User access grants** | Grants reference the data source by UUID. |
| **Role assignments** | Same — UUID-based. |
| **Catalog selections** | The saved catalog references upstream schema/table/column names, which don't change when you rename the BetweenRows alias. |

## Safe rename procedure

1. **Announce the rename** to all users and application owners. Give them the new name and a migration window.
2. **Drain active connections.** Ask users to disconnect or wait for idle timeout (`BR_IDLE_TIMEOUT_SECS`, default 900s).
3. **Rename** the data source in the admin UI (edit → change name → save).
4. **Update all connection strings** in applications, BI tools, and scripts.
5. **Update policy targets** if any use explicit schema names that changed (not needed for wildcard `"*"` targets).
6. **Update decision functions** if any reference the old name in JS logic.
7. **Verify** by connecting with the new name and running a query. Check the audit log for the new name.
8. **Re-enable traffic.**

::: warning
There is no "rename alias" feature that redirects the old name to the new one. The rename is immediate and the old name stops working. Plan for a brief outage during the migration window.
:::

## See also

- [Data Sources](/guides/data-sources) — full data source configuration guide
- [Troubleshooting](/operations/troubleshooting) — "connection closed unexpectedly" after a rename
