# Security Policy

## Reporting a vulnerability

**Please do not open a public GitHub issue for security problems.**

Use GitHub's private vulnerability reporting form:

👉 **https://github.com/getbetweenrows/betweenrows/security/advisories/new**

Reports submitted through this form are visible only to the project maintainers. You do not need a CVE number or a specific format — a clear description of the issue, steps to reproduce, and the affected version are enough.

## What to include

- **Affected version(s)** — proxy image tag or commit SHA, admin UI version if relevant.
- **Component** — proxy, admin UI, policy hook, migration, etc.
- **Reproduction steps** — minimum commands or configuration to trigger the issue.
- **Impact** — what an attacker could read, write, bypass, or crash.
- **Suggested remediation** (optional) — if you have a fix in mind.

## Scope

In scope:

- The **proxy** (Rust, `proxy/`) — wire-protocol handling, query rewriting, policy enforcement, audit logging.
- The **admin UI** (React, `admin-ui/`) — authentication, authorization, CSRF, XSS, IDOR, etc.
- The **admin REST API** — management-plane endpoints on port 5435.
- **Migrations** (`migration/`) — SQL schema correctness, data-destruction risks.
- **Decision functions** (JavaScript → WASM via Javy) — sandbox escape, fuel/memory limit bypass.
- **Docker images** published to `ghcr.io/getbetweenrows/betweenrows`.

Out of scope:

- Vulnerabilities in upstream PostgreSQL itself.
- Misconfiguration issues where the documented guidance was not followed (unless the documentation itself is misleading).
- Denial-of-service via deliberately pathological SQL on an unprotected deployment. The proxy is not a DDoS shield.
- Third-party integrations not maintained in this repository.

## Alpha caveat

BetweenRows is pre-1.0 alpha software. The threat model, defenses, and known limitations are documented in the public docs:

- [Threat Model](docs-site/docs/concepts/threat-model.md)
- [Security Overview](docs-site/docs/concepts/security-overview.md)
- [Known Limitations](docs-site/docs/operations/known-limitations.md)

Issues already listed in those pages are known trade-offs, not vulnerabilities — though reports that sharpen or contradict them are welcome.

## Credit

We will credit reporters in the release notes unless you prefer to stay anonymous. Please let us know your preference when you report.
