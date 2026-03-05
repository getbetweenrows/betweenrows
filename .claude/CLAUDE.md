# Project Instructions

## Use Context7 by Default
Always use context7 when I need code generation, setup or configuration steps, or library/API documentation. Automatically use the Context7 MCP tools to resolve library id and get library docs without me having to explicitly ask.

## Project Overview
QueryProxy is a Rust PostgreSQL wire protocol proxy with a React admin UI. See `README.md` for full details.

App-level instructions:
- Rust proxy → `proxy/CLAUDE.md`
- React admin UI → `admin-ui/CLAUDE.md`

## Repo Structure
```
proxy/          Rust proxy server (pgwire, DataFusion, axum)
admin-ui/       React admin frontend (Vite, TanStack Query, Tailwind)
migration/      SeaORM migrations
.githooks/      pre-commit: cargo fmt, clippy, admin-ui tests
.github/workflows/cicd.yml   CI test → Docker publish → Fly deploy
```

## Pre-commit Hook
`.githooks/pre-commit` runs `cargo fmt --check`, `cargo clippy`, and `admin-ui` tests. Enable once per clone:
```bash
git config core.hooksPath .githooks
```

## CI/CD
Push to `main` → GitHub Actions runs Rust tests + admin-ui tests → builds Docker image → deploys to Fly.io.
