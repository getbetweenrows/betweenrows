-- Demo e-commerce schema for BetweenRows. Canonical state used by the docs
-- guides and the screenshot capture workflow.
--
-- Tenants are identified by a short string ("acme", "globex", "stark") stored
-- in every row as the `org` column — type TEXT so policy filter expressions
-- like `org = {user.tenant}` work directly against a string user attribute.
--
-- Run against a fresh `demo_ecommerce` database:
--     psql postgresql://postgres:postgres@127.0.0.1:5432/demo_ecommerce \
--         < schema.sql
-- Then run seed.py to populate.

CREATE TABLE IF NOT EXISTS organizations (
    name       TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS customers (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org         TEXT NOT NULL REFERENCES organizations(name),
    first_name  TEXT NOT NULL,
    last_name   TEXT NOT NULL,
    email       TEXT NOT NULL,
    phone       TEXT,
    ssn         TEXT,        -- masked by policy DS-01
    credit_card TEXT,        -- denied by policy DS-10
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS products (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org         TEXT NOT NULL REFERENCES organizations(name),
    name        TEXT NOT NULL,
    description TEXT,
    price       NUMERIC(10, 2) NOT NULL,
    cost_price  NUMERIC(10, 2),   -- restricted by policy DS-02
    margin      NUMERIC(5, 4),    -- restricted by policy DS-02
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS orders (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org          TEXT NOT NULL REFERENCES organizations(name),
    customer_id  UUID NOT NULL REFERENCES customers(id),
    status       TEXT NOT NULL DEFAULT 'pending',  -- pending, processing, shipped, delivered, cancelled
    total_amount NUMERIC(10, 2) NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS order_items (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id   UUID NOT NULL REFERENCES orders(id),
    product_id UUID NOT NULL REFERENCES products(id),
    quantity   INTEGER NOT NULL DEFAULT 1,
    unit_price NUMERIC(10, 2) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS payments (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id       UUID NOT NULL REFERENCES orders(id),
    amount         NUMERIC(10, 2) NOT NULL,
    payment_method TEXT NOT NULL,  -- credit_card, bank_transfer, paypal
    status         TEXT NOT NULL DEFAULT 'pending',
    processed_at   TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS support_tickets (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org         TEXT NOT NULL REFERENCES organizations(name),
    customer_id UUID NOT NULL REFERENCES customers(id),
    subject     TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'open',  -- open, in_progress, resolved, closed
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_customers_org        ON customers(org);
CREATE INDEX IF NOT EXISTS idx_products_org         ON products(org);
CREATE INDEX IF NOT EXISTS idx_orders_org           ON orders(org);
CREATE INDEX IF NOT EXISTS idx_orders_customer      ON orders(customer_id);
CREATE INDEX IF NOT EXISTS idx_order_items_order    ON order_items(order_id);
CREATE INDEX IF NOT EXISTS idx_order_items_product  ON order_items(product_id);
CREATE INDEX IF NOT EXISTS idx_payments_order       ON payments(order_id);
CREATE INDEX IF NOT EXISTS idx_support_tickets_org  ON support_tickets(org);
