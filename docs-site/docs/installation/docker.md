---
title: Install with Docker
description: Run BetweenRows as a Docker container on a single host. Covers the minimum invocation, the full environment variable reference, and volume setup.
---

# Install with Docker

BetweenRows ships as a single Docker image at `ghcr.io/getbetweenrows/betweenrows`. The image contains both the data plane (pgwire proxy on port 5434) and the management plane (admin UI and REST API on port 5435) in one binary.

## Minimum invocation

```sh
docker run -d \
  -e BR_ADMIN_USER=admin \
  -e BR_ADMIN_PASSWORD=changeme \
  -p 5434:5434 -p 5435:5435 \
  -v betweenrows_data:/data \
  ghcr.io/getbetweenrows/betweenrows:{{VERSION}}
```

::: tip
**Always pin the image tag** to a specific version like `:{{VERSION}}`. `:latest` will move you across release boundaries on every container restart — upgrade deliberately instead. See [Upgrading](/operations/upgrading) for how to change versions safely.
:::

Once the container is up, open `http://localhost:5435` and log in with your admin credentials:

![BetweenRows admin login screen served from the Docker container on port 5435](/screenshots/docker-login-v0.14.png)

## What the flags do

| Flag | Purpose |
|---|---|
| `-e BR_ADMIN_USER=admin` | Username for the initial admin account. Only used on first boot; ignored on subsequent boots. Change it before the first run if you prefer a different name — the username cannot be changed after creation. |
| `-e BR_ADMIN_PASSWORD=changeme` | Password for the initial admin account. **Required on first boot.** Change it before running in any shared environment. You can change it later through the admin UI. |
| `-p 5434:5434` | SQL proxy port. Connect your PostgreSQL clients here. |
| `-p 5435:5435` | Admin UI and REST API port. |
| `-v betweenrows_data:/data` | **Persistent volume.** Stores the SQLite admin database, auto-generated encryption key, and JWT secret. **Do not omit** — without it, all data and keys are lost when the container restarts. |

## Production-grade invocation

For anything beyond a local demo, set the encryption key and JWT secret explicitly so they survive volume reset and can be rotated independently:

```sh
docker run -d \
  --name betweenrows \
  --restart unless-stopped \
  -e BR_ADMIN_USER=admin \
  -e BR_ADMIN_PASSWORD="$(openssl rand -base64 24)" \
  -e BR_ENCRYPTION_KEY="$(openssl rand -hex 32)" \
  -e BR_ADMIN_JWT_SECRET="$(openssl rand -base64 32)" \
  -e BR_PROXY_BIND_ADDR=0.0.0.0:5434 \
  -e BR_ADMIN_BIND_ADDR=0.0.0.0:5435 \
  -e RUST_LOG=info \
  -p 5434:5434 -p 5435:5435 \
  -v /srv/betweenrows/data:/data \
  ghcr.io/getbetweenrows/betweenrows:{{VERSION}}
```

- `BR_ENCRYPTION_KEY` must be a **64-character hex string** (32 bytes → AES-256-GCM key). If you change this value after secrets have been stored, existing secrets become unreadable.
- `BR_ADMIN_JWT_SECRET` can be any non-empty string, but should be high-entropy. Tokens signed with the old value are rejected after rotation — all admins will need to re-authenticate.
- Save both values somewhere secure *before* the first boot. If you lose them, you will need to wipe `/data` and re-create everything.

See the [Configuration reference](/reference/configuration) for the full list of environment variables.

## Persistent data

The `/data` volume contains:

- `proxy_admin.db` — the SQLite admin database (users, policies, datasources, audit logs, attribute definitions).
- `.betweenrows/encryption_key` — the auto-generated AES-256-GCM key, if `BR_ENCRYPTION_KEY` was not set explicitly.
- `.betweenrows/jwt_secret` — the auto-generated JWT signing secret, if `BR_ADMIN_JWT_SECRET` was not set explicitly.

::: tip
**Back up the whole `/data` directory regularly.** See the [Backups](/operations/backups) page for the recommended approach.
:::

## Verifying the install

1. **Check the container is running.**

   ```sh
   docker ps --filter name=betweenrows
   ```

2. **Tail the logs to confirm startup.**

   ```sh
   docker logs -f betweenrows
   ```

   Look for lines indicating the admin and proxy bind addresses and that migrations completed.

3. **Open the admin UI.**

   Visit [http://localhost:5435](http://localhost:5435) and log in with your admin credentials.

4. **Run the Quickstart walkthrough.**

   Follow the [Quickstart](/start/quickstart) from step 2 onward to add a data source, create a user, define a policy, and verify it with psql.


## Docker Compose

For reproducible local setups, a `compose.yaml` snippet:

```yaml
services:
  betweenrows:
    image: ghcr.io/getbetweenrows/betweenrows:{{VERSION}}
    container_name: betweenrows
    restart: unless-stopped
    ports:
      - "5434:5434"
      - "5435:5435"
    environment:
      BR_ADMIN_USER: admin
      BR_ADMIN_PASSWORD: ${BR_ADMIN_PASSWORD:?required}
      BR_ENCRYPTION_KEY: ${BR_ENCRYPTION_KEY:?required}
      BR_ADMIN_JWT_SECRET: ${BR_ADMIN_JWT_SECRET:?required}
      BR_PROXY_BIND_ADDR: 0.0.0.0:5434
      BR_ADMIN_BIND_ADDR: 0.0.0.0:5435
      RUST_LOG: info
    volumes:
      - betweenrows_data:/data

volumes:
  betweenrows_data:
```

Put the secrets in a `.env` file (not checked into git) and run `docker compose up -d`.

## Behind a reverse proxy

In production, place the admin UI (port 5435) behind a reverse proxy (nginx, Caddy, Cloudflare Tunnel) for TLS, authentication, and rate limiting. The SQL proxy (port 5434) uses the PostgreSQL wire protocol and should be exposed directly or through a TCP load balancer — not an HTTP proxy.

Set `BR_CORS_ALLOWED_ORIGINS` if the admin UI is served from a different origin than the API:

```sh
-e BR_CORS_ALLOWED_ORIGINS=https://admin.example.com
```

::: tip
The admin API requires JWT authentication for all endpoints except `/auth/login`. A reverse proxy adds defense-in-depth: TLS termination, rate limiting on the login endpoint, and IP allowlisting if your admin team is on a known network.
:::

## Upgrading

See [Upgrading](/operations/upgrading) — the short version is *change the image tag and restart, back up `/data` first, read the changelog between your current and target versions before pulling*.

## Next steps

- **[Configuration reference](/reference/configuration)** — all environment variables
- **[Backups](/operations/backups)** — what to snapshot and how
- **[Troubleshooting](/operations/troubleshooting)** — connection failures, policy not matching
- **[Fly.io install](/installation/fly)** — if you want hosted over self-hosted
