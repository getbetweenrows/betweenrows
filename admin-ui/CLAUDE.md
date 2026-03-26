# Admin UI — React Frontend

## Key Versions
React 19, Vite 6, Tailwind 4, TanStack Query 5, react-router-dom 7, Vitest 3, @testing-library/react 16

## Key Files
- `vite.config.ts` — proxies `/api` → `http://localhost:5435`
- `src/api/client.ts` — axios instance with JWT interceptor and 401 redirect
- `src/api/roles.ts` — Role CRUD, member management, inheritance, datasource role access
- `src/api/adminAudit.ts` — Admin audit log queries
- `src/auth/AuthContext.tsx` — `AuthProvider`, `useAuth`, localStorage-backed token/user
- `src/components/CatalogDiscoveryWizard.tsx` — 4-step discovery wizard (schemas → tables → columns → save)
- `src/components/RoleForm.tsx` — Reusable role name + description form
- `src/components/RoleMemberPanel.tsx` — Effective member list (direct + inherited with source badges), add/remove for direct members
- `src/components/RoleInheritancePanel.tsx` — Parent/child role management with cycle detection feedback
- `src/components/RoleAccessPanel.tsx` — Checkbox-based role access panel for datasource edit page
- `src/components/AuditTimeline.tsx` — Reusable admin audit timeline (used on role/user/policy/datasource detail pages)
- `src/utils/auditBadge.ts` — Shared `actionBadgeClass()` for audit action badge styling (used by AuditTimeline + AdminAuditPage)
- `src/components/PolicyAssignmentPanel.tsx` — Three components: `PolicyAssignmentsReadonly`, `PolicyAssignmentEditPanel` (with scope selector: all/user/role), `DatasourceAssignmentsReadonly`
- `src/pages/RolesListPage.tsx` — Paginated list with search, member counts, active/inactive badges
- `src/pages/RoleCreatePage.tsx` — Create form
- `src/pages/RoleEditPage.tsx` — Tabbed view (Details, Members, Inheritance, Data Sources, Policies, Activity)
- `src/pages/AdminAuditPage.tsx` — Centralized admin audit log with filters (resource type, actor, date range)
- `src/types/policy.ts` — TypeScript interfaces for policies, assignments (`PolicyType`, `AssignmentScope`, `TargetEntry`)
- `src/types/role.ts` — TypeScript interfaces for roles, members, audit entries
- `src/types/decisionFunction.ts` — TypeScript interfaces for decision functions (`DecisionFunction`, `DecisionFunctionCreate`, `DecisionFunctionUpdate`)
- `src/api/decisionFunctions.ts` — API client functions for decision function CRUD (`listDecisionFunctions`, `getDecisionFunction`, `createDecisionFunction`, `updateDecisionFunction`, `deleteDecisionFunction`)
- `src/test/test-utils.tsx` — `renderWithProviders` (QueryClient + AuthProvider + MemoryRouter)
- `src/test/factories.ts` — `makeUser`, `makeDataSource`, `makeDataSourceType`, `makeDiscoveredSchema/Table/Column`

## Running Tests
```bash
npm run test:run   # single run (CI)
npm test           # watch mode
```

## Critical Test Gotchas

### `useParams` requires a wrapping `<Route>`
Pages that call `useParams()` must be rendered inside `<Routes><Route path="/foo/:id" element={<Page />} /></Routes>` with a matching `routerEntries` option. Without it, `useParams()` returns `{}`, the query is disabled (`enabled: false`), and `isLoading` is `false` in TanStack Query v5 — the component renders "not found" instead of loading.

```tsx
function renderEditPage() {
  return renderWithProviders(
    <Routes><Route path="/users/:id/edit" element={<UserEditPage />} /></Routes>,
    { authenticated: true, routerEntries: ['/users/u-1/edit'] },
  )
}
```

### TanStack Query v5 — mutation mock assertions
`mutationFn` is called as `mutationFn(variables, { client, meta, mutationKey })` — two args. `toHaveBeenCalledWith('id')` fails. Use:
```ts
expect(mockFn).toHaveBeenCalled()
expect(mockFn.mock.calls[0][0]).toBe('id')
```

### HTML5 required validation blocks `userEvent.click` on submit
Dynamic form fields rendered after type selection have `required` attributes. Use `fireEvent.submit` to bypass constraint validation:
```ts
fireEvent.submit(container.querySelector('form')!)
```

### No `htmlFor`/`id` label associations
Components use `<label>Text</label><input />` as siblings, not linked pairs. Use:
- `screen.getByRole('textbox')` or `getAllByRole('textbox')[n]` for text inputs
- `container.querySelector('input[type="password"]')` for password inputs
- `screen.getByDisplayValue('...')` for inputs with known default values

### CSS `uppercase` is visual-only
`className="uppercase"` does not change DOM `textContent`. Search for lowercase `'public'`, not `'PUBLIC'`.

### Text split by HTML tags
Rich text like `<strong>1</strong> schema` cannot be matched by `getByText(/1.*schema/i)`. Use:
```ts
expect(document.body.textContent).toMatch(/1.*schema/i)
```
