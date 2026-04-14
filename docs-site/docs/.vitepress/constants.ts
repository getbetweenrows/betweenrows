// Centralized URLs and tokens for the docs site. Keep these here so
// future domain renames are a one-file edit. Mirrors the same pattern
// used in the www repo's src/config.ts.

export const SITE_URL = 'https://docs.betweenrows.dev'
export const WWW_URL = 'https://betweenrows.dev'
export const GITHUB_URL = 'https://github.com/getbetweenrows/betweenrows'
export const EDIT_PAGE_URL = `${GITHUB_URL}/edit/main/docs-site/docs/:path`
export const LICENSE_URL = `${GITHUB_URL}/blob/main/LICENSE`
// Shared with the www landing page — one brand card, one source of truth.
export const OG_IMAGE_URL = `${WWW_URL}/og-image.png`

// Current released proxy version. Bumped by /release alongside Cargo.toml
// and admin-ui/package.json. Substituted into markdown via the {{VERSION}}
// token — see the markdown.config hook in .vitepress/config.ts.
export const VERSION = '0.16.1'
