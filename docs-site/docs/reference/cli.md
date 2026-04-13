---
title: CLI
description: Command-line interface for the BetweenRows proxy binary — create users, rescue admin access, run from source.
---

# CLI

The `proxy` binary ships with a minimal CLI for bootstrap operations — primarily creating users and rescuing locked-out admins.

::: warning Not the final CLI
This is a bootstrap helper built into the proxy binary, not a standalone CLI tool. We plan to build a dedicated `betweenrows` CLI that supports **Policy as Code** — declarative YAML/JSON definitions for policies, data source configurations, catalog selections, roles, and attribute definitions, managed through version control and applied via CI/CD. Think of it like Terraform or Pulumi, but for your BetweenRows access layer. User management may or may not be included (still under consideration — credentials have different lifecycle requirements than policy definitions).

When the dedicated CLI ships, these bootstrap subcommands may move or change. For automation today, prefer the [Admin REST API](/reference/admin-rest-api) — it covers everything the UI can do and is the most complete automation surface today.
:::

## Running the CLI

### Docker

```sh
docker exec -it <container> proxy <subcommand> [args]
```

### From source

```sh
cargo run -p proxy -- <subcommand> [args]
```

### Release binary

```sh
./proxy <subcommand> [args]
```

## Subcommands

### `user create`

Create a new user without logging into the admin UI.

```sh
proxy user create --username alice --password secret
```

| Flag | Description |
|---|---|
| `--username <name>` | Required. Must match `[a-zA-Z0-9_.-]`, 3–50 characters, start with a letter. |
| `--password <password>` | Required. Stored as an Argon2id hash. |
| `--admin` | Optional. Creates the user with `is_admin: true`. Use this to create a rescue admin when you're locked out of the UI. |

::: warning Password complexity is not enforced from the CLI
The `proxy user create` command stores whatever password you pass without running the admin API's complexity check (8+ characters, mixed case, digit, and special character). A user created through the CLI with a weak password will later fail to update their own password through the admin UI, because the UI path enforces complexity on edit. This is a known limitation of the interim `proxy user …` binary — the planned `betweenrows` CLI (below) will align with the admin API rules.
:::

The password is read from the command line for scripting convenience. **Be careful with shell history** — the password will appear in `~/.bash_history` or `~/.zsh_history` if entered directly. Safer alternatives:

```sh
# From a secrets file (Docker Swarm, Kubernetes)
proxy user create --username alice --password "$(cat /run/secrets/alice-password)"

# From an environment variable
proxy user create --username alice --password "$ALICE_PASSWORD"

# Generate a random password
PASSWORD=$(openssl rand -base64 24)
proxy user create --username alice --password "$PASSWORD"
echo "Alice's password: $PASSWORD"  # display once, then it's only in the Argon2id hash
```

::: tip
After creating the user, clear the password from your shell history:
```sh
history -d $(history 1 | awk '{print $1}')  # bash
# or
fc -W  # zsh — writes history, removing the last command
```
:::


### Rescue admin example

If you've forgotten the admin password and have no other admin accounts:

```sh
# From the host
docker exec -it betweenrows proxy user create \
  --username rescue \
  --password "$(openssl rand -base64 24)" \
  --admin

# Or, from inside the container:
docker exec -it betweenrows bash
proxy user create --username rescue --password "$(openssl rand -base64 24)" --admin
```

Log in as `rescue`, then use the UI to reset the original admin password (or delete the rescue account after fixing things).

::: info
A forgot/reset password feature is on the [roadmap](/about/roadmap). Until then, the CLI rescue path is the only way to recover from a lost admin password.
:::

## Running the proxy server

When the binary is invoked without a subcommand, it runs the proxy server:

```sh
proxy
```

Configuration comes from environment variables — see the [Configuration reference](/reference/configuration).

## Future: dedicated `betweenrows` CLI

A standalone CLI is planned to support **Policy as Code** workflows. The goal is to let you define your entire BetweenRows configuration in version-controlled files and apply it declaratively — like Terraform for your access layer.

Planned scope:

- **Policies** — define, validate, diff, and apply policies from YAML/JSON
- **Data sources** — configure connection details and catalog selections
- **Roles & inheritance** — define role hierarchies and membership
- **Attribute definitions** — schema-first attribute management
- **Catalog sync** — trigger discovery drift reconciliation from CI

User management is under consideration — credentials have a different lifecycle than declarative config (rotation, password complexity enforcement, MFA in the future), so it may stay API/UI-only.

Until the dedicated CLI ships, use the [Admin REST API](/reference/admin-rest-api) for automation.

## See also

- **[Install from source](/installation/from-source)** — build the binary yourself
- **[Admin REST API](/reference/admin-rest-api)** — scriptable automation
- **[Troubleshooting](/operations/troubleshooting)** — common issues
