---
title: Install on Fly.io
description: Deploy BetweenRows to Fly.io with a persistent volume, explicit secrets, and IPv6/IPv4 client connectivity.
---

# Install on Fly.io

Fly.io is a convenient hosted deployment path for BetweenRows. The steps below assume you have `flyctl` installed and authenticated.

## 1. Create the app

```sh
fly launch --no-deploy --copy-config --name <your-app-name>
```

This creates the app and a 1 GB persistent volume (configured in `fly.toml`).

## 2. Set your admin password and deploy

```sh
fly secrets set BR_ADMIN_PASSWORD=<strong-password> --app <your-app-name>
fly deploy --app <your-app-name>
```

Encryption and JWT keys are auto-generated and persisted to the volume — no additional secrets needed to get started.

**For production**, set `BR_ENCRYPTION_KEY` and `BR_ADMIN_JWT_SECRET` explicitly so they can be rotated independently of the volume:

```sh
fly secrets set \
  BR_ENCRYPTION_KEY=$(openssl rand -hex 32) \
  BR_ADMIN_JWT_SECRET=$(openssl rand -base64 32) \
  --app <your-app-name>
```

`BR_ENCRYPTION_KEY` must be a 64-character hex string (AES-256-GCM). `BR_ADMIN_JWT_SECRET` accepts any non-empty string.

See the [Configuration reference](/reference/configuration) for all available options.

## 3. Open the admin UI

| Endpoint | URL / address |
|---|---|
| Admin UI | `https://<app-name>.fly.dev` |
| Admin REST API | `https://<app-name>.fly.dev/api/v1/...` |
| PostgreSQL proxy | `<app-name>.fly.dev:5432` |

Log in as `admin` with your password.

## Connecting via pgwire

The pgwire port is accessible for free via **IPv6** (most modern clients resolve it automatically):

```sh
psql "postgresql://admin:<password>@<app-name>.fly.dev:5432/<datasource-name>"
```

### macOS: if the connection times out

Check whether IPv6 is configured on your machine:

```sh
ifconfig | grep "inet6" | grep -v "::1" | grep -v "fe80"
```

If that returns nothing, your machine has no routable IPv6 address. Re-enable it:

```sh
sudo networksetup -setv6automatic Wi-Fi
```

Then confirm with `ping6 google.com` and retry the connection.

### IPv4-only environments

If your network has no IPv6 support, tunnel via WireGuard:

```sh
fly proxy 5432:5434 --app <app-name>
psql "postgresql://admin:<password>@127.0.0.1:5432/<datasource-name>"
```

Or allocate a dedicated IPv4 ($2/mo):

```sh
fly ips allocate-v4 --app <app-name>
```

## Upgrading

Pull a specific image tag and redeploy:

```sh
fly deploy --image ghcr.io/getbetweenrows/betweenrows:{{VERSION}} --app <your-app-name>
```

Or if your `fly.toml` already references a specific image tag, just:

```sh
fly deploy --app <your-app-name>
```

::: tip
Always pin `fly.toml` to a specific version tag rather than `:latest`. That way a `fly deploy` only upgrades the image when you change the tag deliberately.
:::

See [Upgrading](/operations/upgrading) for general upgrade guidance (backup the volume first, read the changelog between versions).

## Next steps

- **[Troubleshooting](/operations/troubleshooting)** — connection and client compatibility issues
- **[Backups](/operations/backups)** — how to snapshot the Fly volume
- **[Configuration reference](/reference/configuration)** — all environment variables
