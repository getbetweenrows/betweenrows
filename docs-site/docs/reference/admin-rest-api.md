---
title: Admin REST API
description: Reference for the BetweenRows admin REST API — authentication, error format, and common request/response examples for scripting.
---

# Admin REST API

Everything the admin UI does is backed by the REST API at `http://localhost:5435/api/v1/`. The UI is just a client. Anything you can do in the UI, you can do via the API — useful for scripting, CI/CD, and automation.

::: info
A complete OpenAPI schema (every endpoint, request and response body, error codes, query parameters) is a post-launch deliverable. Until it ships, this page covers authentication, error format, and a handful of worked examples that double as payload documentation. For any other action, the fastest source of truth is the [admin UI network tab](https://developer.mozilla.org/en-US/docs/Tools/Network_Monitor) — perform the action in the UI and inspect the request.
:::

## Base URL

```
http://localhost:5435/api/v1
```

Or your deployment's admin plane URL. All endpoints require a JSON `Content-Type` on the request, return JSON on the response, and require an `Authorization: Bearer <token>` header (except `POST /auth/login`).

## Authentication

### `POST /auth/login`

```sh
curl -X POST http://localhost:5435/api/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"changeme"}'
```

Response:

```json
{ "token": "eyJhbGc...", "expires_at": "2026-04-11T10:00:00Z" }
```

Use the token in subsequent calls:

```sh
export TOKEN="eyJhbGc..."
curl -H "Authorization: Bearer $TOKEN" http://localhost:5435/api/v1/users
```

The token lifetime is controlled by `BR_ADMIN_JWT_EXPIRY_HOURS` (default 24h).

## Endpoint surface

A full OpenAPI specification is a post-launch deliverable. Until it ships, the fastest way to discover the exact request and response shapes for any admin action is to open the admin UI, perform the action, and inspect the request in your browser's network tab. Every button in the UI maps 1:1 to a single REST call under `/api/v1/`.

The concrete examples in the next section cover the most common scripting flows: creating users and policies, assigning policies to data sources, testing decision functions, and doing optimistic-concurrency updates. Audit endpoint query parameters are documented in [Audit Log Fields](/reference/audit-log-fields).

## Error format

All errors return a JSON body:

```json
{ "error": "description of what went wrong" }
```

With an appropriate HTTP status code:

- `400` — validation error (bad request shape)
- `401` — missing or expired token
- `403` — authenticated but not authorized (non-admin on admin endpoint)
- `404` — resource not found
- `409` — conflict (optimistic concurrency on `PUT /policies/{id}`; deleting an inactive role with active grants)
- `422` — unprocessable entity (policy validation, attribute value type mismatch)
- `500` — unexpected server error

## Common request/response examples

### Create a user

```sh
curl -X POST http://localhost:5435/api/v1/users \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "username": "alice",
    "password": "Demo1234!",
    "is_admin": false,
    "attributes": { "tenant": "acme", "department": "engineering" }
  }'
```

Response (`201`):
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "username": "alice",
  "is_admin": false,
  "is_active": true,
  "attributes": { "tenant": "acme", "department": "engineering" },
  "created_at": "2026-04-12T10:00:00Z",
  "updated_at": "2026-04-12T10:00:00Z"
}
```

### Create a policy

```sh
curl -X POST http://localhost:5435/api/v1/policies \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "tenant-isolation",
    "policy_type": "row_filter",
    "is_enabled": true,
    "targets": [
      { "schemas": ["*"], "tables": ["*"] }
    ],
    "definition": {
      "filter_expression": "org = {user.tenant}"
    }
  }'
```

Response (`201`):
```json
{
  "id": "...",
  "name": "tenant-isolation",
  "policy_type": "row_filter",
  "is_enabled": true,
  "targets": [...],
  "definition": { "filter_expression": "org = {user.tenant}" },
  "version": 1,
  "decision_function_id": null,
  "created_at": "...",
  "updated_at": "..."
}
```

### Assign a policy to a data source

```sh
curl -X POST http://localhost:5435/api/v1/datasources/$DS_ID/policies \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "policy_id": "...",
    "scope": "all",
    "priority": 100
  }'
```

Scope options: `"all"` (everyone), `"role"` (add `"role_id": "..."`), `"user"` (add `"user_id": "..."`).

### Test a decision function

```sh
curl -X POST http://localhost:5435/api/v1/decision-functions/test \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "decision_fn": "function evaluate(ctx, config) { return { fire: ctx.session.user.roles.includes(\"admin\") }; }",
    "decision_config": {},
    "evaluate_context": "session",
    "test_context": {
      "session": {
        "user": { "id": "...", "username": "alice", "roles": ["analyst"], "tenant": "acme" },
        "time": { "now": "2026-04-12T10:00:00Z", "hour": 10, "day_of_week": "Saturday" },
        "datasource": { "name": "demo_ecommerce", "access_mode": "policy_required" }
      }
    }
  }'
```

Response:
```json
{
  "success": true,
  "result": {
    "fire": false,
    "logs": [],
    "fuel_consumed": 12345,
    "time_us": 980,
    "error": null
  },
  "error": null
}
```

### Update a policy (optimistic concurrency)

```sh
curl -X PUT http://localhost:5435/api/v1/policies/$POLICY_ID \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "version": 1,
    "definition": {
      "filter_expression": "org = {user.tenant} AND status = '\''active'\''"
    }
  }'
```

The `version` field must match the current version. If another edit happened first, you get `409 Conflict` — re-fetch, merge, and retry.

## See also

- **[CLI](/reference/cli)** — for operations outside the HTTP surface
- **[Audit & Debugging](/guides/audit-debugging)** — the audit endpoints in practice
- **[Configuration](/reference/configuration)** — how to expose or restrict the admin plane
