# Admin UI — React Frontend

## Key Versions
React 19, Vite 7, Tailwind 4, TanStack Query 5, react-router-dom 7, Vitest 4, @testing-library/react 16, CodeMirror 6 (`@uiw/react-codemirror`)

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
- `src/components/DecisionFunctionModal.tsx` — Modal for creating/editing decision functions with CodeMirror 6 editors (JS + JSON). Features: `ctx.*`/`config.*` autocomplete (activates on dot), templates (create mode), test panel with client-side pre-check → server WASM test, fire/skip/error badges, shared function warning, optimistic concurrency. Three linters: JS linter (executes function against mock context for syntax + runtime errors, highlights exact error line), JSON linter on config + test context editors (validates JSON.parse, highlights error position).
- `src/components/PolicyForm.tsx` — Policy create/edit form with funnel-framed sections (Policy → Effect → Targets → Decision Function), toggle-based decision function attachment (create new / select existing / edit / detach)
- `src/pages/RolesListPage.tsx` — Paginated list with search, member counts, active/inactive badges
- `src/pages/RoleCreatePage.tsx` — Create form
- `src/pages/RoleEditPage.tsx` — Tabbed view (Details, Members, Inheritance, Data Sources, Policies, Activity)
- `src/pages/AdminAuditPage.tsx` — Centralized admin audit log with filters (resource type, actor, date range)
- `src/types/policy.ts` — TypeScript interfaces for policies, assignments (`PolicyType`, `AssignmentScope`, `TargetEntry`)
- `src/types/role.ts` — TypeScript interfaces for roles, members, audit entries
- `src/types/decisionFunction.ts` — TypeScript interfaces (`DecisionFunctionResponse`, `DecisionFunctionSummary`, `CreateDecisionFunctionPayload`, `UpdateDecisionFunctionPayload`, `TestDecisionFnPayload`, `TestDecisionFnResponse`, `EvaluateContext`, `OnErrorBehavior`, `LogLevel`)
- `src/api/decisionFunctions.ts` — API client: `listDecisionFunctions`, `getDecisionFunction`, `createDecisionFunction`, `updateDecisionFunction`, `deleteDecisionFunction`, `testDecisionFn`
- `src/api/attributeDefinitions.ts` — API client: `listAttributeDefinitions`, `getAttributeDefinition`, `createAttributeDefinition`, `updateAttributeDefinition`, `deleteAttributeDefinition`
- `src/types/attributeDefinition.ts` — TypeScript interfaces (`AttributeDefinition`, `CreateAttributeDefinitionPayload`, `UpdateAttributeDefinitionPayload`, `ValueType`, `EntityType`)
- `src/pages/AttributeDefinitionsPage.tsx` — List attribute definitions with entity type filter, force-delete support
- `src/pages/AttributeDefinitionEditPage.tsx` — Create/edit attribute definitions (exports `AttributeDefinitionCreatePage` and `AttributeDefinitionEditPage`). Uses `AttributeDefinitionForm` component with standard card wrapper + back button layout.
- `src/components/AttributeDefinitionForm.tsx` — Reusable form for attribute definition create/edit (matches `RoleForm`/`DataSourceForm` pattern). Supports value types: string, integer, boolean, list.
- `src/components/UserAttributeEditor.tsx` — Inline editor for user attributes on UserEditPage. Loads attribute definitions to show type-appropriate inputs (text, number, boolean toggle, enum dropdown, tag/chip input for lists, multi-select checkboxes for lists with allowed values). Shows `{user.KEY}` syntax hint per attribute.
- `src/components/ExpressionEditor.tsx` — CodeMirror-based expression editor for filter/mask expressions
- `src/components/PolicyCodeView.tsx` — Read-only policy code preview
- `src/components/UserAssignmentPanel.tsx` — User-scoped policy assignment panel
- `src/components/UserForm.tsx` — Reusable user create/edit form
- `src/components/Layout.tsx` — App shell layout with sidebar navigation
- `src/pages/PoliciesListPage.tsx` — Paginated policy list
- `src/pages/PolicyCreatePage.tsx` — Policy creation page
- `src/pages/PolicyEditPage.tsx` — Policy edit page with form + assignment panel
- `src/pages/QueryAuditPage.tsx` — Query audit log viewer
- `src/test/test-utils.tsx` — `renderWithProviders` (QueryClient + AuthProvider + MemoryRouter)
- `src/test/factories.ts` — `makeUser`, `makeDataSource`, `makeDataSourceType`, `makeDiscoveredSchema/Table/Column`, `makeDecisionFunction`, `makePolicy`, `makePolicyAssignment`

## Running
```bash
npm run dev        # start dev server (proxies /api → localhost:5435)
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

### CodeMirror 6 in tests
CodeMirror 6 uses DOM APIs (Selection, Range) that jsdom doesn't support. Mock `@uiw/react-codemirror` as a simple textarea in tests:
```tsx
vi.mock('@uiw/react-codemirror', () => ({
  default: ({ value, onChange, placeholder }: { value: string; onChange?: (v: string) => void; placeholder?: string }) => (
    <textarea data-testid="codemirror" value={value} onChange={(e) => onChange?.(e.target.value)} placeholder={placeholder} />
  ),
}))
```
Use `fireEvent.change(editor, { target: { value: '...' } })` instead of `userEvent.type` for CodeMirror textareas — `userEvent.type` interprets `{` as keyboard modifier syntax.

### Text split by HTML tags
Rich text like `<strong>1</strong> schema` cannot be matched by `getByText(/1.*schema/i)`. Use:
```ts
expect(document.body.textContent).toMatch(/1.*schema/i)
```
