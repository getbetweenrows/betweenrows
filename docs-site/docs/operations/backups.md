---
title: Backups
description: What lives in /data, how to snapshot it safely, and how to export policies independently as belt-and-suspenders recovery.
---

# Backups

BetweenRows stores its entire admin state in the `/data` volume. Losing `/data` without a backup means losing users, policies, data source credentials, audit logs, and the encryption keys needed to read the credentials. **Back it up.**

## What's in `/data`

| Path | Contents | Recovery impact if lost |
|---|---|---|
| `/data/proxy_admin.db` | SQLite admin database — users, roles, policies, data sources, audit logs, attribute definitions, decision functions | All admin state; recreating from scratch takes hours for a non-trivial deployment |
| `/data/.betweenrows/encryption_key` | Auto-generated AES-256-GCM key (if `BR_ENCRYPTION_KEY` was not set explicitly) | Data source passwords and decision function JS source become unreadable |
| `/data/.betweenrows/jwt_secret` | Auto-generated HMAC signing key (if `BR_ADMIN_JWT_SECRET` was not set explicitly) | All admin sessions invalidated (low impact — admins re-authenticate) |

::: warning
If you set `BR_ENCRYPTION_KEY` and `BR_ADMIN_JWT_SECRET` explicitly via environment variables (recommended for production), the `/data/.betweenrows/` files are not created, but the admin database still needs to be backed up.
:::

## Audit log growth

The admin database holds two audit tables that grow over time and have no built-in retention:

- **`query_audit_log`** — one row per evaluated query. Grows with proxy traffic.
- **`admin_audit_log`** — one row per management-plane mutation (policy edits, role changes, user CRUD). Grows with admin activity, typically at a much lower rate.

Neither table is pruned automatically. On a busy deployment `query_audit_log` is usually what pushes `/data` size. You are responsible for sizing the volume and pruning or archiving old rows on whatever schedule your compliance story requires.

### Pruning old rows

Run a scheduled job against the SQLite admin database to delete rows past your retention window. The proxy holds a write connection, so either stop the container briefly or rely on SQLite's busy handler:

```sh
# 90-day retention, run nightly via cron
sqlite3 /data/proxy_admin.db <<'SQL'
PRAGMA busy_timeout = 30000;
DELETE FROM query_audit_log WHERE created_at < datetime('now', '-90 days');
DELETE FROM admin_audit_log WHERE created_at < datetime('now', '-90 days');
SQL
```

SQLite reuses freed pages automatically; the file does not need to be compacted after each prune.

### Archive before prune

If your compliance posture requires retaining audit history, export the rows you are about to delete first. A simple `sqlite3` export piped to gzip is enough:

```sh
sqlite3 /data/proxy_admin.db \
  "SELECT * FROM query_audit_log WHERE created_at < datetime('now', '-90 days');" \
  | gzip > /var/archive/query_audit_log-$(date +%F).csv.gz
```

Store the archive wherever you keep long-term audit records (S3, Glacier, your SIEM). Treat it as sensitive — audit rows contain query text that may reveal schema and user behavior.

::: info Planned
Operator-configurable audit log retention (TTL and/or row cap, with optional export-before-prune) is on the [roadmap](/about/roadmap). Until it ships, use the scripts above.
:::

## Backup options

### Option 1: Volume snapshot (recommended)

If you run in a cloud environment with volume snapshots, use them. They're atomic, fast, and include everything in `/data` in one consistent view.

- **AWS EBS:** `aws ec2 create-snapshot --volume-id vol-... --description "betweenrows-YYYY-MM-DD"`
- **GCP Persistent Disk:** `gcloud compute disks snapshot <disk-name> --snapshot-names=betweenrows-YYYY-MM-DD`
- **Fly volumes:** Fly Volumes have automatic daily snapshots with 5-day retention. Manual snapshot: `fly volumes snapshots create <volume-id>`.
- **Docker named volumes (local):** no built-in snapshot; use one of the other options below.

Schedule snapshots via cron or a scheduled job. Retain enough to recover from a failed upgrade — a week is a reasonable baseline.

### Option 2: SQLite online backup

SQLite supports an atomic online backup even while the database is in use. Use the `.backup` pragma or the `sqlite3` CLI:

```sh
# From inside the container
sqlite3 /data/proxy_admin.db ".backup /data/backups/proxy_admin-$(date +%F).db"

# Copy out to the host
docker cp betweenrows:/data/backups/proxy_admin-$(date +%F).db ./backups/
```

Or use the SQLite `BACKUP TO` API from a small script. This produces a point-in-time consistent copy without stopping the proxy.

Don't forget to also copy the encryption key and JWT secret if they're auto-generated:

```sh
docker cp betweenrows:/data/.betweenrows/encryption_key ./backups/
docker cp betweenrows:/data/.betweenrows/jwt_secret ./backups/
```

::: info Planned: YAML export/import
Human-readable YAML export/import of policies is on the roadmap as part of the declarative "policy as code" workflow for the planned `betweenrows` CLI. Until it ships, the primary backup path is the volume snapshot (above) and the admin database dump. Do not rely on a separate policy backup artifact yet.
:::

## Backup checklist

1. **Pick a schedule.** Daily snapshots for production; weekly for staging. Retention at least 2 weeks for daily, 2 months for weekly.
2. **Automate it.** Cron on the host, scheduled CI job, or cloud-provider-managed snapshots. Manual backups get forgotten.
3. **Test a restore.** At least once per quarter, restore a backup to a scratch environment and verify you can log in, see policies, and query a data source. Backups that have never been restored are not backups.
4. **Secure the backup location.** Backups contain encrypted data source passwords. If an attacker gets the backup *and* the `BR_ENCRYPTION_KEY`, they can decrypt them. Treat backups as sensitive data — encrypt at rest, restrict access.
5. **Document the recovery procedure.** Write down the exact steps to restore, including where the backup lives, how to decrypt it, and how to point a fresh container at the restored volume. Put it in your runbook.


## Recovery procedure

### From a volume snapshot

1. Stop the current container: `docker stop betweenrows && docker rm betweenrows`.
2. Mount the snapshot as a new volume (cloud-provider-specific).
3. Start a new container with the same environment variables, mounting the restored volume at `/data`.
4. Verify login and query.

### From a SQLite `.backup` dump

1. Stop the container.
2. Replace `/data/proxy_admin.db` with the backup file.
3. Replace `/data/.betweenrows/encryption_key` and `jwt_secret` with the backup files (if auto-generated).
4. Start the container.
5. Verify login and query.

## See also

- **[Upgrading](/operations/upgrading)** — the other time you really need a backup
- **[Configuration](/reference/configuration)** — `BR_ENCRYPTION_KEY`, `BR_ADMIN_DATABASE_URL`
- **[Admin REST API](/reference/admin-rest-api)** — admin plane reference
