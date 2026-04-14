---
title: Upgrading
description: Safely upgrade BetweenRows between versions — pin the tag, back up /data, read the changelog, swap the image, verify.
---

# Upgrading

Upgrades between minor versions can include database migrations, configuration changes, or API adjustments. Always upgrade deliberately, not automatically — pin your Docker image tag to a specific version (e.g. `{{VERSION}}`) so that a container restart never crosses a release boundary on its own.

## Upgrade checklist

1. **Read the changelog** between your current version and the target version. [CHANGELOG.md](https://github.com/getbetweenrows/betweenrows/blob/main/CHANGELOG.md) on GitHub lists every release. Pay special attention to:
   - **Breaking changes** — API shape changes, renamed fields, removed endpoints
   - **Migration requirements** — new database columns, backfills, long-running schema changes
   - **Configuration changes** — new required env vars, renamed or deprecated ones

2. **Back up `/data`.** See the [Backups](/operations/backups) page. Don't skip this even for minor version bumps — migrations are forward-only, and recovering from a failed migration may require a snapshot.

3. **Have a rollback plan.** If the upgrade fails or reveals a regression:
   - With a volume snapshot, you can restore `/data` and roll back to the previous image tag.
   - Without a snapshot, rollback means restoring from application-level backups (export policies as YAML before upgrading, re-import after rollback).

4. **Test in a staging environment first** if you have one. The staging data source can point at a clone of your production upstream.

5. **Pull the new image.**

   ```sh
   docker pull ghcr.io/getbetweenrows/betweenrows:{{VERSION}}
   ```

6. **Stop the old container, start the new one** with the same env vars and volume mount.

   ```sh
   docker stop betweenrows
   docker rm betweenrows
   docker run -d \
     --name betweenrows \
     --restart unless-stopped \
     -e BR_ADMIN_PASSWORD="$BR_ADMIN_PASSWORD" \
     -e BR_ENCRYPTION_KEY="$BR_ENCRYPTION_KEY" \
     -e BR_ADMIN_JWT_SECRET="$BR_ADMIN_JWT_SECRET" \
     -p 5434:5434 -p 5435:5435 \
     -v /srv/betweenrows/data:/data \
     ghcr.io/getbetweenrows/betweenrows:{{VERSION}}
   ```

   Or with Docker Compose: change the `image:` tag in `compose.yaml` and run `docker compose up -d`.

7. **Watch the logs** for migration output.

   ```sh
   docker logs -f betweenrows
   ```

   Migrations run automatically on startup. You should see messages indicating each migration applied and the final "ready" or "listening" log line. A crash during migration is a serious event — stop, investigate, and restore from the backup if necessary.

8. **Verify the admin UI loads** at port 5435 and you can log in.

9. **Verify the data plane** by connecting with psql as an existing user and running a query you know should work. Check the Query Audit page to confirm the query was processed normally.

10. **Spot-check a policy.** Pick a non-trivial policy and run a test query that should be filtered/masked. Confirm the rewritten SQL in the audit log matches what you expect.


## Fly.io upgrade

On Fly, the same pattern but via `flyctl`:

```sh
# Read the changelog first, then back up the volume:
fly ssh console --app <your-app-name> -C "tar -czf /tmp/data.tgz /data"
fly ssh sftp get /tmp/data.tgz ./data-backup-$(date +%F).tgz --app <your-app-name>

# Deploy the new image
fly deploy --image ghcr.io/getbetweenrows/betweenrows:{{VERSION}} --app <your-app-name>

# Tail logs during rollout
fly logs --app <your-app-name>
```

See [Install on Fly.io](/installation/fly) for the full deployment reference.

## Migration safety

BetweenRows uses [SeaORM migrations](https://www.sea-ql.org/SeaORM/docs/migration/setting-up-migration/) that run automatically on startup. The migration framework tracks which migrations have been applied in a `seaql_migrations` table inside the admin database.

**Migration conventions in this project** (from the repository's root CLAUDE.md):

- **One DDL statement per file.** No multi-step migrations that could leave the database in a partial state.
- **`IF NOT EXISTS`** on every `CREATE TABLE` and `CREATE INDEX` — so migrations are safe to retry if a previous run applied the DDL but crashed before recording.
- **No renames or deletions of applied migration files.** Renaming an applied migration causes a fatal startup error.

These rules mean migrations are generally safe to retry, but **do not manually edit the `seaql_migrations` table** or the admin database files. If a migration fails and you cannot recover cleanly, restore from your backup and file a GitHub issue with the full logs.

## Downgrading

**Downgrades are not supported.** SeaORM migrations are forward-only. If you need to roll back:

1. Stop the new container.
2. Restore `/data` from the snapshot taken before the upgrade.
3. Start the old image version.

If you've made policy changes between the upgrade and the rollback, capture them first by snapshotting the admin database (see [Backups](/operations/backups)) — there is no YAML export API yet.

## Version pinning in CI/CD

If you deploy BetweenRows via an IaC or GitOps workflow, pin the tag in source control:

```yaml
# compose.yaml
services:
  betweenrows:
    image: ghcr.io/getbetweenrows/betweenrows:{{VERSION}}   # not :latest
```

```hcl
# terraform
resource "fly_app" "betweenrows" {
  image = "ghcr.io/getbetweenrows/betweenrows:{{VERSION}}"  # not :latest
}
```

Treat version bumps as deliberate PRs — with changelog review in the PR description and a small smoke-test playbook in the pipeline.

## See also

- **[Changelog](/about/changelog)** — version history and breaking changes
- **[Backups](/operations/backups)** — what to snapshot before upgrading
- **[Troubleshooting](/operations/troubleshooting)** — if something goes wrong
- **[License & Beta Status](/about/license)** — pre-1.0 stability posture
