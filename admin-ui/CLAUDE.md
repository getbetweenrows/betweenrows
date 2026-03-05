# Admin UI — React Frontend

## Key Versions
React 19, Vite 6, Tailwind 4, TanStack Query 5, react-router-dom 7, Vitest 3, @testing-library/react 16

## Key Files
- `vite.config.ts` — proxies `/api` → `http://localhost:5435`
- `src/api/client.ts` — axios instance with JWT interceptor and 401 redirect
- `src/auth/AuthContext.tsx` — `AuthProvider`, `useAuth`, localStorage-backed token/user
- `src/components/CatalogDiscoveryWizard.tsx` — 4-step discovery wizard (schemas → tables → columns → save)
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
