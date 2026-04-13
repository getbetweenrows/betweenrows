# Launch blockers — docs-site

Items that could not be completed in-repo during the launch-polish pass and
are gated on external resources or design work. When the resource becomes
available, follow the exact recipe below to unblock.

This file lives under `internal/` and is **not** part of the VitePress
build — it will never ship to `docs.getbetweenrows.com`.

## 1. Cloudflare Web Analytics token

**Gate:** a Cloudflare Pages project for `docs.getbetweenrows.com` must exist.
The beacon token is issued per-project.

**Action:** `docs/.vitepress/config.ts` line 32 — replace the literal
`REPLACE_WITH_CLOUDFLARE_WEB_ANALYTICS_TOKEN` with the token from
Cloudflare dashboard → Web Analytics → `docs.getbetweenrows.com` → Snippet.

Do NOT delete the placeholder `<script>` block in the meantime — it's inert
as-is (beacon.min.js ignores the malformed token) and removing it would
mean re-adding four nested array literals later.

## 2. Cloudflare Stream demo video

**Gate:** 2–3 min demo recorded and uploaded to Cloudflare Stream.

**Action:** `docs/start/demo.md` lines 12–20 — in the `iframe` src, replace
`customer-REPLACE` with the real customer subdomain and `VIDEO_ID` with the
uploaded Stream video ID (Cloudflare dashboard → Stream → Settings for the
customer code, the video's edit page for the ID).

## 3. og-default.png

**Gate:** no design tool / OG brand card produced yet.

**Action:** drop a 1200×630 PNG brand card at
`docs/public/og-default.png`. The two `<meta>` tags in
`docs/.vitepress/config.ts` (lines 21, 23) already reference this path — no
config change needed. Until the file exists, social cards degrade gracefully
with platform-specific fallbacks.

Suggested content: BetweenRows logo + wordmark, tagline
"Row-level security for PostgreSQL", footer `docs.getbetweenrows.com`.

## 4. Screenshots (deferred from the capture run)

Populate with any frames from the A3d/e capture run that couldn't be captured
cleanly, with the reason. Empty if all 11 frames landed.

<!-- Format when there are deferrals:
- `<filename>.png` on `<page route>` — <reason> (e.g., "policy editor hadn't
  saved yet when selector stabilised", "need to re-record after the admin UI
  redesign in v0.15")
-->
