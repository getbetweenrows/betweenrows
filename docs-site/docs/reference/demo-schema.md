---
title: Demo Schema
description: The canonical demo_ecommerce schema used in all guides — tables, columns, sample data, personas, and setup instructions.
---

# Demo Schema

All guide tutorials use the `demo_ecommerce` schema — a multi-tenant e-commerce database with three tenants, realistic sample data, and pre-configured policies. This page documents the full schema so you can reproduce every tutorial example.

## Setup

The demo stack lives at `scripts/demo_ecommerce/` in the repo root. Two commands bring up everything:

```sh
cd scripts/demo_ecommerce
docker compose -f compose.demo.yaml up -d
./setup.sh
```

`setup.sh` runs three phases: (1) applies the schema and seeds data in the upstream PostgreSQL, (2) creates the BetweenRows admin bootstrap (tenant attribute, users, datasource, catalog), (3) creates policies and assigns them.

To reset cleanly (Phase 1 is not idempotent):

```sh
docker compose -f compose.demo.yaml down -v
docker compose -f compose.demo.yaml up -d
./setup.sh
```

See `scripts/demo_ecommerce/README.md` for env var overrides and advanced usage.

## Tenants

Three tenants, each with their own data in every table:

| Tenant | Description |
|---|---|
| `acme` | User: `alice` |
| `globex` | User: `bob` |
| `stark` | User: `charlie` |

## Users

| Username | Password | Tenant attribute | Role |
|---|---|---|---|
| `admin` | `changeme` | — | Admin (UI + API access only, no data plane) |
| `alice` | `Demo1234!` | `acme` | Data plane user |
| `bob` | `Demo1234!` | `globex` | Data plane user |
| `charlie` | `Demo1234!` | `stark` | Data plane user |

Connect through the proxy:

```sh
psql 'postgresql://alice:Demo1234!@127.0.0.1:5434/demo_ecommerce'
```

## Tables

### organizations

| Column | Type | Notes |
|---|---|---|
| `name` | TEXT (PK) | Tenant name: `acme`, `globex`, `stark` |
| `created_at` | TIMESTAMPTZ | |

### customers

| Column | Type | Notes |
|---|---|---|
| `id` | UUID (PK) | |
| `org` | TEXT (FK → organizations) | Tenant identifier — filtered by `tenant-isolation` policy |
| `first_name` | TEXT | |
| `last_name` | TEXT | |
| `email` | TEXT | |
| `phone` | TEXT | |
| `ssn` | TEXT | Masked by `mask-ssn-partial` policy |
| `credit_card` | TEXT | Denied by `hide-credit-card` policy |
| `created_at` | TIMESTAMPTZ | |

10 customers per tenant (30 total).

### products

| Column | Type | Notes |
|---|---|---|
| `id` | UUID (PK) | |
| `org` | TEXT (FK → organizations) | |
| `name` | TEXT | |
| `description` | TEXT | |
| `price` | NUMERIC(10,2) | Public price |
| `cost_price` | NUMERIC(10,2) | Denied by `hide-product-financials` policy |
| `margin` | NUMERIC(5,4) | Denied by `hide-product-financials` policy |
| `created_at` | TIMESTAMPTZ | |

20 products per tenant (60 total).

### orders

| Column | Type | Notes |
|---|---|---|
| `id` | UUID (PK) | |
| `org` | TEXT (FK → organizations) | |
| `customer_id` | UUID (FK → customers) | |
| `status` | TEXT | `pending`, `processing`, `shipped`, `delivered`, `cancelled` |
| `total_amount` | NUMERIC(10,2) | |
| `created_at` | TIMESTAMPTZ | |
| `updated_at` | TIMESTAMPTZ | |

~34 orders per tenant (~102 total).

### order_items

| Column | Type | Notes |
|---|---|---|
| `id` | UUID (PK) | |
| `order_id` | UUID (FK → orders) | No `org` column — see note below |
| `product_id` | UUID (FK → products) | |
| `quantity` | INTEGER | |
| `unit_price` | NUMERIC(10,2) | |
| `created_at` | TIMESTAMPTZ | |

### payments

| Column | Type | Notes |
|---|---|---|
| `id` | UUID (PK) | |
| `order_id` | UUID (FK → orders) | No `org` column — see note below |
| `amount` | NUMERIC(10,2) | |
| `payment_method` | TEXT | `credit_card`, `bank_transfer`, `paypal` |
| `status` | TEXT | |
| `processed_at` | TIMESTAMPTZ | |
| `created_at` | TIMESTAMPTZ | |

### support_tickets

| Column | Type | Notes |
|---|---|---|
| `id` | UUID (PK) | |
| `org` | TEXT (FK → organizations) | |
| `customer_id` | UUID (FK → customers) | |
| `subject` | TEXT | |
| `status` | TEXT | `open`, `in_progress`, `resolved`, `closed` |
| `created_at` | TIMESTAMPTZ | |

~50 tickets per tenant.

::: info Tables without an org column
`order_items` and `payments` reference `orders` via `order_id` but don't carry their own `org` column. In `policy_required` mode, tables without a matching policy return zero rows by default (safe default). The guide tutorials only query `customers`, `orders`, `products`, and `support_tickets`.
:::

## Pre-configured policies

| Policy | Type | Effect |
|---|---|---|
| `tenant-isolation` | `row_filter` | `org = {user.tenant}` on customers, orders, products, support_tickets |
| `mask-ssn-partial` | `column_mask` | `customers.ssn` → last-4 masking |
| `mask-ssn-full` | `column_mask` | `customers.ssn` → `[RESTRICTED]` (created but **unassigned** by default) |
| `hide-credit-card` | `column_deny` | Removes `customers.credit_card` from all queries |
| `hide-product-financials` | `column_deny` | Removes `products.cost_price` and `products.margin` |
| `admin-full-access` | `column_allow` | Full access on all tables (created but **unassigned** — assign per user in the admin UI) |

`tenant-isolation`, `mask-ssn-partial`, `hide-credit-card`, and `hide-product-financials` are assigned with `scope=all` to the `demo_ecommerce` datasource by `setup.sh`.

## Quick verification

After setup, each user sees only their tenant's rows:

```sql
-- As alice (acme tenant):
SELECT DISTINCT org FROM orders;
-- → acme

-- SSNs are masked:
SELECT first_name, ssn FROM customers LIMIT 3;
-- → Alice, ***-**-1234

-- Credit cards are hidden:
SELECT credit_card FROM customers;
-- → ERROR: column "credit_card" does not exist
```
