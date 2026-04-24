# Admin UI Design Standard

This is the reference for building new pages in `admin-ui/`. Read the template decision first; most pages pick one and follow the rules under it.

Admin UI is **desktop-first, min-width ~1024px**. No responsive collapse for the sidebar pattern.

---

## Pick a template

```
Detail / edit a resource ─────────────────────────────────────────► T1  (sidebar)

Single form (create) ─────────────────────────────────────────────► T2  (one-card form)

Tabular resource listing ─────────────────────────────────────────► T3  (list)

Log / audit viewer ───────────────────────────────────────────────► T4  (filter + dense table)
```

**On the section count.** Don't over-split. If two sections feel like they're answering the same user question (e.g., "who has access via this role?" → direct members + inheritance, "what does this role let them into?" → datasource grants + policy assignments), combine them in one `SectionPane` with stacked cards. The right section count is usually 2–5; 6+ is a smell worth revisiting. A 2-section sidebar (e.g., Details + Activity) is fine — visual consistency with the rest of the admin beats a slightly-empty nav.

### T1 — Multi-section Detail (sidebar)
Header + sticky (optionally grouped) `SecondaryNav` + mounted-but-hidden `SectionPane`s + per-section save + `DangerZone` at the bottom of the primary section. URL state via `?section=<id>`; optional `?focus=<target>` for cross-page deep links.

Use this for every edit page, regardless of size. A 2-section sidebar (e.g., Details + Activity) is still T1 — keep the visual shell consistent.

Reference implementation: `src/pages/DataSourceEditPage.tsx`.

### T2 — Single Form
Header with breadcrumb + one card + Cancel/Submit. No back-button. Fits create pages where there's no resource yet to hang sections off of.

### T3 — List
Full-width table + search/filter + primary "New" CTA. No breadcrumb — the primary sidebar provides context. No destructive row actions: delete lives on the detail page's danger zone. Any prompts use `ConfirmDialog`, never native `confirm()`.

### T4 — Audit/Log
Filter form with explicit Apply + dense table + expandable rows + pagination.

---

## Rules

### Header
- Breadcrumb + title + metadata line (`·` separators, muted gray) + status indicator at far right of title line.
- Breadcrumb replaces `← Back` on T1 and T2. T3 lists and T4 audit views have no breadcrumb.
- Status: `StatusDot` for binary (Active/Inactive, Enabled/Disabled), `StatusChip` for categorical (policy type). **Always paired with an adjacent text label** — color alone fails for color-blind users.
- No timestamps in the metadata line — they belong in the Activity section.
- Title is plain text, not monospace.

### Secondary navigation (T1)
- Sticky `w-56 shrink-0` sidebar, `sticky top-6 self-start`.
- Grouped with small-caps labels (`text-[10px] font-semibold text-gray-400 uppercase tracking-wider`) when ≥2 groups; flat list otherwise.
- `<nav aria-label>` + `<button>` with `aria-current="page"`. Do **not** use `role="tab"` / `role="tablist"` — this is site nav, not a widget. Default Tab order only; no arrow-key handling.

### URL state (T1)
- `?section=<id>`, `replace: true` on internal clicks, `push` (default) on inbound deep links from other pages. Validated against a known-id set; falls back to the first section.
- `?focus=<target>` for cross-page deep links into a row: wait for the relevant query, scroll-into-view + 1.5s highlight, or pre-fill an Add form if the target doesn't exist. Strip the param after first use (ref-tracked so refresh doesn't re-fire).

### Sections (T1)
- All sections mount eagerly; toggle visibility with Tailwind `hidden` + `aria-hidden={!active}`. Preserves in-progress drafts when users switch sections to cross-reference.
- Max-width scale: `max-w-2xl` for forms, `max-w-5xl` for tables/lists, `max-w-none` for code editors. Outer container stays full-width; each section sets its own.

### Save
- Form section → explicit Save button (batch commit).
- Row section → per-row action (immediate: add, delete, toggle).
- Read-only section → no save.
- **Update:** `toast.success('Saved')` and stay on the page with scroll preserved. Never auto-navigate.
- **Create:** on success, navigate to the new resource's edit page (`/{resource}/{newId}/edit`). The list has no context.
- **Optimistic-concurrency 409:** show a non-dismissing banner at the top of the form: "This {resource} was modified by someone else. Reload to see the latest changes." Do not auto-reload — it discards drafts.

### Errors
- Server errors → red banner at the top of the form.
- Field-level errors → inline under the field.
- Success confirmation → toast.
- Toasts are for confirmations, never errors.

### Destructive actions
- `DangerZone` card at the bottom of the primary section (Details, Profile, etc.): red border + header, stacked `<DangerRow>`s.
- **Delete (irreversible):** `ConfirmDeleteModal` with typed-name exact-match input. Offer a soft-delete escape when one exists: "Deactivate instead" when the resource has `is_active`, "Disable instead" when it has `is_enabled`.
- **Rename with external side effects:** warning-only modal, no typed-name.
- List pages do not carry destructive row actions — delete is exclusively a detail-page operation. Use `ConfirmDialog` for any remaining inline confirmations (not native `confirm()`).

### Modals
- One `ModalShell` primitive. Closes on Esc and click-outside. Traps focus while open and restores focus on close.

### Components
- Sections colocate with their page until a second page needs them. Named exports within the same file (e.g., `RelationshipsSection`, `ColumnAnchorsSection` in `RelationshipsPanel.tsx`). Extract to `components/` only on the 2nd consumer.
- Share data via TanStack Query cache, not custom stateful hooks. Thin `useQuery` wrappers that own queryKey + fetcher are fine.

### Testing
- New primitives ship with unit tests written first (TDD, per `CLAUDE.md`).
- Pages using T1 get tests for URL-driven section selection (refresh preserves section), deep-link handling, delete flow through the new modal, and save-shows-toast-stays-on-page.
- `data-testid` on the SecondaryNav root and each section pane — labels are not `htmlFor`-linked here (see "Critical Test Gotchas" in `CLAUDE.md`).
- Inactive `SectionPane`s have `aria-hidden={true}`, so `getByRole` queries don't see their content. Tests that assert content inside a non-default section must click the sidebar button first.

---

## Shared primitives

All primitives ship with unit tests in the same folder.

| Primitive | Path |
|---|---|
| `PageHeader` | `src/components/layout/PageHeader.tsx` |
| `SecondaryNav` + `useSectionParam` | `src/components/layout/SecondaryNav.tsx`, `src/hooks/useSectionParam.ts` |
| `SectionPane` | `src/components/layout/SectionPane.tsx` |
| `DangerZone` + `DangerRow` | `src/components/DangerZone.tsx` |
| `ModalShell` (focus trap + Esc + click-outside) | `src/components/ModalShell.tsx` |
| `StatusDot`, `StatusChip` | `src/components/Status.tsx` |
| `ConfirmDeleteModal` | `src/components/ConfirmDeleteModal.tsx` — typed-name + optional soft-delete escape |
| `ConfirmDialog` | `src/components/ConfirmDialog.tsx` — plain Yes/No |

**Not extracted yet — add only when a real second consumer appears:** `useFocusParam` (only `RelationshipsPanel` uses it), `EmptyState` / `LoadingRow` / `ErrorState` (no demonstrated inconsistency), `PageLayout` (T1/T2/T3 are genuinely different shapes), section-registry abstractions.
