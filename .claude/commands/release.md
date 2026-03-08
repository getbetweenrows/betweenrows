---
description: Prepare and tag a new release for this project. Both apps (proxy and admin-ui) share a single version.
---

## Steps

### 1. Pre-flight checks

- Run `git status --porcelain` — abort if working tree is not clean.
- Run `git log --oneline $(git describe --tags --abbrev=0 2>/dev/null)..HEAD` to get commits since the last tag (or all commits if no tag exists).
- Run `git describe --tags --abbrev=0 2>/dev/null` to find the current version.

### 2. Draft changelog entries

Draft entries **solely from the commit messages** gathered in step 1. Do not use the existing `## [Unreleased]` section in `CHANGELOG.md` as a source — it may contain entries for features that were planned but never implemented. The commits are the ground truth.

Group entries by:

- **Added** — new features
- **Changed** — changes to existing behaviour
- **Fixed** — bug fixes
- **Infrastructure** — CI, build, tooling changes (omit if trivial)

Format each entry as a bolded feature name with a short summary, followed by detail sub-bullets where useful:
```
- **Feature name** — one-line summary
  - detail point
  - detail point
```
Do not mix flat one-liners with wall-of-text entries. Keep the top-level line scannable; put specifics in sub-bullets.

Do not include merge commits, formatting-only commits, or version bump commits. Do not classify a commit as **Fixed** if it fixes a bug in code that was not yet released — those are part of the new feature and belong under **Added** or **Changed**. **Fixed** is only for regressions or bugs in previously released behavior.

Show the draft to the user and ask them to confirm or edit it before proceeding. Also ask for the new version number (suggest one based on the changes: patch for fixes only, minor for new features, major for breaking changes).

### 3. Update files

Once the user confirms the version and changelog entries:

1. **`CHANGELOG.md`** — In the `## [Unreleased]` section, insert the confirmed entries. Then rename that section to `## [X.Y.Z] - YYYY-MM-DD` (today's date) and add a new empty `## [Unreleased]` above it.

2. **`proxy/Cargo.toml`** — Update the `version = "..."` field on the first occurrence.

3. **`migration/Cargo.toml`** — Update the `version = "..."` field on the first occurrence.

4. **`admin-ui/package.json`** — Update the `"version": "..."` field.

### 4. Commit and tag

- Stage the four files above plus `Cargo.lock` (updated when Cargo.toml versions change).
- Commit with message: `Release vX.Y.Z`
- Create an annotated tag: `git tag -a vX.Y.Z -m "Release vX.Y.Z"`

### 5. Finish

Tell the user the release is ready and show the exact command to push:

```
git push && git push origin vX.Y.Z
```

Remind them that:
- Pushing the commit (`git push`) triggers CI: tests only.
- Pushing the tag (`git push origin vX.Y.Z`) triggers CI: tests → build → publish Docker image tagged `X.Y.Z` and `X.Y` → deploy to Fly.io.
- To redeploy an existing version without a new release, use the `workflow_dispatch` trigger in GitHub Actions with the version number (e.g. `1.2.3`).
