---
description: Prepare and tag a new release for this project. Both apps (proxy and admin-ui) share a single version.
---

## Steps

### 1. Pre-flight checks

- Run `git status --porcelain` — abort if working tree is not clean.
- Run `git log --oneline $(git describe --tags --abbrev=0 2>/dev/null)..HEAD` to get commits since the last tag (or all commits if no tag exists).
- Run `git describe --tags --abbrev=0 2>/dev/null` to find the current version.

### 2. Sync docs-site against the release window

Invoke `/docs-sync <prev-tag>..HEAD` (where `<prev-tag>` is the tag from step 1) to detect drift between the release window's changes and the public docs. The command prints any drifts inline and waits for your decision before modifying anything. Review the findings and reply with `apply`, `apply N,M`, `skip N`, `skip all`, or `explain N`.

Docs-site edits become part of the release commit in step 4, so the published documentation matches the released version when the tag pushes. Do not proceed until `/docs-sync` reports either "no drift detected" or has finished applying your approved edits and `npm run build` has passed.

If this is the first release after adding `/docs-sync` to the workflow (or if drift looks suspicious), run `/docs-sync --full` once instead for a whole-codebase audit — it spawns parallel subagents against every docs-site cluster and surfaces accumulated drift.

### 3. Draft changelog entries

Draft entries **solely from the commit messages** gathered in step 1. Do not use the existing `## [Unreleased]` section in `CHANGELOG.md` as a source — it may contain entries for features that were planned but never implemented. The commits are the ground truth.

Group entries by:

- **Added** — new features
- **Changed** — changes to existing behaviour
- **Fixed** — bug fixes
- **Infrastructure** — CI, build, tooling changes (omit if trivial)

Format each entry with a component tag and bolded feature name, followed by detail sub-bullets where useful:
```
- **[Admin UI] Feature name** — one-line summary
  - detail point
  - detail point
- **[Proxy] Feature name** — one-line summary
- **[Both] Feature name** — one-line summary
```

Component tags: `[Proxy]`, `[Admin UI]`, `[Docs]`, `[Migration]`, `[Both]` (when a change spans proxy and admin-ui). Derive the tag from which files the commit touches — commits scoped to `proxy/` → `[Proxy]`, `admin-ui/` → `[Admin UI]`, `docs/` only → `[Docs]`, `migration/` → `[Migration]`. Commits touching both `proxy/` and `admin-ui/` → `[Both]`.

Do not mix flat one-liners with wall-of-text entries. Keep the top-level line scannable; put specifics in sub-bullets.

Do not include merge commits, formatting-only commits, or version bump commits. Do not classify a commit as **Fixed** if it fixes a bug in code that was not yet released — those are part of the new feature and belong under **Added** or **Changed**. **Fixed** is only for regressions or bugs in previously released behavior.

Show the draft to the user and ask them to confirm or edit it before proceeding. Also ask for the new version number (suggest one based on the changes: patch for fixes only, minor for new features, major for breaking changes).

### 4. Update files

Once the user confirms the version and changelog entries:

1. **`CHANGELOG.md`** — In the `## [Unreleased]` section, insert the confirmed entries. Then rename that section to `## [X.Y.Z] - YYYY-MM-DD` (today's date) and add a new empty `## [Unreleased]` above it.

2. **`proxy/Cargo.toml`** — Update the `version = "..."` field on the first occurrence.

3. **`migration/Cargo.toml`** — Update the `version = "..."` field on the first occurrence.

4. **`admin-ui/package.json`** — Update the `"version": "..."` field.

5. **`docs-site/docs/.vitepress/constants.ts`** — Update the `VERSION` constant. This is the single source of truth for the `{{VERSION}}` token substituted across every docs page at build time (see `docs-site/CLAUDE.md` → "Version-bump checklist"). Forgetting this leaves `docs.betweenrows.dev` showing the previous version in install/upgrade instructions.

### 5. Commit and tag

- Stage the five files above plus `Cargo.lock` (updated when Cargo.toml versions change) **and any docs-site edits applied in step 2**.
- Commit with message: `Release vX.Y.Z`
- Create an annotated tag: `git tag -a vX.Y.Z -m "Release vX.Y.Z"`

### 6. Finish and publish the GitHub release

1. Tell the user the local release is ready and show the exact command to push:

    ```
    git push && git push origin vX.Y.Z
    ```

   Remind them that:
   - Pushing the commit (`git push`) triggers CI: tests only.
   - Pushing the tag (`git push origin vX.Y.Z`) triggers CI: tests → build → publish Docker image tagged `X.Y.Z` and `X.Y` → deploy to Fly.io.
   - To redeploy an existing version without a new release, use the `workflow_dispatch` trigger in GitHub Actions with the version number (e.g. `1.2.3`).

2. **Wait for the user to confirm they've pushed the tag.** Do not run `gh release create` before the tag exists on the remote — the command fails with "tag not found."

3. Once confirmed, extract the `## [X.Y.Z]` section from `CHANGELOG.md` (everything from the `## [X.Y.Z] - YYYY-MM-DD` heading up to — but not including — the next `## [` heading). This is the curated release body.

4. Create the GitHub release, passing the extracted section via `--notes`:

    ```
    gh release create vX.Y.Z --title "vX.Y.Z" --notes "<extracted section>"
    ```

   Do not use `--generate-notes`. The auto-generated notes are a list of raw commit titles, which is noisier and less useful than the curated changelog entries we just wrote in step 3.

5. Report the release URL that `gh release create` prints, and remind the user that the Docker image and Fly deploy are still in progress in CI.
