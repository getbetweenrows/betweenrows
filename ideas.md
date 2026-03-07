# Ideas

## Allow testing/preview policy before deployment

Allow sudo as a user to test out the policy if the policy isn't for the admin.

## Visibility follows Access

we want to differentiate metadata access (virtual schema) and data access (cells, values), and the philosophy is "Visibility follows Access.", if you can access a schema/table/column, then you can see them in your sql client sidebar. To avoid evaluating the policies twice (once at connection time to render the virtual schema, once at query time to filter), we can have store CompiledVisibility in admin store, it is updated every time a policy is created/updated, we need to see what is best way to keep it always up to date, database level trigger vs code in api.
We also need to discuss what the schema should look like to store these pre-compiled visibility.

## User name, datasource name, policy name validation in the UI

right now we only have hints but not live validation

## Improve hints in the UI

Like create user, datasource, policies, obligations, assignment, i feel we lack guidance and hints that can explain to users what each config does, risk assotiated with the configs, recommended config/value eg priority value etc.

## Query Audit Log UI Improvements

- Search filters should support username and datasource name, and policy id/name. Search by keyword in query, or Maybe even consider as full-text search (or might be overkill)
- Log table UI should allow toggle on/off columns (consider an icon at the top right corner that users can toggle what fields to display), do we need to remember the preferences?
- Log table ui should handle pagination, we will have a lot of logs.

## The Unified E2E Strategy

We will split the testing into two distinct "flows" that both run in your CI/CD pipeline. One focuses on the User Experience (Browser → API) and the other on the Infrastructure (SQL Client → Proxy → DB).1. Web UI & API (The Playwright Track)Goal: Ensure the React frontend correctly communicates with the Rust REST/GraphQL API.Tool: Playwright.Mechanism: Playwright spins up the React dev server and the Rust binary. It mimics a real user clicking buttons, filling forms, and verifying that the Rust backend saves data correctly to the DB.Why: It handles the "flakiness" of web UI (waiting for elements to load) better than any other tool.2. TCP SQL Proxy (The psql Track)Goal: Ensure the Rust TCP server correctly proxies the Postgres Wire Protocol without dropping packets or mangling queries.Tool: psql (CLI).Mechanism: A shell script (or Playwright execSync) runs a real SQL query against the Proxy Port (e.g., 5433) instead of the database port (5432).Validation:Bashpsql -h localhost -p 5433 -U test_user -d test_db -c "SELECT 1;"
If this returns 1, your Rust proxy is successfully handling the handshake, authentication, and data frames.🚀 GitHub Actions Implementation (Summary)To make this work in one workflow, your .github/workflows/e2e.yml would look like this:StageActionServicesStart a Postgres Docker container in the "services" section.BuildBuild your Rust binary and install Node dependencies.SetupRun your Rust migrations against the Docker DB.Test (UI)Run npx playwright test. Playwright starts your UI and API.Test (Proxy)Run the psql command pointing to your Rust Proxy port.ArtifactsUpload the Playwright Trace Viewer files if any UI test fails.

## Performance of PolicyHook

For very complex queries or a large number of active policies, the transform_up operations on the logical plan could potentially introduce overhead. It would be important to ensure that performance benchmarks have been run with a high volume of policies and complex queries.

## ALTER TABLE ADD COLUMN Idempotency

The migration rules explicitly mention that ALTER TABLE ADD COLUMN has no idempotency guard in SeaORM and users must not interrupt this migration. While this is documented, it's a potential point of failure. Is there any way to mitigate this at the framework level or provide more robust guidance/tooling to users for such migrations?

## Testing Strategy

Given the complexity of the new policy system, particularly its interaction with DataFusion and PostgreSQL, a detailed security test plan (as mentioned in docs/permission-security-tests.md) is crucial. This plan should include:
_ Edge cases for policy conflicts and priority resolution.
_ Negative tests to ensure policy bypasses are not possible.
_ Performance tests for policy application under load.
_ Tests for YAML import/export, especially for malformed YAML or security implications (e.g., injecting malicious SQL via policy definitions).

## Error Handling for Policy Definitions

How are errors in filter_expression or mask_expression handled at runtime? Is there robust validation during policy creation/update to prevent syntactically incorrect or semantically invalid expressions from being saved? The summary mentions "Definition validation (parse expressions, check catalog references)" in Phase D, which is good.

## User Experience for Policy Creation (Admin UI)

For row_filter and column_mask policies, the expressions are raw SQL/DataFusion expressions. While powerful, this can be complex for less technical administrators. Are there plans for a more guided or templated approach in the UI for common scenarios, or a DSL that simplifies expression writing?

# Others

New features:

- forget password and reset password, 2FA or OTP

Items to improve:

- sometimes sql queries take long time and cause UI to hang, do some performance test, maybe because we missed index?
- ssl mode default prefer, also research see if we have the comprehensive options for postgres ssl, also see if the naming of the options are following standards.
- we expose a bunch a pg\_ system tables to users, i assume they are from datafusion or pgwire, how does that work, what are the list of tables they support in the postgres setup, and any security concerns with exposing them, can we restrict them.
- in pg_database, we see the original source database name instead of the virtual database name. the original source database name is also exposed in error logs in sql client, like postgres.<schema>.<table> not found. anyway to show the virtual database name instead? any existing solution or best practice?

Bugs

2026-03-04T00:10:49.516017Z ERROR proxy::handler: DataFusion query error error=Error during planning: Invalid function 'pg_get_function_identity_arguments'.
Did you mean 'pg_get_statisticsobjdef_columns'?
2026-03-04T00:10:52.422806Z ERROR proxy::handler: DataFusion query error error=Error during planning: table 'postgres.pg_catalog.pg_statio_user_tables' not found
2026-03-04T00:10:52.424390Z ERROR proxy::handler: DataFusion query error error=Error during planning: table 'postgres.information_schema.table_constraints' not found
2026-03-04T00:10:52.430341Z ERROR proxy::handler: DataFusion query error error=Error during planning: Invalid function 'quote_ident'.
Did you mean 'date_bin'?

Next milestones

- Configurable policies
  - Configure policies, RLS, per user access access to schema, table, columns. data masking etc. assign by user or by group or by role? decisions to be made. we may also want the rules/policies/tags be able to represent in a single yaml file, so it can be configured as code (possibly proving a new CLI experience for developers?) without using the UI? the as code approach will allow version control too.
  - also need to think about how to keep audit history for all the policy updates. so we know when who changed what.
- performance test
  - large tables
  - expensive queries
- security penetration testing
  - sql injection
  - SSL
- UI/UX Redesign
  - only about look, user experience. not related to fundamental product feature.
