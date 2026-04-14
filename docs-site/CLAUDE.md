# docs-site Instructions

## Layout

- **Public docs**: `docs/` — plain `.md`, rendered to `docs.betweenrows.dev` by VitePress. Sidebar/theme in `docs/.vitepress/config.ts`. URLs and the CF Web Analytics token are centralized in `docs/.vitepress/constants.ts`. Static assets in `docs/public/`.
- **Upstream design docs**: `../docs/` — the betweenrows repo's design-time documentation (policy system philosophy, roadmap rationale, security vector taxonomy, user story catalog). This is the source of truth for development rationale, and the site **reads from** it:
  - `concepts/threat-model.md` transcludes `../../../docs/security-vectors.md` via `<!--@include:-->`. Edit the source, not the wrapper.
  - Guide content drawn from `../docs/permission-system.md` is editorially translated for a user audience — see the `/docs-sync` command for the reconciliation flow.
  - **Direction of dependency**: this site reads from `../docs/` and from source code. Nothing in the betweenrows repo outside `docs-site/` may reference `docs-site/`. Details in `../.claude/CLAUDE.md` → "Documentation architecture".

Where new content goes:

- New **public user docs** → `docs-site/docs/`
- New **design or architecture notes for betweenrows development** → `../docs/`

## Sidebar organization

The sidebar has five top-level sections, each with a clear job. **There is no "Reference" or "Appendix" section** — lookup material lives next to what it belongs to.

| Section | Answers | Contents |
|---|---|---|
| **Start** | Get hands on it | Introduction, Quickstart, Demo Schema |
| **Concepts** | Understand the system | Architecture, Policy Model, Security Overview, Threat Model, Known Limitations, Glossary |
| **Features** | What each feature IS | Data Sources, Users & Roles, User Attributes, Policies (w/ Policy Types sub-group, Template Expressions, Decision Functions), Audit & Debugging |
| **Guides** | How to DO things | Deployment/{Docker, Fly.io, From Source, Configuration}, Upgrading, Backups, Troubleshooting, Rename Safety, Recipes/{...} |
| **About** | Meta | Changelog (external), Roadmap, License (external), Report an Issue |

**Rules that fall out of this shape:**

- Per-feature reference material (field tables, validation rules, JSON shapes) lives **inside** the feature page, not in a separate reference section. The 7-section guide template has a "Field reference" step — that's the home.
- Template Expressions and Decision Functions are nested under Policies in the sidebar because they're policy-adjacent (the language and conditional gating), even though their files still live at `reference/template-expressions.md` and `guides/decision-functions.md` for URL stability.
- Configuration (env vars) lives under Guides > Deployment — it's operational config, not a feature.
- Known Limitations and Glossary live under Concepts — they're "understand the system, its limits, its vocabulary."
- Demo Schema lives under Start — it's the dataset tutorials and recipes use.
- File paths under `reference/` are kept for URL stability even though there's no sidebar "Reference" section — users who bookmarked those URLs still land on the right content.
- `operations/` files are grouped under Guides in the sidebar; the folder name is internal.

## What belongs on the site

The goal is a small set of pages that are each worth reading, not a large set that inflates the sidebar. Prefer deleting over bloat.

- **Only add a page if there's content a user would go looking for AND no other reasonable home exists.** If the content is a plain-language restatement of something already authoritative (the `LICENSE` file, the `CHANGELOG`, an existing guide), skip the page and link to the canonical source directly — external links in the sidebar (`link: LICENSE_URL`) are fine. Retarget inbound references when you delete a page; don't leave them pointing at a tombstone.
- **No stability claims while in early beta.** "What is stable / What is less stable" subsystem lists, "the X API is stable" statements, and similar reassurances are liabilities: users build compliance stories around guarantees you don't actually make, and every refactor breaks a promise in writing. Until 1.0, the only stability guidance we publish is the operational one — pin your tag, back up, read the changelog before upgrading. No subsystem-level promises.
- **No pages that exist just to host a link.** If a page would be ten lines of prose framing around a single outbound link, the outbound link belongs directly in the sidebar / footer / parent page instead.

The test for any new page: "if I delete this in six months, will users notice a gap, or will they just follow the links they were going to follow anyway?" If the second, don't write it.

