import { defineConfig } from 'vitepress';
import llmstxt from 'vitepress-plugin-llms';
import { copyOrDownloadAsMarkdownButtons } from 'vitepress-plugin-llms';

// IMPORTANT: VitePress builds everything under `docs/` (this directory's
// parent). `../internal/` and legacy `../../docs/` (inside the betweenrows
// repo root) are NOT part of the site and are not built. See docs-site/CLAUDE.md
// for the overlap rules and the launch docs plan for the reasoning.

export default defineConfig({
  title: 'BetweenRows',
  description:
    'A fully customizable data access governance layer — a SQL-aware proxy that enforces fine-grained access policies across your databases, warehouses, and lakehouses.',
  lang: 'en-US',
  cleanUrls: true,
  lastUpdated: true,
  ignoreDeadLinks: 'localhostLinks',
  sitemap: {
    hostname: 'https://docs.getbetweenrows.com',
  },
  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/favicon.svg' }],
    ['meta', { property: 'og:image', content: '/og-default.png' }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:image', content: '/og-default.png' }],
    // Cloudflare Web Analytics — privacy-first, no cookies, one-line beacon.
    // Replace the token when the Cloudflare Pages project is created.
    [
      'script',
      {
        defer: '',
        src: 'https://static.cloudflareinsights.com/beacon.min.js',
        'data-cf-beacon':
          '{"token": "REPLACE_WITH_CLOUDFLARE_WEB_ANALYTICS_TOKEN"}',
      },
    ],
  ],
  vite: {
    plugins: [llmstxt()],
  },
  markdown: {
    config(md) {
      md.use(copyOrDownloadAsMarkdownButtons);
    },
  },
  themeConfig: {
    logo: '/logo.svg',
    siteTitle: 'BetweenRows',

    nav: [
      { text: 'Quickstart', link: '/start/quickstart' },
      { text: 'How It Works', link: '/concepts/architecture' },
      { text: 'Guides', link: '/guides/data-sources' },
      { text: 'Reference', link: '/reference/configuration' },
      { text: 'Docs for AI', link: '/llms-full.txt' },
    ],

    sidebar: {
      '/': [
        {
          text: 'Start',
          items: [
            { text: 'Introduction', link: '/start/introduction' },
            { text: 'Quickstart', link: '/start/quickstart' },
          ],
        },
        {
          text: 'How It Works',
          items: [
            { text: 'Architecture', link: '/concepts/architecture' },
            { text: 'Policy Model', link: '/concepts/policy-model' },
            {
              text: 'Security Overview',
              link: '/concepts/security-overview',
            },
            { text: 'Threat Model', link: '/concepts/threat-model' },
          ],
        },
        {
          text: 'Guides',
          items: [
            { text: 'Data Sources', link: '/guides/data-sources' },
            { text: 'Users & Roles', link: '/guides/users-roles' },
            {
              text: 'User Attributes (ABAC)',
              link: '/guides/attributes',
            },
            {
              text: 'Policies',
              link: '/guides/policies/',
              collapsed: false,
              items: [
                { text: 'Row Filters', link: '/guides/policies/row-filters' },
                {
                  text: 'Column Masks',
                  link: '/guides/policies/column-masks',
                },
                {
                  text: 'Column Allow & Deny',
                  link: '/guides/policies/column-allow-deny',
                },
                { text: 'Table Deny', link: '/guides/policies/table-deny' },
              ],
            },
            {
              text: 'Decision Functions',
              link: '/guides/decision-functions',
            },
            {
              text: 'Audit & Debugging',
              link: '/guides/audit-debugging',
            },
            {
              text: 'Recipes',
              link: '/guides/recipes/',
              collapsed: false,
              items: [
                {
                  text: 'Multi-Tenant Isolation',
                  link: '/guides/recipes/multi-tenant-isolation',
                },
                {
                  text: 'Per-User Deny Exceptions',
                  link: '/guides/recipes/deny-exceptions',
                },
              ],
            },
          ],
        },
        {
          text: 'Reference',
          items: [
            { text: 'Configuration', link: '/reference/configuration' },
            { text: 'Policy Types', link: '/reference/policy-types' },
            {
              text: 'Template Expressions',
              link: '/reference/template-expressions',
            },
            { text: 'Audit Log Fields', link: '/reference/audit-log-fields' },
            { text: 'Demo Schema', link: '/reference/demo-schema' },
            { text: 'CLI', link: '/reference/cli' },
            { text: 'Admin REST API', link: '/reference/admin-rest-api' },
            { text: 'Glossary', link: '/reference/glossary' },
          ],
        },
        {
          text: 'Operate',
          items: [
            {
              text: 'Deployment',
              collapsed: false,
              items: [
                { text: 'Docker', link: '/installation/docker' },
                { text: 'Fly.io', link: '/installation/fly' },
                { text: 'From Source', link: '/installation/from-source' },
              ],
            },
            { text: 'Upgrading', link: '/operations/upgrading' },
            { text: 'Backups & Recovery', link: '/operations/backups' },
            {
              text: 'Rename Safety',
              link: '/operations/rename-safety',
            },
            {
              text: 'Troubleshooting',
              link: '/operations/troubleshooting',
            },
            {
              text: 'Known Limitations',
              link: '/operations/known-limitations',
            },
          ],
        },
        {
          text: 'About',
          items: [
            { text: 'Changelog', link: '/about/changelog' },
            { text: 'Roadmap', link: '/about/roadmap' },
            {
              text: 'License & Alpha Status',
              link: '/about/license',
            },
            { text: 'Report an Issue', link: '/about/report-an-issue' },
          ],
        },
      ],
    },

    socialLinks: [
      {
        icon: 'github',
        link: 'https://github.com/getbetweenrows/betweenrows',
      },
    ],

    editLink: {
      pattern:
        'https://github.com/getbetweenrows/betweenrows/edit/main/docs-site/docs/:path',
      text: 'Edit this page on GitHub',
    },

    search: {
      provider: 'local',
    },

    footer: {
      message:
        'Released under the <a href="https://github.com/getbetweenrows/betweenrows/blob/main/LICENSE">Elastic License v2</a>. <strong>Alpha software.</strong>',
      copyright: 'Copyright © 2026 BetweenRows',
    },
  },
});
