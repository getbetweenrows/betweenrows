---
title: Report an Issue
description: How to report bugs, request features, and disclose security vulnerabilities for BetweenRows. Security problems must go through the private vulnerability reporting form, not public issues.
---

# Report an Issue

BetweenRows is maintained by a small team, and feedback from early users directly shapes what we build next — reports and requests are genuinely welcome. Different kinds of feedback go through different channels; pick the one that matches what you have.

## Security vulnerabilities

**Do not open a public GitHub issue for security problems.** Filing a public issue for a vulnerability means an attacker can read it before a fix ships.

Use GitHub's private vulnerability reporting form:

👉 **[github.com/getbetweenrows/betweenrows/security/advisories/new](https://github.com/getbetweenrows/betweenrows/security/advisories/new)**

Reports submitted through the form are visible only to project maintainers. See [SECURITY.md](https://github.com/getbetweenrows/betweenrows/blob/main/SECURITY.md) for the full disclosure policy — what to include, what to expect, scope, and credit.

## Bugs and unexpected behavior

Open a public GitHub issue:

👉 **[github.com/getbetweenrows/betweenrows/issues/new](https://github.com/getbetweenrows/betweenrows/issues/new)**

What helps us reproduce and fix it:

- **Proxy version** — check the admin UI footer, or run `curl http://localhost:5435/health`
- **Component** — proxy, admin UI, admin REST API, migration, CLI
- **What you did** — minimum SQL, API call, or UI steps to trigger the issue
- **What you expected** — the correct outcome, in your words
- **What happened instead** — actual output, error message, or screenshot
- **Logs (if relevant)** — proxy stdout/stderr, query audit log entry, admin audit log entry

Before filing, a quick search through existing [open issues](https://github.com/getbetweenrows/betweenrows/issues) often finds a match — upvoting an existing report is more useful than a duplicate.

## Feature requests and design discussions

Also GitHub Issues, same form as bugs. Tell us:

- **Your use case** — what are you actually trying to accomplish?
- **Why the current behavior does not work** — is a feature missing, awkward, or subtly wrong?
- **A rough shape of the ideal outcome** — not a full design, just enough for us to understand what "good" looks like to you.

Non-trivial features benefit from a design conversation before any PR is written. If you have strong opinions on the shape, opening an issue to talk it through first saves rework.

## Questions and how-to

Start with the documentation:

- **Search** — the top nav has a search box that indexes every page.
- **Guides** — the [Guides](/guides/data-sources) section covers the common workflows end-to-end.
- **Reference** — the [Reference](/reference/configuration) section has the exhaustive field tables and expression syntax.
- **LLM-friendly bundle** — if you prefer to ask your own chat model, the full docs are available as a single text file at [/llms-full.txt](/llms-full.txt).

If the docs do not answer your question, that itself is a useful signal — [open a GitHub issue](https://github.com/getbetweenrows/betweenrows/issues) describing what you were looking for and where you looked. We'll fill in the gap: either pointing you to something you missed, or extending the docs to cover your case. "I couldn't find how to do X" is a welcome issue type, and those reports directly shape which pages get written or expanded next.
