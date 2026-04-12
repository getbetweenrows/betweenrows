# Demo e-commerce

The canonical BetweenRows demo. A deterministic seed of a multi-tenant
e-commerce database plus a matching set of policies, used by the public
docs guides and the screenshot capture workflow. Also handy for ad-hoc
local demos.

## What the demo contains

Three tenants (`acme`, `globex`, `stark`), each with:

| Table | Count per tenant | Notes |
|---|---|---|
| `customers` | 10 | with ssn, credit_card (both redacted by policies) |
| `products` | 20 | with cost_price, margin (hidden by policy DS-02) |
| `orders` | 34 | ~1–4 items per order |
| `order_items` | ~3× orders | filtered via join on `orders` |
| `payments` | ~shipped+delivered orders | filtered via join on `orders` |
| `support_tickets` | ~5× customers | filtered by `org` |

All rows carry an `org TEXT` column holding the tenant name
(`acme`/`globex`/`stark`). Row filters use
`org = {user.tenant}` against a string user attribute.

## One-shot setup

Run the entire demo stack + bootstrap in two commands:

```sh
cd scripts/demo_ecommerce

# 1. Bring up proxy + upstream postgres
docker compose -f compose.demo.yaml up -d

# 2. Install Python deps (first run only)
pip install -r requirements.txt

# 3. Apply schema, seed data, and configure BR admin
./setup.sh
```

The script is idempotent for the BR admin bootstrap (phase 2 and 3 —
users, policies, assignments) but **not** for the upstream seed
(phase 1). Re-running `setup.sh` against an existing database will
duplicate customers, orders, etc. To reset cleanly:

```sh
docker compose -f compose.demo.yaml down -v
docker compose -f compose.demo.yaml up -d
./setup.sh
```

## Env var overrides

| Var | Default | Purpose |
|---|---|---|
| `BR_HOST` | `http://127.0.0.1:5435` | BR admin API base URL |
| `BR_ADMIN_USER` | `admin` | BR admin username |
| `BR_ADMIN_PASSWORD` | `changeme` | BR admin password |
| `UPSTREAM_DSN` | `postgresql://postgres:postgres@127.0.0.1:5432/demo_ecommerce` | DSN psql/seed.py use from the host |
| `BR_UPSTREAM_HOST` | `upstream` | Hostname the BR proxy uses to reach upstream (service name in compose network) |
| `BR_UPSTREAM_PORT` | `5432` | |
| `BR_UPSTREAM_DB` | `demo_ecommerce` | Upstream database name |
| `BR_UPSTREAM_USER` | `postgres` | |
| `BR_UPSTREAM_PASS` | `postgres` | |
| `DATASOURCE_NAME` | `demo_ecommerce` | BR datasource name users connect through the proxy with |

## Using the demo

After `setup.sh` finishes:

- **Admin UI:** <http://127.0.0.1:5435> (log in as `admin`/`changeme`)
- **Proxy:** `127.0.0.1:5434` (postgres wire protocol)

Connect through the proxy as one of the seeded users:

```sh
psql 'postgresql://alice:Demo1234!@127.0.0.1:5434/demo_ecommerce'
psql 'postgresql://bob:Demo1234!@127.0.0.1:5434/demo_ecommerce'
psql 'postgresql://charlie:Demo1234!@127.0.0.1:5434/demo_ecommerce'
```

Each user sees only their own tenant's rows:

```sql
SELECT DISTINCT org FROM orders;
-- alice   → acme
-- bob     → globex
-- charlie → stark
```

SSNs are masked via `mask-ssn-partial` (`***-**-<last 4>`), credit card
numbers are hidden (`column_deny`), and product cost/margin columns are
hidden for everyone.

## Policies

See `policies.yaml` for the full list. Summary:

| Policy | Type | Story | Effect |
|---|---|---|---|
| `tenant-isolation` | `row_filter` | MT-01 | Users see only their tenant's rows (customers/orders/products/support_tickets) |
| `mask-ssn-partial` | `column_mask` | DS-01 | `customers.ssn` → `***-**-NNNN` |
| `mask-ssn-full` | `column_mask` | DS-04 | `customers.ssn` → `[RESTRICTED]` (unassigned by default) |
| `hide-credit-card` | `column_deny` | DS-10 | `customers.credit_card` removed from all queries |
| `hide-product-financials` | `column_deny` | DS-02 | `products.cost_price`, `products.margin` removed |
| `admin-full-access` | `column_allow` | MT-05 | Full access (assign per user) |

`setup.sh` assigns `tenant-isolation`, `mask-ssn-partial`, `hide-credit-card`,
and `hide-product-financials` to `prod-db` with scope=all. The other
policies are created but unassigned — assign manually in the admin UI
to explore different scenarios.

**Note:** `payments` and `order_items` don't carry an `org` column — they
reference `orders` via `order_id`. The proxy's filter expression parser
rejects subqueries in filter expressions, so we can't write
`order_id IN (SELECT id FROM orders WHERE ...)`. In `policy_required`
mode, tables without a matching policy return zero rows by default, which
is the correct safe default here. The docs guides only query
customers/orders/products/support_tickets, so this gap doesn't show up.