## Finding things

```sh
grep -r '^description:' docs --include='*.md'      # topic overview of public pages
```

## Writing conventions

- **Plain Markdown.** GitHub-flavored, no `.mdx`.
- **Pages stay substantial.** Don't fragment one concept across tiny files.
- **Callouts** use markdown-it containers: `::: tip|info|warning|danger|details`.
- **Numbered steps** are just numbered lists — VitePress styles them.
- **Cross-page links** use absolute paths without trailing slashes: `[Architecture](/concepts/architecture)`.
- **Frontmatter** is `title` + `description` only. Sidebar order lives in `.vitepress/config.ts`.
- **Every page MUST have `description:` frontmatter.** It feeds `<meta name="description">`, the internal grep-TOC, and the `llms.txt` section index. Missing descriptions break the index.
- **Every content page MUST have a body `# Title` H1 matching the frontmatter `title:`.** VitePress renders the visible page heading from the first body H1, and `vitepress-plugin-llms` injects the Copy/Download-as-Markdown buttons after the first `h1` token — skipping the H1 silently disables both. Use Title Case for both. Exception: custom-layout pages like `index.md` (`layout: home` with a hero block) intentionally omit the body H1 because the hero supplies the visible heading.

## Terminology

- Prose: "data source" (two words). Identifiers/URLs/code: `datasource` (one word).
- "policy" (not "permission"), "NULL" uppercase in SQL contexts.
- "row filter" in prose, `row_filter` as the type identifier. Same for `column_mask`, `column_allow`, `column_deny`, `table_deny`.
- See `reference/glossary.md` for the full standardized vocabulary. Terminology standardization happens once in the glossary — other pages just use the terms.

## Sensitive content rules (security-adjacent)

- **No historical bugs in public docs.** Git history is the permanent record.
- **No internal test names, source file paths, or internal function names** in public pages. Users care about *what* is guaranteed, not *where*.
- **`concepts/threat-model.md` is a thin wrapper** that transcludes `../docs/security-vectors.md` via `<!--@include:-->`. Edit the source, not the wrapper. The wrapper contains only frontmatter + the include directive.
- **`concepts/security-overview.md` is a curation page only.** No new guarantee claims — only links and framing.

## Canonical sources for shared tables

One table, one home. Other pages link, don't duplicate.

| Material | Canonical home |
|---|---|
| Policy types table, targets matrix, definition-by-type JSON, validation rules, wildcard glob syntax | `guides/policies/index.md` |
| Template variables, missing-attribute behavior, SQL subset, NULL semantics | `reference/template-expressions.md` |
| Access modes (deployment perspective) | `guides/data-sources.md` |
| Access modes (conceptual semantics) | `concepts/policy-model.md` |
| Role assignment scopes | `guides/users-roles.md` |
| Audit log field meanings (query audit + admin audit) | `guides/audit-debugging.md` |
| Environment variables | `reference/configuration.md` |
| Terminology / glossary | `reference/glossary.md` |

## Writing a feature guide

Every guide follows this 7-section template:

1. **Purpose & when to use** (2–3 sentences)
2. **Field reference table** — name, type, default, required, purpose, recommendation, catches. Must cover every field in the admin UI form AND the backing Rust struct.
3. **Step-by-step tutorial** — a screenshot at every UI step. Inline the demo-schema subset the tutorial uses at the top of "Step 1."
4. **Patterns & recipes** — 3–5 common patterns with example code and expected outcomes
5. **Composition** — how this feature interacts with others. Link, don't duplicate.
6. **Limitations & catches** — brief callouts; link to `operations/known-limitations.md` for depth.
7. **Troubleshooting & next steps** — anchored links to `operations/troubleshooting.md`.

### Before writing

Catches and subtle behaviors often live in inline comments and internal docs. Surface them first:

```sh
grep -nrE '// (NOTE|IMPORTANT|WARNING|SECURITY|TODO|HACK|GOTCHA|CAVEAT)' \
  ../proxy/src ../admin-ui/src
grep -nE '<feature-keyword>' ../docs/security-vectors.md ../docs/permission-system.md
```

Fold anything interesting into the guide's Field reference, Composition, or Limitations sections.

