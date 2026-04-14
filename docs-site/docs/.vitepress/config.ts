import { defineConfig, type DefaultTheme } from 'vitepress';
import llmstxt from 'vitepress-plugin-llms';
import { copyOrDownloadAsMarkdownButtons } from 'vitepress-plugin-llms';
import {
  SITE_URL,
  GITHUB_URL,
  EDIT_PAGE_URL,
  LICENSE_URL,
  CHANGELOG_URL,
  OG_IMAGE_URL,
  VERSION,
} from './constants';

// vitepress-plugin-llms warns on every sidebar entry whose link doesn't
// resolve to a markdown file in the docs tree — including intentional
// external links (Changelog, License) we wire into the About section.
// This helper strips external entries before the plugin sees the sidebar,
// so llms.txt / llms-full.txt generation stays warning-free. The public
// site's sidebar is unaffected — this is a plugin-scoped copy.
function stripExternalSidebarLinks(
  sidebar: DefaultTheme.Sidebar | undefined,
): DefaultTheme.Sidebar | undefined {
  if (!sidebar) return sidebar;
  const isExternal = (link: string | undefined) =>
    typeof link === 'string' && /^(?:https?:)?\/\//.test(link);
  const filterItems = (
    items: DefaultTheme.SidebarItem[],
  ): DefaultTheme.SidebarItem[] =>
    items
      .filter((item) => !isExternal(item.link))
      .map((item) => (item.items ? { ...item, items: filterItems(item.items) } : item));
  if (Array.isArray(sidebar)) return filterItems(sidebar);
  return Object.fromEntries(
    Object.entries(sidebar).map(([key, value]) => [key, filterItems(value)]),
  );
}

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
    hostname: SITE_URL,
  },
  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/favicon.svg' }],
    ['meta', { property: 'og:image', content: OG_IMAGE_URL }],
    ['meta', { property: 'og:image:width', content: '2400' }],
    ['meta', { property: 'og:image:height', content: '1260' }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:image', content: OG_IMAGE_URL }],
  ],
  vite: {
    plugins: [
      // Substitute {{VERSION}} with the current release version in every
      // markdown source file. Runs before VitePress's own loader and before
      // llmstxt, so both the rendered HTML and the raw .md files emitted
      // for the copy-as-markdown button / llms-full.txt see the real
      // version. Single source of truth lives in constants.ts so /release
      // bumps one file instead of ~13.
      {
        name: 'version-substitution',
        enforce: 'pre',
        transform(code, id) {
          if (id.endsWith('.md')) {
            return code.replace(/\{\{VERSION\}\}/g, VERSION);
          }
        },
      },
      llmstxt({ sidebar: stripExternalSidebarLinks }),
    ],
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
      { text: 'Concepts', link: '/concepts/architecture' },
      { text: 'Features', link: '/guides/data-sources' },
      { text: 'Guides', link: '/installation/docker' },
    ],

    sidebar: {
      '/': [
        {
          text: 'Start',
          items: [
            { text: 'Introduction', link: '/start/introduction' },
            { text: 'Quickstart', link: '/start/quickstart' },
            { text: 'Demo Schema', link: '/reference/demo-schema' },
          ],
        },
        {
          text: 'Concepts',
          items: [
            { text: 'Architecture', link: '/concepts/architecture' },
            { text: 'Policy Model', link: '/concepts/policy-model' },
            {
              text: 'Security Overview',
              link: '/concepts/security-overview',
            },
            { text: 'Threat Model', link: '/concepts/threat-model' },
            {
              text: 'Known Limitations',
              link: '/operations/known-limitations',
            },
            { text: 'Glossary', link: '/reference/glossary' },
          ],
        },
        {
          text: 'Features',
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
                {
                  text: 'Policy Types',
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
                  text: 'Template Expressions',
                  link: '/reference/template-expressions',
                },
                {
                  text: 'Decision Functions',
                  link: '/guides/decision-functions',
                },
              ],
            },
            {
              text: 'Audit & Debugging',
              link: '/guides/audit-debugging',
            },
          ],
        },
        {
          text: 'Guides',
          items: [
            {
              text: 'Deployment',
              collapsed: false,
              items: [
                { text: 'Docker', link: '/installation/docker' },
                { text: 'Fly.io', link: '/installation/fly' },
                { text: 'From Source', link: '/installation/from-source' },
                { text: 'Configuration', link: '/reference/configuration' },
              ],
            },
            { text: 'Upgrading', link: '/operations/upgrading' },
            { text: 'Backups & Recovery', link: '/operations/backups' },
            {
              text: 'Troubleshooting',
              link: '/operations/troubleshooting',
            },
            {
              text: 'Rename Safety',
              link: '/operations/rename-safety',
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
          text: 'About',
          items: [
            { text: 'Changelog', link: CHANGELOG_URL },
            { text: 'Roadmap', link: '/about/roadmap' },
            { text: 'License', link: LICENSE_URL },
            { text: 'Report an Issue', link: '/about/report-an-issue' },
          ],
        },
      ],
    },

    socialLinks: [
      {
        icon: 'github',
        link: GITHUB_URL,
      },
    ],

    editLink: {
      pattern: EDIT_PAGE_URL,
      text: 'Edit this page on GitHub',
    },

    search: {
      provider: 'local',
    },

    footer: {
      message:
        'Released under the <a href="https://github.com/getbetweenrows/betweenrows/blob/main/LICENSE">Elastic License v2</a>.',
      copyright: 'Copyright © 2026 BetweenRows',
    },
  },
});
