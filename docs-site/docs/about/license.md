---
title: License & Alpha Status
description: BetweenRows is licensed under ELv2. This page summarizes the license terms and the alpha software disclaimer.
---

# License & Alpha Status

## License: Elastic License v2 (ELv2)

BetweenRows is licensed under the [Elastic License v2 (ELv2)](https://github.com/getbetweenrows/betweenrows/blob/main/LICENSE). This section is a plain-language summary, not a substitute for reading the license itself.

### What ELv2 allows

- **Use BetweenRows freely** — in production, in development, for internal business use, for personal projects, for commercial products you build.
- **Modify the source code** for your own use.
- **Redistribute BetweenRows or modified versions**, as long as you comply with the conditions below.
- **Host BetweenRows for your own users** — run it as part of a product you offer, use it in a SaaS you operate, deploy it inside your company.

### What ELv2 prohibits

ELv2 has three specific restrictions you must not violate:

1. **You may not offer BetweenRows as a managed service** — specifically, you can't provide the functionality of BetweenRows to others as a hosted or managed service where they pay you for access to BetweenRows itself. You can *use* BetweenRows inside a product you sell, but you can't turn around and sell "BetweenRows as a service" to compete with it.
2. **You may not circumvent the license key functionality** of the software. (BetweenRows alpha does not currently use license keys, but the restriction still applies if keys are added later.)
3. **You may not remove or obscure** any licensing, copyright, or trademark notices from the software.

### What this means in practice

**Most users should not worry about the license.** If you're:

- Running BetweenRows to protect your own company's data
- Building a product that uses BetweenRows internally
- Self-hosting for your team
- Experimenting, learning, or contributing

…ELv2 imposes no restrictions you'd notice.

**The restriction kicks in if** you want to offer BetweenRows itself as a hosted service to third parties — e.g., "Managed BetweenRows for \$X/month." That's what ELv2 exists to prevent.

### Read the actual license

This is a summary. The [LICENSE file](https://github.com/getbetweenrows/betweenrows/blob/main/LICENSE) in the repository is the authoritative text. When in doubt, read it. For commercial questions or edge cases, reach out via GitHub Issues.

## Alpha software disclaimer

::: warning
**BetweenRows is alpha software.** Read this carefully before deploying in any environment where stability and backward compatibility matter.
:::

### What "alpha" means for this project

- **APIs, configuration, and data formats may change between releases.** A `PUT` endpoint that exists in 0.14.x may be renamed, restructured, or removed in 0.15.x. Environment variables may change. Database schemas may migrate in ways that require a backup-before-upgrade discipline.
- **There is no backward compatibility guarantee** between minor versions during alpha. The CHANGELOG will call out breaking changes, but you are responsible for reading it before upgrading.
- **Pin your Docker image tag** to a specific version (e.g., `0.15.0`). Never use `:latest` in production during alpha.
- **The software is provided as-is with no warranty.** This is standard open-source boilerplate, but it is especially meaningful during alpha. Use at your own risk.
- **We welcome bug reports and feedback** via [GitHub Issues](https://github.com/getbetweenrows/betweenrows/issues). Alpha-stage feedback has an outsized effect on what we prioritize.

### What is stable

Even in alpha, some parts are more stable than others:

- **The policy enforcement model** (five types, deny-wins, scan-level rewriting) is unlikely to change. These decisions have test coverage and are load-bearing for the whole product.
- **The PostgreSQL wire protocol compatibility** is driven by external libraries (`pgwire`, DataFusion); behavior is reasonably stable.
- **The admin REST API shape** is likely to see small changes (new fields, new endpoints) but not large rewrites.

### What is less stable

- **Decision function APIs and compilation path** — Javy versioning, WASM harness.
- **Catalog discovery internals** — the way schemas/tables/columns are represented may be refactored.
- **CLI subcommands** — new ones will be added; existing ones are short-lived for now.
- **Specific endpoint URL conventions** — some may be relocated under more consistent prefixes.

### Recommended posture for alpha adopters

1. **Start in a non-critical environment.** Development, staging, or a lower-traffic data source in production.
2. **Back up before every upgrade.** See [Backups](/operations/backups).
3. **Read the changelog before upgrading.** See [Upgrading](/operations/upgrading).
4. **Subscribe to [GitHub Releases](https://github.com/getbetweenrows/betweenrows/releases)** for notifications.
5. **Report issues.** Early reports directly shape what we work on next.

## Warranty disclaimer

BetweenRows is provided "AS IS," without warranty of any kind, express or implied, including but not limited to the warranties of merchantability, fitness for a particular purpose and noninfringement. In no event shall the authors or copyright holders be liable for any claim, damages or other liability, whether in an action of contract, tort or otherwise, arising from, out of or in connection with the software or the use or other dealings in the software.

This is the standard disclaimer for open-source software. It is not a loophole — it is a reminder that BetweenRows has no commercial support contract and no guarantee that it will work for your use case. That said, we try hard, and we want it to work for you. File an issue.

## See also

- **[Upgrading](/operations/upgrading)** — practical upgrade hygiene
- **[Known limitations](/operations/known-limitations)** — things to be aware of before production
- **[Security Overview](/concepts/security-overview)** — for security reviewers
- **[Roadmap](/about/roadmap)** — where the project is heading