### Diff against the UI + data model

Before committing, reconcile the field reference table against:
- `../admin-ui/src/components/<Feature>Form.tsx` — every form field
- `../proxy/src/entity/<feature>.rs` — every column

Gaps between the two are worth noting in the guide.

## Demo stack (for screenshot capture)

Lives at `../scripts/demo_ecommerce/`. Multi-tenant e-commerce schema, seeded users, pre-configured policies.

### Bring up

```sh
cd ../scripts/demo_ecommerce
docker compose -f compose.demo.yaml up -d
source .venv/bin/activate     # REQUIRED — setup.sh uses `python3` which needs psycopg2 from the venv
./setup.sh
```

First-time venv setup: `python3 -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt`.

### Reset

**Phase 1 (upstream seed) is NOT idempotent.** Always `down -v` first before re-running.

```sh
docker compose -f compose.demo.yaml down -v
```

Phase 2 (admin bootstrap) and Phase 3 (policies) ARE idempotent.

### Sanity check

```sh
curl http://127.0.0.1:5435/health                 # {"version":"0.X.Y"}
PGPASSWORD='Demo1234!' psql "postgresql://alice@127.0.0.1:5434/demo_ecommerce" \
  -c "SELECT DISTINCT org FROM orders"            # → acme
```

See `../scripts/demo_ecommerce/README.md` for env var overrides.

## Screenshot capture (Playwright MCP)

### The recipe

1. Bring up the demo stack (above).
2. `browser_resize(width: 1440, height: 900)`
3. `browser_navigate("http://127.0.0.1:5435")`, snapshot, fill login with `admin` / `changeme`, click Sign in.
4. For each target page: navigate, `browser_snapshot` for refs, fill form fields with realistic example data, then:
   ```
   browser_take_screenshot(
     type: "png",
     filename: "docs-site/docs/public/screenshots/<name>-v<version>.png",
     fullPage: true
   )
   ```
5. Tear down: `cd ../scripts/demo_ecommerce && docker compose -f compose.demo.yaml down -v`

### Rules

- **Always `fullPage: true`.** The admin UI uses fixed sidebar + inner-scroll main content; viewport-only captures miss anything below 900px.
- **Always 1440×900.** Don't use browser zoom — Chrome's zoom state leaks across `setViewportSize` calls and produces stuck non-1 DPR values. If captures come out at non-1440 widths, `pkill -f mcp-chrome` and restart.
- **Don't use element-only screenshots** (`element` + `ref` params). They lose the sidebar context.
- **Filename CWD is the repo root** (`/Users/.../betweenrows/`), so `docs-site/docs/public/screenshots/...` lands in the right place.
- **Verify after a session:**
  ```sh
  cd docs-site/docs/public/screenshots
  for f in *v0.15.png; do
    sips -g pixelWidth "$f" | awk -v f="$f" '/pixelWidth/ && $2 != 1440 {print f, $2}'
  done
  ```

### Against the Vite dev server

When you need UI fixes to appear in screenshots without rebuilding Docker: `cd ../admin-ui && npm run dev` runs Vite at `http://localhost:5173` proxying `/api` to the running proxy. Navigate captures to `:5173` instead of `:5435`.

### Admin UI URLs

| URL | Page |
|---|---|
| `/` | Users list (default landing) |
| `/users/create`, `/users/{id}/edit` | User create / edit |
| `/roles`, `/roles/create`, `/roles/{id}` | Roles (detail has Details/Members/Inheritance/Data Sources/Policies/Activity tabs) |
| `/attributes`, `/attributes/create` | Attribute definitions |
| `/policies`, `/policies/create`, `/policies/{id}/edit` | Policies (decision functions attach here, not a separate page) |
| `/datasources`, `/datasources/create`, `/datasources/{id}/edit`, `/datasources/{id}/catalog` | Data sources |
| `/query-audit`, `/admin-audit` | Audit logs |

### Gotchas

