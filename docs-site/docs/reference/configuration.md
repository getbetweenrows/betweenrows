---
title: Configuration
description: Every BetweenRows environment variable, default, and note. Used by the Docker image, Fly.io deployments, and source builds alike.
---

# Configuration

BetweenRows is configured entirely via environment variables. There is no config file.

## Required on first boot

| Variable | Default | Description |
|---|---|---|
| `BR_ADMIN_PASSWORD` | — | Password for the initial admin account. **Required** when no users exist in the admin database. Only used on first boot. Change it through the admin UI after logging in. |

## Admin account

| Variable | Default | Description |
|---|---|---|
| `BR_ADMIN_USER` | `admin` | Username for the initial admin account. Only used on first boot — ignored on subsequent boots. The username cannot be changed after creation; pick the name you want before the first run. You can always create additional admin accounts later via the UI or CLI. |

## Secrets and signing

| Variable | Default | Description |
|---|---|---|
| `BR_ENCRYPTION_KEY` | auto-persisted | **64-char hex string.** AES-256-GCM key used to encrypt sensitive admin data at rest (data source passwords, decision function JS source). If unset, auto-generated on first boot and persisted to `/data/.betweenrows/encryption_key`. **Set explicitly in production.** If you rotate this value, existing encrypted data becomes unreadable — migrate carefully. |
| `BR_ADMIN_JWT_SECRET` | auto-persisted | HMAC-SHA256 signing secret for admin JWTs. Any non-empty string. Auto-generated and persisted to `/data/.betweenrows/jwt_secret` if unset. **Set explicitly in production.** Rotating this value invalidates all existing admin sessions — admins must re-authenticate. |
| `BR_ADMIN_JWT_EXPIRY_HOURS` | `24` | JWT lifetime in hours. After this duration, admins must re-authenticate. |

## Admin database

| Variable | Default | Description |
|---|---|---|
| `BR_ADMIN_DATABASE_URL` | `sqlite://proxy_admin.db?mode=rwc` | SeaORM connection URL for the admin database. SQLite is the supported and tested backend. The file lives under `/data` in the Docker image. |

## Network bindings

| Variable | Default (binary) | Default (Docker) | Description |
|---|---|---|---|
| `BR_PROXY_BIND_ADDR` | `127.0.0.1:5434` | `0.0.0.0:5434` | The address the SQL proxy listens on. Docker image defaults to `0.0.0.0` so the port is reachable from outside the container. |
| `BR_ADMIN_BIND_ADDR` | `127.0.0.1:5435` | `0.0.0.0:5435` | The address the admin REST API and UI listens on. Same Docker override. |

## Connection lifecycle

| Variable | Default | Description |
|---|---|---|
| `BR_IDLE_TIMEOUT_SECS` | `900` (15 min) | Close idle proxy connections after this many seconds with no activity. Prevents slow or abandoned clients from holding connections indefinitely. Set to `0` to disable (not recommended — risks connection exhaustion under load). |

## CORS

| Variable | Default | Description |
|---|---|---|
| `BR_CORS_ALLOWED_ORIGINS` | _(empty — same-origin only)_ | Comma-separated list of allowed CORS origins for the admin REST API. Required if you host the admin UI on a different origin than the REST API. Example: `https://admin.example.com,https://staging-admin.example.com`. |

## Logging

| Variable | Default | Description |
|---|---|---|
| `RUST_LOG` | `info` | Standard Rust tracing filter. Examples: `debug`, `info,hyper=warn`, `proxy=debug,info`. Use `debug` when investigating an issue, `info` for normal operation. |

## Example: minimum production configuration

```sh
docker run -d \
  --name betweenrows \
  --restart unless-stopped \
  -e BR_ADMIN_PASSWORD="$(openssl rand -base64 24)" \
  -e BR_ENCRYPTION_KEY="$(openssl rand -hex 32)" \
  -e BR_ADMIN_JWT_SECRET="$(openssl rand -base64 32)" \
  -e BR_ADMIN_JWT_EXPIRY_HOURS=8 \
  -e BR_IDLE_TIMEOUT_SECS=600 \
  -e RUST_LOG=info \
  -p 5434:5434 -p 5435:5435 \
  -v /srv/betweenrows/data:/data \
  ghcr.io/getbetweenrows/betweenrows:0.15.0
```

::: tip
Save `BR_ENCRYPTION_KEY` and `BR_ADMIN_JWT_SECRET` in a secrets manager (Vault, AWS Secrets Manager, Fly secrets, Kubernetes secrets). Losing them means losing encrypted data source credentials and invalidating all admin sessions.
:::

## Related pages

- **[Install with Docker](/installation/docker)** — the typical deployment path
- **[Install on Fly.io](/installation/fly)** — for hosted deployments
- **[Backups](/operations/backups)** — what to snapshot
- **[Security Overview](/concepts/security-overview)** — the production checklist
