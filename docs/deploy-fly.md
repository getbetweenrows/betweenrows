# Deploy to Fly.io

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

Encryption and JWT keys are auto-generated and persisted to the volume — no additional secrets needed to get started. For production, set `BR_ENCRYPTION_KEY` and `BR_ADMIN_JWT_SECRET` explicitly:

```sh
fly secrets set \
  BR_ENCRYPTION_KEY=$(openssl rand -hex 32) \
  BR_ADMIN_JWT_SECRET=$(openssl rand -base64 32) \
  --app <your-app-name>
```

`BR_ENCRYPTION_KEY` must be a 64-char hex string (AES-256-GCM). `BR_ADMIN_JWT_SECRET` accepts any non-empty string.

See the [Configuration](../README.md#configuration) section in the README for all available options.

## 3. Open the admin UI

| Endpoint | URL / address |
|----------|---------------|
| Admin UI | `https://<app-name>.fly.dev` |
| Admin REST API | `https://<app-name>.fly.dev/api/...` |
| PostgreSQL proxy | `<app-name>.fly.dev:5432` |

Log in with `admin` / your password.

## Upgrading

Pull the latest image and redeploy:

```sh
fly deploy --image ghcr.io/getbetweenrows/betweenrows:latest --app <your-app-name>
```

Or if your `fly.toml` already references the image:

```sh
fly deploy
```

## Connecting via pgwire

The pgwire port is accessible for free via **IPv6** (most modern clients resolve it automatically):

```sh
psql "postgresql://admin:<password>@<app-name>.fly.dev:5432/<datasource-name>"
```

**macOS: if the connection times out**, check whether IPv6 is configured:

```sh
ifconfig | grep "inet6" | grep -v "::1" | grep -v "fe80"
```

If that returns nothing, your machine has no routable IPv6 address. Re-enable it:

```sh
sudo networksetup -setv6automatic Wi-Fi
```

Then confirm it's working with `ping6 google.com` and retry the connection.

For **IPv4-only** environments (no IPv6 support), tunnel via WireGuard:

```sh
fly proxy 5432:5434 --app <app-name>
psql "postgresql://admin:<password>@127.0.0.1:5432/<datasource-name>"
```

Or allocate a dedicated IPv4 ($2/mo):

```sh
fly ips allocate-v4 --app <app-name>
```

## CI/CD

The GitHub Actions workflow (`.github/workflows/cicd.yml`) handles automated deploys:

- Push to `main` → tests only (Rust + admin-ui)
- Push `v*` tag → tests → build & publish Docker image to GHCR → deploy to Fly.io
- `workflow_dispatch` → redeploy a specific existing version

For CI/CD deploys to work, the GHCR package must be public: **GitHub → Packages → betweenrows → Package settings → Change visibility → Public**.
