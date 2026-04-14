# BetweenRows

[![CI](https://github.com/getbetweenrows/betweenrows/actions/workflows/cicd.yml/badge.svg)](https://github.com/getbetweenrows/betweenrows/actions)
[![Version](https://img.shields.io/github/v/tag/getbetweenrows/betweenrows?label=Version)](https://github.com/getbetweenrows/betweenrows/tags)
[![Status: Beta](https://img.shields.io/badge/Status-Beta-orange.svg)](https://docs.betweenrows.dev/about/license)
[![License: ELv2](https://img.shields.io/badge/License-ELv2-blue.svg)](LICENSE)
[![Docker](https://img.shields.io/badge/Docker-GHCR-blue?logo=docker)](https://ghcr.io/getbetweenrows/betweenrows)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org)

A fully customizable data access governance layer.

BetweenRows is a SQL-aware proxy that enforces fine-grained access policies — masking, filtering, and blocking — in real-time. Works with PostgreSQL today; warehouses and lakehouses on the roadmap. Free to self-host. Source-available.

📖 **Full documentation:** [docs.betweenrows.dev](https://docs.betweenrows.dev)

## ✨ Why BetweenRows

**Enforcement & audit**
- SQL-aware — parses every query, then masks columns, filters rows, or blocks operations at the level where data actually moves.
- Every query and every policy decision is logged end-to-end — who ran what, when, and what BetweenRows did about it.

**Fully customizable**
- Composable roles, custom attributes, and JavaScript decision functions — all first-class variables in policy expressions that evaluate at query time.
- RBAC, ABAC, and programmable logic in one unified engine.

**Free and source-available**
- Self-host with no usage limits and no seat fees.
- Inspect the code. Run it on your infrastructure. No black-box security.

Built in **Rust** on **DataFusion** — low overhead, memory-safe, production-grade query rewriting.

## 📸 Screenshots

| | |
|---|---|
| ![Admin dashboard](docs-site/docs/public/screenshots/introduction-dashboard-v0.14.png) | The admin dashboard — data sources, users, roles, and policies at a glance. |
| ![Row filter policy editor](docs-site/docs/public/screenshots/row-filters-policy-editor-v0.15.png) | Row filter policy editor — write a `WHERE` expression with template variables like `{user.tenant}`. |
| ![Query audit detail](docs-site/docs/public/screenshots/row-filters-audit-v0.15.png) | Query audit — see the original SQL, the rewritten SQL, and which policies fired. |

## 🏗️ Components

BetweenRows ships as a single binary with two planes:

**Data plane** (port 5434) — PostgreSQL wire protocol proxy. Connect with any PostgreSQL client (`psql`, TablePlus, DBeaver, your app). Policies are enforced transparently on every query.

**Management plane** (port 5435) — Admin UI and REST API for managing users, data sources, roles, policies, and audit logs. Only admin users have access.

The two planes are independent — being an admin does **not** grant data access. All data access must be explicitly granted via data source assignments and policies.

```
psql / app
    ↓  PostgreSQL wire protocol (port 5434)
BetweenRows
    ├─ Authenticates user
    ├─ Checks data source access
    ├─ Applies policies:
    │      row_filter   — inject WHERE clauses
    │      column_mask  — replace column values
    │      column_deny  — hide columns
    │      table_deny   — hide tables
    │      column_allow — allowlist columns
    └─ Executes via DataFusion
    ↓
Upstream PostgreSQL
```

## 🚀 Quick Start (Docker)

```bash
docker run -d \
  -e BR_ADMIN_USER=admin \
  -e BR_ADMIN_PASSWORD=changeme \
  -p 5434:5434 -p 5435:5435 \
  -v betweenrows_data:/data \
  ghcr.io/getbetweenrows/betweenrows:latest  # demo only — pin a specific tag for anything real
```

| Variable                    | Required | Default | Description                                                                                                                                                                                                                                                                            |
| --------------------------- | -------- | ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `BR_ADMIN_USER`             | No       | `admin` | Username for the initial admin account. Change it now if you prefer a different name — the username cannot be changed after creation. You can always create additional admin users through the UI later.                                                                               |
| `BR_ADMIN_PASSWORD`         | **Yes**  | —       | Password for the initial admin account. Only used on first boot. You can change the password later through the UI.                                                                                                                                                                     |
| `-p 5434:5434`              | **Yes**  | —       | SQL proxy port. Connect your SQL clients here.                                                                                                                                                                                                                                         |
| `-p 5435:5435`              | **Yes**  | —       | Admin UI and REST API port.                                                                                                                                                                                                                                                            |
| `-v betweenrows_data:/data` | **Yes**  | —       | Persistent volume. Stores the SQLite database (users, data sources, policies, audit logs) and auto-generated encryption/JWT keys when `BR_ENCRYPTION_KEY` and `BR_ADMIN_JWT_SECRET` are not set. **Do not omit** — without it, all data and keys are lost when the container restarts. |

Change these values to your preference **before the first run**. See [Configuration](#configuration) for all available options.

Open **http://localhost:5435** and log in with your admin credentials.

### 5-Minute Walkthrough

1. 🔗 **Add a data source** — Go to Data Sources → Create. Enter your data source connection details and test the connection.
2. 🔍 **Discover the schema** — Click "Discover Catalog" on your new data source. Select which schemas, tables, and columns to expose through the proxy.
3. 👤 **Create a user** — Go to Users → Create. Set a username and password.
4. 🔑 **Grant access** — On the data source page, assign the user (or a role) access to the data source.
5. 📜 **Create a policy** — Go to Policies → Create. For example, a `row_filter` policy with expression `tenant = {user.tenant}` to isolate rows by tenant. (The `tenant` attribute must be defined as a custom attribute definition first.)
6. 🎯 **Assign the policy** — On the data source page, assign the policy to a user, role, or all users.
7. 🚀 **Connect through the proxy** — The user can now query through BetweenRows:
   ```bash
   psql "postgresql://alice:secret@localhost:5434/my-datasource"
   ```
   Policies are applied automatically. Check the Query Audit page to see what happened.

## ⚙️ Configuration

| Env var                     | Required             | Default                            | Description                                                                                                                                                                                                                                                                                                                |
| --------------------------- | -------------------- | ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `BR_ADMIN_PASSWORD`         | **Yes** (first boot) | —                                  | Password for the initial admin account. Must be set when no users exist in DB.                                                                                                                                                                                                                                             |
| `BR_ADMIN_USER`             | No                   | `admin`                            | Username for the initial admin account. Only used on first boot.                                                                                                                                                                                                                                                           |
| `BR_ENCRYPTION_KEY`         | No                   | _(auto-persisted)_                 | 64-char hex — AES-256-GCM key for secrets at rest. If unset, auto-generated and saved to `/data/.betweenrows/encryption_key`. **Set explicitly in prod.** If switching from auto-generated to explicit, copy the value from `/data/.betweenrows/encryption_key` — using a different key makes existing secrets unreadable. |
| `BR_ADMIN_JWT_SECRET`       | No                   | _(auto-persisted)_                 | Any non-empty string — HMAC-SHA256 signing key for admin JWTs. If unset, auto-generated and saved to `/data/.betweenrows/jwt_secret`. **Set explicitly in prod.**                                                                                                                                                          |
| `BR_ADMIN_JWT_EXPIRY_HOURS` | No                   | `24`                               | JWT lifetime in hours.                                                                                                                                                                                                                                                                                                     |
| `BR_ADMIN_DATABASE_URL`     | No                   | `sqlite://proxy_admin.db?mode=rwc` | SeaORM connection URL (use `postgres://…` for shared backend).                                                                                                                                                                                                                                                             |
| `BR_PROXY_BIND_ADDR`        | No                   | `127.0.0.1:5434`                   | Proxy listen address. Docker image defaults to `0.0.0.0:5434`.                                                                                                                                                                                                                                                             |
| `BR_ADMIN_BIND_ADDR`        | No                   | `127.0.0.1:5435`                   | Admin REST API listen address. Docker image defaults to `0.0.0.0:5435`.                                                                                                                                                                                                                                                    |
| `BR_IDLE_TIMEOUT_SECS`      | No                   | `900` (15 min)                     | Close idle proxy connections after this many seconds. Set to `0` to disable.                                                                                                                                                                                                                                               |
| `BR_CORS_ALLOWED_ORIGINS`   | No                   | _(empty, same-origin only)_        | Comma-separated list of allowed CORS origins for the Admin API.                                                                                                                                                                                                                                                            |
| `RUST_LOG`                  | No                   | `info`                             | Log filter (standard Rust/tracing convention).                                                                                                                                                                                                                                                                             |

## 🔌 Connecting to the Proxy

BetweenRows speaks the PostgreSQL wire protocol — connect with any PostgreSQL client using the datasource name as the database:

```bash
psql "postgresql://<user>:<password>@127.0.0.1:5434/<datasource-name>"
```

Tested with **psql** and **TablePlus**. Any tool that supports PostgreSQL or ODBC with a PostgreSQL driver should work — including DBeaver, DataGrip, BI tools, and application ORMs.

> **Note:** Some SQL clients send additional metadata queries (e.g., for autocompletion or schema browsing) that BetweenRows may not support yet. If your client fails to connect, please [open an issue](https://github.com/getbetweenrows/betweenrows/issues).

## 🛡️ Policy System

BetweenRows supports five policy types:

| Type           | What it does                                                                              |
| -------------- | ----------------------------------------------------------------------------------------- |
| `row_filter`   | Injects a WHERE clause to filter rows (e.g., `tenant = {user.tenant}`)                    |
| `column_mask`  | Replaces column values with an expression (e.g., `'***@' \|\| split_part(email, '@', 2)`) |
| `column_allow` | Permits access to specific columns (required in `policy_required` mode)                   |
| `column_deny`  | Hides columns from the user's schema entirely                                             |
| `table_deny`   | Hides entire tables from the user's schema                                                |

Key concepts:

- **RBAC** — assign policies to roles. Users inherit policies through role membership, including via role hierarchies
- **ABAC** — define custom user attributes (e.g., department, region, clearance level) and use them in policy expressions via `{user.<key>}` template variables
- **Decision functions** — optional JavaScript functions compiled to WASM that gate policy evaluation based on arbitrary logic (time windows, multi-attribute conditions, external state)
- **Assignment scopes** — policies can be assigned to individual users, roles, or all users on a data source
- **Template variables** — `{user.username}`, `{user.id}` (built-in), and custom attributes like `{user.tenant}`, `{user.region}` are substituted at query time, making policies dynamic per user
- **Access modes** — data sources can be set to `open` (all tables accessible by default) or `policy_required` (tables are hidden unless a `column_allow` policy grants access)
- **Deny wins** — deny policies are evaluated before permit policies and cannot be overridden
- **Visibility follows access** — denied columns and tables are removed from the user's schema at connection time, so they don't appear in client tools like TablePlus or DBeaver

See [docs-site/docs/concepts/policy-model.md](docs-site/docs/concepts/policy-model.md) for the full guide.

## 📋 Catalog Workflow

Before a data source is queryable through the proxy, its catalog must be saved. The UI wizard guides through four steps:

1. **Discover schemas** — select which schemas to include
2. **Discover tables** — select tables within those schemas
3. **Discover columns** — choose which columns to expose
4. **Save** — persists selections and makes them available for queries

The catalog is an **allowlist** — the proxy can never expose tables or columns not explicitly saved. To detect schema drift after upstream changes, use "Sync Catalog" from the data source page.

## 🔗 Admin REST API

BetweenRows includes a full REST API at `http://localhost:5435/api/v1` for managing users, data sources, roles, policies, catalog discovery, and audit logs. All endpoints require JWT authentication (`POST /auth/login` to obtain a token).

Everything you can do in the admin UI can also be done via the API — useful for scripting, CI/CD integration, and automation.

## 💻 CLI

Create users without the UI — useful for scripting and automation. If you're locked out of the admin UI, use `--admin` to create a new admin user to regain access. You can then change passwords through the UI. A forgot/reset password feature is on the [roadmap](docs-site/docs/about/roadmap.md):

```bash
# Docker
docker exec -it <container> proxy user create --username alice --password secret
docker exec -it <container> proxy user create --username rescue --password secret --admin

# From source
cargo run -p proxy -- user create --username alice --password secret
```

## 🗺️ Roadmap

See [docs-site/docs/about/roadmap.md](docs-site/docs/about/roadmap.md) for planned features including shadow mode, governance workflows, and more.

## 🤝 Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for architecture details, build instructions, and development setup.
