---
title: Install from Source
description: Build BetweenRows from source for development, contributions, or unpackaged platforms.
---

# Install from Source

Most users should run BetweenRows from the [official Docker image](/installation/docker). Build from source if you want to contribute, run an unreleased commit, or target a platform for which there is no published binary.

## Prerequisites

- **Rust** — the latest stable toolchain (`rustup install stable`). Check `proxy/Cargo.toml` for the edition.
- **Node 22+** and **npm** — for building the admin UI.
- **System dependencies** (Linux): `cmake`, `clang`, `libssl-dev`, `pkg-config`.
- **Docker** (optional) — integration tests use `testcontainers` and require Docker to be running.

On macOS, install the system dependencies with Homebrew:

```sh
brew install cmake
```

On Debian/Ubuntu:

```sh
sudo apt-get install -y cmake clang libssl-dev pkg-config
```

## Clone and build

1. **Clone the repository.**

   ```sh
   git clone https://github.com/getbetweenrows/betweenrows.git
   cd betweenrows
   ```

2. **Enable the pre-commit hook.**

   ```sh
   git config core.hooksPath .githooks
   ```

   This runs `cargo fmt --check`, `cargo clippy`, the proxy test suite, and the admin-ui tests before every commit. Recommended for contributors.

3. **Build the admin UI.**

   ```sh
   cd admin-ui
   npm ci
   npm run build
   cd ..
   ```

   The output goes to `admin-ui/dist/` and is embedded into the Rust binary at build time.

4. **Build the proxy.**

   ```sh
   cargo build -p proxy --release
   ```

   The binary is produced at `target/release/proxy`.

5. **Run the proxy.**

   ```sh
   BR_ADMIN_USER=admin \
   BR_ADMIN_PASSWORD=changeme \
   BR_PROXY_BIND_ADDR=127.0.0.1:5434 \
   BR_ADMIN_BIND_ADDR=127.0.0.1:5435 \
   ./target/release/proxy
   ```

   Open [http://localhost:5435](http://localhost:5435) to log in.


## Development workflow

For iterative development on the proxy:

```sh
cargo run -p proxy -- --help
cargo test -p proxy
cargo clippy -p proxy -- -D warnings
cargo fmt --check
```

For the admin UI (hot reload):

```sh
cd admin-ui
npm run dev
```

The dev server proxies API calls to `http://localhost:5435`, so run the Rust proxy in a separate terminal.

## Running tests

The project has two test binaries:

```sh
# Unit tests (no Docker required)
cargo test --lib -p proxy

# Integration tests (require Docker — testcontainers-based)
cargo test --test policy_enforcement
cargo test --test protocol
```

Integration tests are skipped gracefully if Docker is unavailable.

## Contributing

See [`CONTRIBUTING.md`](https://github.com/getbetweenrows/betweenrows/blob/main/CONTRIBUTING.md) in the repo for architecture details, coding conventions, testing philosophy, and the bug fix protocol. In short: write failing tests first, then fix the code until they pass. TDD is non-optional for security-adjacent changes.

::: tip
The repository contains `proxy/CLAUDE.md`, `admin-ui/CLAUDE.md`, and a root `CLAUDE.md` with instructions for working in the codebase. Read those before opening a non-trivial PR.
:::

## Next steps

- **[Troubleshooting](/operations/troubleshooting)** — common build and runtime issues
- **[Quickstart](/start/quickstart)** — once your build is running
- **[Roadmap](/about/roadmap)** — what's planned and what's already shipped
