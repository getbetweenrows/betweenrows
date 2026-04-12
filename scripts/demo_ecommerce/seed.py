#!/usr/bin/env python3
"""Seed the demo e-commerce database for BetweenRows.

Produces the deterministic canonical state used by the docs guides and
screenshot capture. Three tenants (acme, globex, stark); per-tenant:
10 customers, 20 products, 34 orders, 1-4 items per order, payments for
shipped/delivered orders, ~5 support tickets per customer.

Re-running against an existing database WILL duplicate rows — the
`organizations` upsert is safe but customers/products/orders insert
fresh rows every time. To reset, drop and recreate the database
(or `docker compose down -v` on the demo stack).

Usage:
    pip install -r requirements.txt
    DATABASE_URL=postgresql://postgres:postgres@127.0.0.1:5432/demo_ecommerce \\
        python3 seed.py
"""

import os
import sys
import uuid
import random
import decimal
from datetime import datetime, timezone

import psycopg2
from faker import Faker
from dotenv import load_dotenv

load_dotenv()

fake = Faker()
random.seed(42)
Faker.seed(42)

DATABASE_URL = os.environ.get("DATABASE_URL")
if not DATABASE_URL:
    print("ERROR: DATABASE_URL environment variable not set", file=sys.stderr)
    sys.exit(1)

# Canonical tenants used by the docs guides. Change these only in lockstep
# with the guide prose and the policies.yaml filter expressions.
ORGS = ["acme", "globex", "stark"]

ORDER_STATUSES = ["pending", "processing", "shipped", "delivered", "cancelled"]
PAYMENT_METHODS = ["credit_card", "bank_transfer", "paypal"]
TICKET_STATUSES = ["open", "in_progress", "resolved", "closed"]

conn = psycopg2.connect(DATABASE_URL)
cur = conn.cursor()

print("Seeding organizations…")
for org in ORGS:
    cur.execute(
        "INSERT INTO organizations (name) VALUES (%s) "
        "ON CONFLICT (name) DO NOTHING",
        (org,),
    )
conn.commit()
print(f"  {len(ORGS)} organizations")

print("Seeding customers…")
customer_ids_by_org: dict[str, list[str]] = {org: [] for org in ORGS}
for org in ORGS:
    for _ in range(10):
        cid = str(uuid.uuid4())
        first = fake.first_name()
        last = fake.last_name()
        email = fake.email()
        phone = fake.phone_number()[:20]
        ssn = fake.ssn()
        cc = fake.credit_card_number()
        cur.execute(
            """
            INSERT INTO customers (id, org, first_name, last_name, email, phone, ssn, credit_card)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s) RETURNING id
            """,
            (cid, org, first, last, email, phone, ssn, cc),
        )
        customer_ids_by_org[org].append(cur.fetchone()[0])
conn.commit()
total_customers = sum(len(v) for v in customer_ids_by_org.values())
print(f"  {total_customers} customers")

print("Seeding products…")
product_ids_by_org: dict[str, list[str]] = {org: [] for org in ORGS}
for org in ORGS:
    for _ in range(20):
        pid = str(uuid.uuid4())
        price = decimal.Decimal(str(round(random.uniform(9.99, 499.99), 2)))
        cost = price * decimal.Decimal("0.6")
        margin = (price - cost) / price
        cur.execute(
            """
            INSERT INTO products (id, org, name, description, price, cost_price, margin)
            VALUES (%s, %s, %s, %s, %s, %s, %s) RETURNING id
            """,
            (
                pid,
                org,
                fake.catch_phrase(),
                fake.text(max_nb_chars=100),
                price,
                round(cost, 2),
                round(margin, 4),
            ),
        )
        product_ids_by_org[org].append(cur.fetchone()[0])
conn.commit()
total_products = sum(len(v) for v in product_ids_by_org.values())
print(f"  {total_products} products")

print("Seeding orders, order items, and payments…")
order_count = 0
item_count = 0
payment_count = 0
for org in ORGS:
    customers = customer_ids_by_org[org]
    products = product_ids_by_org[org]
    for _ in range(34):
        order_id = str(uuid.uuid4())
        customer_id = random.choice(customers)
        status = random.choice(ORDER_STATUSES)
        cur.execute(
            """
            INSERT INTO orders (id, org, customer_id, status)
            VALUES (%s, %s, %s, %s) RETURNING id
            """,
            (order_id, org, customer_id, status),
        )
        order_id = cur.fetchone()[0]
        order_count += 1

        total = decimal.Decimal("0")
        for _ in range(random.randint(1, 4)):
            product_id = random.choice(products)
            qty = random.randint(1, 5)
            cur.execute("SELECT price FROM products WHERE id = %s", (str(product_id),))
            row = cur.fetchone()
            unit_price = row[0] if row else decimal.Decimal("9.99")
            cur.execute(
                """
                INSERT INTO order_items (id, order_id, product_id, quantity, unit_price)
                VALUES (%s, %s, %s, %s, %s)
                """,
                (str(uuid.uuid4()), order_id, product_id, qty, unit_price),
            )
            total += unit_price * qty
            item_count += 1

        cur.execute("UPDATE orders SET total_amount = %s WHERE id = %s", (total, order_id))

        if status in ("shipped", "delivered"):
            cur.execute(
                """
                INSERT INTO payments (id, order_id, amount, payment_method, status, processed_at)
                VALUES (%s, %s, %s, %s, 'completed', %s)
                """,
                (
                    str(uuid.uuid4()),
                    order_id,
                    total,
                    random.choice(PAYMENT_METHODS),
                    datetime.now(timezone.utc),
                ),
            )
            payment_count += 1

conn.commit()
print(f"  {order_count} orders, {item_count} order items, {payment_count} payments")

print("Seeding support tickets…")
ticket_count = 0
for org in ORGS:
    for customer_id in customer_ids_by_org[org]:
        for _ in range(5):
            cur.execute(
                """
                INSERT INTO support_tickets (id, org, customer_id, subject, status)
                VALUES (%s, %s, %s, %s, %s)
                """,
                (
                    str(uuid.uuid4()),
                    org,
                    customer_id,
                    fake.sentence(nb_words=6),
                    random.choice(TICKET_STATUSES),
                ),
            )
            ticket_count += 1
conn.commit()
print(f"  {ticket_count} support tickets")

cur.close()
conn.close()
print("Done.")
