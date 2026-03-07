#!/usr/bin/env python3
"""
Seed demo e-commerce data for BetweenRows permission system demo.

Usage:
    pip install -r requirements.txt
    DATABASE_URL=postgres://user:pass@host/db python seed.py

Or with a .env file containing DATABASE_URL.
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

ORGANIZATIONS = [
    {"name": "Acme Corp"},
    {"name": "Widgets Inc"},
    {"name": "Global Trade"},
]

STATUSES = ["pending", "processing", "shipped", "delivered", "cancelled"]
PAYMENT_METHODS = ["credit_card", "bank_transfer", "paypal"]

conn = psycopg2.connect(DATABASE_URL)
cur = conn.cursor()

print("Seeding organizations…")
org_ids = {}
for org in ORGANIZATIONS:
    oid = str(uuid.uuid4())
    org_ids[org["name"]] = oid
    cur.execute(
        "INSERT INTO organizations (id, name) VALUES (%s, %s) ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
        (oid, org["name"]),
    )
    org_ids[org["name"]] = cur.fetchone()[0]
conn.commit()
print(f"  {len(org_ids)} organizations")

print("Seeding customers…")
customer_ids_by_org = {name: [] for name in org_ids}
for org_name, org_id in org_ids.items():
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
            INSERT INTO customers (id, organization_id, first_name, last_name, email, phone, ssn, credit_card)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s) RETURNING id
            """,
            (cid, org_id, first, last, email, phone, ssn, cc),
        )
        customer_ids_by_org[org_name].append(cur.fetchone()[0])
conn.commit()
total_customers = sum(len(v) for v in customer_ids_by_org.values())
print(f"  {total_customers} customers")

print("Seeding products…")
product_ids_by_org = {name: [] for name in org_ids}
for org_name, org_id in org_ids.items():
    for i in range(20):
        pid = str(uuid.uuid4())
        price = decimal.Decimal(str(round(random.uniform(9.99, 499.99), 2)))
        cost = price * decimal.Decimal("0.6")
        margin = (price - cost) / price
        cur.execute(
            """
            INSERT INTO products (id, organization_id, name, description, price, cost_price, margin)
            VALUES (%s, %s, %s, %s, %s, %s, %s) RETURNING id
            """,
            (
                pid,
                org_id,
                fake.catch_phrase(),
                fake.text(max_nb_chars=100),
                price,
                round(cost, 2),
                round(margin, 4),
            ),
        )
        product_ids_by_org[org_name].append(cur.fetchone()[0])
conn.commit()
total_products = sum(len(v) for v in product_ids_by_org.values())
print(f"  {total_products} products")

print("Seeding orders and order items…")
order_count = 0
item_count = 0
payment_count = 0
for org_name, org_id in org_ids.items():
    customers = customer_ids_by_org[org_name]
    products = product_ids_by_org[org_name]
    for _ in range(34):
        order_id = str(uuid.uuid4())
        customer_id = random.choice(customers)
        status = random.choice(STATUSES)
        cur.execute(
            """
            INSERT INTO orders (id, organization_id, customer_id, status)
            VALUES (%s, %s, %s, %s) RETURNING id
            """,
            (order_id, org_id, customer_id, status),
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

cur.close()
conn.close()
print("Done.")