- **Decision functions live inside the policy editor.** On `/policies/{id}/edit` → Decision Function section → toggle → "+ Create New" opens the modal.
- **Role inheritance direction**: effective members flow from child to parent via BFS downward. A child role's members appear in the PARENT's effective-members tab with "via role '{child}'" annotation. For a rich effective-members screenshot, capture the parent.
- **Mutations take a beat to reflect.** If counts look stale after a Save click, re-navigate before capturing.
- **Sidebar labels ≠ page headings.** Guides use headings: "Attribute Definitions" (sidebar: "Attributes"), "Query Audit Log" (sidebar: "Query Logs"), "Admin Audit Log" (sidebar: "Admin Logs").

### Generate audit entries before capturing audit screens

```sh
PGPASSWORD='Demo1234!' psql "postgresql://alice@127.0.0.1:5434/demo_ecommerce" \
  -c "SELECT first_name, ssn, org FROM customers LIMIT 5"
PGPASSWORD='Demo1234!' psql "postgresql://bob@127.0.0.1:5434/demo_ecommerce" \
  -c "SELECT DISTINCT org FROM orders"
```

### Naming

- Format: `<section>-<step>-v<major.minor>.png`, e.g. `data-sources-connection-form-v0.15.png`.
- Version suffix tracks the **docs target version**, not the running proxy.
- On a version bump where the UI changed, re-capture and replace files (don't accumulate).
- Every page embedding screenshots keeps a bottom comment listing capture targets:
  `<!-- screenshots: [foo-v0.15.png, bar-v0.15.png] -->`

### Fix the app, not the screenshot

If something looks wrong in a screenshot, it probably looks wrong in production too. Fix `admin-ui/src/`, re-capture against the Vite dev server. Example: the Name column on list pages was capped via `max-w-xs` wrapper + `truncate` after screenshot capture revealed it ballooning to fit descriptions.

### Placeholders → image refs

Write guides with placeholders, then replace after capture:

```md
<!-- screenshot: data-sources-connection-form-v0.15.png -->
```
becomes
```md
![Data source connection form with demo PostgreSQL details](/screenshots/data-sources-connection-form-v0.15.png)
```

Images are served from `docs/public/screenshots/`, referenced as `/screenshots/...`. Alt text should be descriptive (5–15 words) — it appears in `llms-full.txt` and matters for accessibility and SEO.

## llms.txt & AI consumption

### Plugin wiring

- `npm install -D vitepress-plugin-llms`
- In `docs/.vitepress/config.ts`:
  ```ts
  import llmstxt from 'vitepress-plugin-llms';
  import { copyOrDownloadAsMarkdownButtons } from 'vitepress-plugin-llms';

  export default defineConfig({
    vite: { plugins: [llmstxt()] },
    markdown: { config(md) { md.use(copyOrDownloadAsMarkdownButtons); } },
    // ...
  });
  ```
- In `docs/.vitepress/theme/index.ts`:
  ```ts
  import CopyOrDownloadAsMarkdownButtons from
    'vitepress-plugin-llms/vitepress-components/CopyOrDownloadAsMarkdownButtons.vue';
  // register in enhanceApp({ app }) with app.component(...)
  ```

### What it generates (at build time)

| URL | Contents |
|---|---|
| `/llms.txt` | Section index of every page with title + description (llmstxt.org standard) |
| `/llms-full.txt` | Full concatenated content of all pages |
| Per-page `.md` | What the Copy/Download buttons serve |

### Consumption paths

- **Per-page Copy/Download as Markdown** buttons — rendered by the plugin on every page. This is the primary affordance for users who want to feed a page to their own chat model.
- `/llms.txt` and `/llms-full.txt` are generated by the plugin and discoverable by convention — no nav entry. AI agents find them at the well-known URLs; humans who want the full dump can type the URL directly.

**No "Docs for AI" nav entry.** It was removed because users found it confusing — the label suggested the link was for an AI chatbot rather than a plain-text dump, and the per-page Copy button already covers the "get this as markdown" use case. Do not re-add it.

### Do NOT add a custom "Ask ChatGPT/Claude" deep-link component

Dropped from the plan: chat provider query-param APIs are unreliable, users already have a chat window open, and the per-page Copy button covers the real use cases.

## Click-to-zoom on images (medium-zoom)

Every image in the main content area is clickable → full-resolution overlay, Esc to close. This is why 1440×900 captures with small UI are fine: readers click to enlarge.

Wired in `docs/.vitepress/theme/index.ts`:

```ts
import { onMounted, watch, nextTick } from 'vue';
import { useRoute } from 'vitepress';
import mediumZoom from 'medium-zoom';

// inside the default export, alongside enhanceApp:
setup() {
  const route = useRoute();
  const initZoom = () => {
    mediumZoom('.vp-doc img', { background: 'var(--vp-c-bg)', margin: 24 });
  };
  onMounted(() => initZoom());
  watch(() => route.path, () => nextTick(() => initZoom()));
}
```

`.vp-doc img` matches every image in VitePress's main content container. If you're tempted to reach for smaller viewports or heavy zoom settings to "make screenshots bigger," stop — medium-zoom already solves that.

## Pre-release checklist (new guide)

- [ ] 7-section template applied
- [ ] Field reference diffed against `admin-ui` form + `proxy/src/entity` struct
- [ ] Code-scan pass completed (`// NOTE|IMPORTANT|WARNING|SECURITY|TODO|HACK` grep)
- [ ] Security-vectors + permission-system cross-checks done for security-adjacent features
- [ ] Every UI step has a 1440×900 fullPage screenshot with descriptive alt text
- [ ] Limitations/catches link to `operations/known-limitations.md`
- [ ] `description:` frontmatter is specific (not generic)
- [ ] `npm run build` passes (dead-link check)
- [ ] Page appears in generated `llms.txt`
- [ ] Glossary updated if new terminology introduced

## Version-bump checklist

The `/release` command automatically invokes `/docs-sync v<prev-tag>..HEAD` as step 1, which detects drift between the release window's changes and the public pages and proposes specific edits for review. Most items in this checklist are covered by that flow — it reads like a manual fallback when you need to audit without running the release.

When the proxy releases `v0.X.Y` (manual version):

- [ ] Bump `VERSION` in `docs/.vitepress/constants.ts` — single source of truth for the `{{VERSION}}` token substituted across all guides at build time
- [ ] Re-capture screenshots if the admin UI changed visually
- [ ] Run the code-scan pass on new source files; fold catches into relevant guides
- [ ] Check `../docs/security-vectors.md` for new vectors; reconcile with guide limitations
- [ ] `npm run build` passes
- [ ] Fresh-reader walkthrough: follow the Quickstart end-to-end

For a deeper audit (e.g., before v1.0 or after a long quiet period), run `/docs-sync --full` — spawns parallel subagents against every docs-site cluster and prints any accumulated drift inline for review. This is not part of the release workflow; run it manually when you want the deeper check.

## No video / demo page policy

The docs site does NOT host video walkthroughs. Reasons: guides + screenshots cover the text flow, videos rot on every UI change, and `/llms-full.txt` lets users ask their own chat model for a narrated walkthrough. Marketing video (if we ever make one) belongs on `www/`.

Do not re-add `start/demo.md` or any other video-embedding page without a prior discussion.

## Running the site locally

```sh
npm install
npm run dev       # http://localhost:5174
npm run build     # generates docs/.vitepress/dist/
npm run preview   # preview the build locally
```

## Deployment

Deployed to Cloudflare Pages as the `betweenrows-docs` project, served at `docs.betweenrows.dev`. Auto-deploys on push to `main` via `.github/workflows/docs-site-deploy.yml`, which is gated by a `paths: ['docs-site/**', '.github/workflows/docs-site-deploy.yml']` filter so non-docs commits don't trigger a rebuild. Uses `cloudflare/wrangler-action@v3` with the `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` repo secrets (the public-repo token is separate from the private www repo's token).

URLs (`SITE_URL`, `WWW_URL`, `GITHUB_URL`, `EDIT_PAGE_URL`, `LICENSE_URL`, `OG_IMAGE_URL`) are centralized in `docs/.vitepress/constants.ts`. The OG image is the same brand card hosted on the www site at `https://betweenrows.dev/og-image.png` — one image for both surfaces.

Cloudflare Web Analytics is enabled in **automatic mode** — the beacon is injected at the edge by Cloudflare for all proxied traffic, no snippet in the VitePress config. Configured per-site in the Cloudflare dashboard under Web Analytics. The docs site has its own separate site/dashboard from the www landing page.
