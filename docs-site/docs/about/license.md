---
title: License & Beta Status
description: BetweenRows is licensed under ELv2 and is currently in beta. This page summarizes the license terms, what beta means in practice, and the recommended posture for early adopters.
---

# License & Beta Status

## License: Elastic License v2 (ELv2)

BetweenRows is licensed under the [Elastic License v2 (ELv2)](https://github.com/getbetweenrows/betweenrows/blob/main/LICENSE). This section is a plain-language summary, not a substitute for reading the license itself.

### What ELv2 allows

- **Use BetweenRows freely** — in production, in development, for internal business use, for personal projects, for commercial products you build.
- **Modify the source code** for your own use.
- **Redistribute BetweenRows or modified versions**, as long as you comply with the conditions below.
- **Host BetweenRows for your own users** — run it as part of a product you offer, use it in a SaaS you operate, deploy it inside your company.

### What ELv2 prohibits

**You may not offer BetweenRows as a managed service** — specifically, you can't provide the functionality of BetweenRows to others as a hosted or managed service where they pay you for access to BetweenRows itself. You can *use* BetweenRows inside a product you sell, but you can't turn around and sell "BetweenRows as a service" to compete with it.

### What this means in practice

**Most users won't notice ELv2.** If you're running BetweenRows for your own company, building a product that uses it internally, self-hosting for your team, or experimenting — the license imposes no restrictions you'd feel. The restriction only kicks in if you want to sell BetweenRows itself as a hosted service to third parties (e.g. "Managed BetweenRows for \$X/month"). That's what ELv2 exists to prevent.

### Read the actual license

This is a summary. The [LICENSE file](https://github.com/getbetweenrows/betweenrows/blob/main/LICENSE) in the repository is the authoritative text. When in doubt, read it. For commercial questions or edge cases, reach out via GitHub Issues.

## Beta status

::: warning
**BetweenRows is pre-1.0 beta software.** Read this section before deploying in any environment where stability and backward compatibility matter.
:::

### What pre-1.0 means in practice

- **APIs, configuration, and data formats may change between releases.** A `PUT` endpoint that exists in 0.16.x may be renamed, restructured, or removed in 0.17.x. Environment variables may change. Database schemas may migrate in ways that require a backup-before-upgrade discipline.
- **There is no backward compatibility guarantee** between minor versions until 1.0. The CHANGELOG calls out breaking changes — read it before upgrading.
- **Pin your Docker image tag** to a specific version. Never use `:latest` in production.
- **The software is provided as-is with no warranty.** Use at your own risk.
- **Early feedback has an outsized effect on what we prioritize** — file issues on [GitHub](https://github.com/getbetweenrows/betweenrows/issues).

### What is stable

- **The policy enforcement model** — five types, deny-wins, scan-level rewriting. These decisions are load-bearing and well-tested.
- **The PostgreSQL wire protocol compatibility** — driven by `pgwire` and DataFusion; behavior is reasonably stable.
- **The admin REST API shape** — likely to see small additive changes (new fields, new endpoints) but not large rewrites.

### What is less stable

- **Decision function APIs and compilation path** — Javy versioning, WASM harness.
- **Catalog discovery internals** — the way schemas/tables/columns are represented may be refactored.
- **CLI subcommands** — new ones will be added; existing ones are short-lived for now.
- **Specific endpoint URL conventions** — some may be relocated under more consistent prefixes.

### Recommended posture for early adopters

1. **Start in a non-critical environment.** Development, staging, or a lower-traffic data source in production.
2. **Back up before every upgrade.** See [Backups](/operations/backups).
3. **Read the changelog before upgrading.** See [Upgrading](/operations/upgrading).

## See also

- **[Upgrading](/operations/upgrading)** — practical upgrade hygiene
- **[Known limitations](/operations/known-limitations)** — things to be aware of before production
- **[Security Overview](/concepts/security-overview)** — for security reviewers
- **[Roadmap](/about/roadmap)** — where the project is heading
