# BetweenRows Documentation Site

The public documentation for BetweenRows, built with [VitePress](https://vitepress.dev/) and deployed to `docs.betweenrows.dev`.

## Commands

```sh
npm install        # install dependencies
npm run dev        # start dev server at http://localhost:5174
npm run build      # build the production site
npm run preview    # preview the build locally
```

## Layout

- `docs/` — public documentation pages (Markdown)
- `docs/.vitepress/` — VitePress config, theme, build cache (the `dist/` output is gitignored)
- `docs/public/` — static assets served as-is (favicon, logo, robots.txt)

The canonical demo stack (schema + seed + compose file + setup script) used
by the docs screenshots lives in `../scripts/demo_ecommerce/` — see its
README for the one-shot bootstrap.

See `CLAUDE.md` in this directory for writing conventions and agent instructions.
