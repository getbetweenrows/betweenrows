# Demo E-Commerce Schema

A demo multi-tenant e-commerce database for testing the BetweenRows permission system.

## Tables

| Table | Description |
|---|---|
| `organizations` | Three tenants: Acme Corp, Widgets Inc, Global Trade |
| `customers` | ~30 customers with SSN and credit card data |
| `products` | ~60 products with cost_price and margin |
| `orders` | ~100 orders |
| `order_items` | ~200 order items |
| `payments` | ~80 payments |

## Setup

### 1. Create the schema

```bash
psql $DATABASE_URL < schema.sql
```

### 2. Install Python dependencies

```bash
pip install -r requirements.txt
```

### 3. Seed data

```bash
DATABASE_URL=postgres://user:pass@host/dbname python seed.py
```

Or create a `.env` file:
```
DATABASE_URL=postgres://user:pass@host/dbname
```

## Using with BetweenRows

### 1. Create a datasource in the admin UI

Point it at your demo database. Set `access_mode` to `policy_required` to ensure policies are required.

### 2. Discover the catalog

Run the catalog discovery wizard to import the schema.

### 3. Import the demo policies

```bash
export TOKEN=$(curl -s -X POST http://localhost:5435/api/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"your-password"}' | jq -r .token)

curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: text/plain" \
  --data-binary @policies.yaml \
  "http://localhost:5435/api/v1/policies/import"
```

### 4. Create test users

Create proxy users and set their tenant attribute:

```bash
cargo run -p proxy -- user create \
  --username alice --password secret

cargo run -p proxy -- user create \
  --username bob --password secret
```

### 5. Assign users to the datasource and set tenant attributes

In the admin UI:
1. Create a `tenant` attribute definition (Attributes → Create, key: `tenant`, entity type: `user`, value type: `string`)
2. Go to the datasource edit page and assign Alice and Bob
3. On each user's edit page, set their `tenant` attribute: Alice → `Acme Corp`, Bob → `Widgets Inc`

### 6. Assign policies

Assign the demo policies to the datasource in the admin UI. For per-user assignments (e.g. mask-ssn-partial vs mask-ssn-full), specify the target user.

### 7. Verify policies via psql

```bash
# Connect as Alice (tenant attribute: Acme Corp)
psql "postgresql://alice:secret@localhost:5434/betweenrows"

-- Should return only Acme Corp's orders
SELECT id, status, total_amount FROM orders LIMIT 5;

-- SSN should be masked
SELECT first_name, ssn FROM customers LIMIT 3;

-- credit_card should be absent
SELECT * FROM customers LIMIT 1;

-- order_items filtered via join
SELECT * FROM order_items LIMIT 5;
```

## Policy scenarios

| Policy | Story | Effect |
|---|---|---|
| `tenant-isolation` | MT-01 | Users see only their org's rows |
| `tenant-isolation-order-items` | RE-01 | order_items filtered via orders join |
| `mask-ssn-partial` | DS-01 | `***-**-1234` format |
| `mask-ssn-full` | DS-04 | `[RESTRICTED]` |
| `hide-credit-card` | DS-10 | credit_card absent from results |
| `hide-product-financials` | DS-02 | cost_price, margin absent |
| `admin-full-access` | MT-05 | Assigned to admin users for full access |
