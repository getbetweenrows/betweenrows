# Internal Engineering Notes

This directory holds engineering-only content that should **not** appear on the public docs site. It is NOT part of any Astro content collection and is structurally excluded from the production build — `astro.config.mjs` scopes the Starlight collection to `src/content/docs/` only.

## What goes here

- **Architecture Decision Records (ADRs)** under `adr/`
- **Design notes** and rejected proposals under `design-notes/`
- **Reconciliation log** for the legacy `../docs/` overlap (`reconciliation-log.md`)
- **Meeting notes**, half-baked thoughts, scratch content
- Anything engineering-facing that benefits from living in git but should not ship to users

## What does NOT go here

- Public user documentation → `../src/content/docs/`
- Pre-migration engineering notes that still live in `betweenrows/docs/` → leave them there for now; they will be reconciled post-launch

## Rules

1. **Zero ceremony.** Plain Markdown (`.md`) with no frontmatter, no imports, no components. Same affordance as today's `docs/`.
2. **PR review still applies.** This directory is in git and reviewable like any other content. It is *hidden from users*, not *hidden from the team*.
3. **Never duplicate content** between here and `src/content/docs/`. If you promote a note to public, delete the internal copy in the same PR (or move it to `_archive/` if the original is considered historical).
4. **No secrets.** Credentials, tokens, and private keys do not belong in any git-tracked file. This directory is public on GitHub.
5. **Paranoia toggle:** if you're unsure whether a note belongs public or internal, default to internal. You can always promote later; you cannot easily un-publish.

## Note on the overlap period

Pre-migration content from `betweenrows/docs/` has **not** been copied here. It lives in its original location until Phase 2 reconciliation. During the overlap, engineers and Claude can find everything with:

```sh
grep -rn 'search term' internal/ ../docs/
```

See the Phase 2 reconciliation process in the launch docs plan for how `docs/` files get reviewed, merged, moved, or deleted after launch.
