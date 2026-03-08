# BetweenRows: User Stories & Use Case Catalog

> **Context**: This document uses a comprehensive **E-commerce Platform** schema to provide concrete SQL examples for each permission scenario. All examples assume the following base schema, which represents a multi-tenant B2B e-commerce platform serving multiple marketplace vendors.

---

## Assumed E-Commerce Schema

### Core Tables

```sql
-- Multi-tenant B2B E-commerce Platform

-- Organizations (Tenants/Vendors)
CREATE TABLE organizations (
    id              UUID PRIMARY KEY,
    name            VARCHAR(255),
    tier            VARCHAR(50), -- 'free', 'pro', 'enterprise'
    parent_org_id   UUID REFERENCES organizations(id),
    created_at      TIMESTAMP
);

-- Users (Both customers and internal employees)
CREATE TABLE users (
    id              UUID PRIMARY KEY,
    email           VARCHAR(255) UNIQUE,
    name            VARCHAR(255),
    role            VARCHAR(50), -- 'admin', 'manager', 'analyst', 'support', 'customer'
    organization_id UUID REFERENCES organizations(id), -- For internal users
    department_id   UUID REFERENCES departments(id),
    manager_id      UUID REFERENCES users(id),
    region_id       UUID REFERENCES regions(id),
    clearance_level INT DEFAULT 1, -- 1-5
    is_active       BOOLEAN DEFAULT true,
    created_at      TIMESTAMP
);

-- Customers (End consumers of the platform)
CREATE TABLE customers (
    id              UUID PRIMARY KEY,
    organization_id UUID REFERENCES organizations(id), -- Tenant (e.g., a retailer)
    name            VARCHAR(255),
    email           VARCHAR(255),
    ssn             VARCHAR(11), -- Social Security Number (PII)
    credit_card     VARCHAR(19), -- Credit Card Number (PII)
    phone           VARCHAR(20),
    billing_address TEXT,
    shipping_address TEXT,
    created_at      TIMESTAMP,
    account_tier    VARCHAR(20) -- 'standard', 'premium', 'vip'
);

-- Orders
CREATE TABLE orders (
    id              UUID PRIMARY KEY,
    customer_id     UUID REFERENCES customers(id),
    organization_id UUID REFERENCES organizations(id), -- Tenant
    total_amount    DECIMAL(10,2),
    tax_amount      DECIMAL(10,2),
    status          VARCHAR(50), -- 'pending', 'processing', 'shipped', 'delivered', 'cancelled'
    shipping_method VARCHAR(50),
    created_at      TIMESTAMP,
    updated_at      TIMESTAMP
);

-- Products
CREATE TABLE products (
    id              UUID PRIMARY KEY,
    organization_id UUID REFERENCES organizations(id), -- Tenant
    name            VARCHAR(255),
    sku             VARCHAR(50),
    category        VARCHAR(100),
    unit_price      DECIMAL(10,2),
    cost_price      DECIMAL(10,2), -- Internal only
    margin          DECIMAL(10,2), -- Computed
    stock_quantity  INT,
    supplier_id     UUID,
    is_active       BOOLEAN DEFAULT true,
    created_at      TIMESTAMP
);

-- Order Items (Many-to-Many between orders and products)
CREATE TABLE order_items (
    id          UUID PRIMARY KEY,
    order_id    UUID REFERENCES orders(id),
    product_id  UUID REFERENCES products(id),
    quantity    INT,
    unit_price  DECIMAL(10,2),
    discount    DECIMAL(10,2)
);

-- Support Tickets (Unstructured data)
CREATE TABLE support_tickets (
    id          UUID PRIMARY KEY,
    customer_id UUID REFERENCES customers(id),
    order_id    UUID REFERENCES orders(id),
    subject     VARCHAR(255),
    comments    TEXT, -- May contain PII in free text
    status      VARCHAR(50),
    priority    VARCHAR(20),
    created_at  TIMESTAMP,
    resolved_at TIMESTAMP
);

-- Payments (Financial data)
CREATE TABLE payments (
    id              UUID PRIMARY KEY,
    order_id        UUID REFERENCES orders(id),
    customer_id     UUID REFERENCES customers(id),
    amount          DECIMAL(10,2),
    payment_method  VARCHAR(50),
    transaction_id  VARCHAR(100),
    status          VARCHAR(50),
    created_at      TIMESTAMP
);
```

### Organizational & Regional Tables

```sql
-- Departments (Hierarchical)
CREATE TABLE departments (
    id                  UUID PRIMARY KEY,
    name                VARCHAR(100),
    parent_department_id UUID REFERENCES departments(id),
    cost_center         VARCHAR(50)
);

-- Regions (Geographic)
CREATE TABLE regions (
    id          UUID PRIMARY KEY,
    name        VARCHAR(100),
    country     VARCHAR(100),
    timezone    VARCHAR(50),
    is_active   BOOLEAN DEFAULT true
);

-- Teams (For project-based access)
CREATE TABLE teams (
    id          UUID PRIMARY KEY,
    name        VARCHAR(100),
    project_id  UUID -- Optional link to a project
);

-- Team Members (Many-to-Many)
CREATE TABLE team_members (
    team_id     UUID REFERENCES teams(id),
    user_id     UUID REFERENCES users(id),
    role        VARCHAR(50), -- 'lead', 'member', 'viewer'
    joined_at   TIMESTAMP,
    PRIMARY KEY (team_id, user_id)
);

-- Employee Permissions (Custom attribute-based access)
CREATE TABLE user_attributes (
    user_id     UUID REFERENCES users(id),
    attribute_key VARCHAR(100),
    attribute_value VARCHAR(255),
    PRIMARY KEY (user_id, attribute_key)
);
```

### Access Control & Audit Tables

```sql
-- Conflict of Interest (Negative relationships)
CREATE TABLE conflict_of_interest (
    id              UUID PRIMARY KEY,
    user_id         UUID REFERENCES users(id),
    customer_id     UUID REFERENCES customers(id),
    reason          VARCHAR(255),
    created_at      TIMESTAMP,
    expires_at      TIMESTAMP -- Optional expiration
);

-- Group Membership (For group-based access)
CREATE TABLE user_groups (
    id          UUID PRIMARY KEY,
    name        VARCHAR(100),
    type        VARCHAR(50) -- 'department', 'role', 'project', 'custom'
);

CREATE TABLE group_members (
    group_id    UUID REFERENCES user_groups(id),
    user_id     UUID REFERENCES users(id),
    PRIMARY KEY (group_id, user_id)
);

-- Folder/Document Hierarchy (Parent-child relationships)
CREATE TABLE folders (
    id          UUID PRIMARY KEY,
    name        VARCHAR(255),
    parent_id   UUID REFERENCES folders(id),
    owner_id    UUID REFERENCES users(id),
    access_list JSONB -- Permissions for this folder
);

CREATE TABLE documents (
    id          UUID PRIMARY KEY,
    folder_id   UUID REFERENCES folders(id),
    name        VARCHAR(255),
    content     TEXT,
    owner_id    UUID REFERENCES users(id),
    created_at  TIMESTAMP
);

-- Break-Glass Access Grants
CREATE TABLE temporary_access (
    id              UUID PRIMARY KEY,
    user_id         UUID REFERENCES users(id),
    resource_type   VARCHAR(50), -- 'table', 'column', 'row'
    resource_id     UUID,
    access_level    VARCHAR(20), -- 'read', 'write', 'admin'
    granted_by      UUID REFERENCES users(id),
    granted_at      TIMESTAMP,
    expires_at      TIMESTAMP,
    reason          TEXT
);

-- Rate Limiting / Quotas
CREATE TABLE user_quotas (
    user_id         UUID REFERENCES users(id),
    quota_type      VARCHAR(50), -- 'rows_per_hour', 'queries_per_day', 'data_mb_per_day'
    limit_value     INT,
    current_usage   INT DEFAULT 0,
    window_start    TIMESTAMP,
    PRIMARY KEY (user_id, quota_type)
);

-- Audit Logs
CREATE TABLE audit_logs (
    id              UUID PRIMARY KEY,
    user_id         UUID REFERENCES users(id),
    action          VARCHAR(50), -- 'SELECT', 'INSERT', 'UPDATE', 'DELETE', 'MASKED', 'DENIED'
    table_name      VARCHAR(100),
    resource_id     UUID,
    original_query  TEXT,
    rewritten_query TEXT,
    ip_address      VARCHAR(45),
    user_agent      TEXT,
    timestamp       TIMESTAMP DEFAULT NOW()
);
```

---

## Persona Reference

Eight canonical personas cover all actor labels used across these stories. Use the parenthetical notation on first mention in a new context: e.g., "As a **Finance Manager** (Manager)".

| Canonical Persona | Covers These Labels | Access Profile |
|---|---|---|
| **Platform Admin** | Platform Admin, Security Admin, System Architect, DBA, DevOps Engineer | Full access, system configuration, monitoring. No tenant restrictions. |
| **Security & Compliance** | Compliance Officer, Data Privacy Officer, Data Governance Officer, Data Custodian, Risk Manager, GDPR Officer, IP Security Team, Device Trust Officer | Policy authoring, audit review, masking rule design. May have elevated PII access for audit. |
| **Auditor** | Auditor, Internal Investigator, Legal Hold Manager | Temporary full visibility for investigations. Read-only with masking bypass. |
| **Manager** | Finance Manager, Sales VP, Regional Director, Department Head, HR Director, Team Lead, Procurement Manager, Product Manager, Warehouse Manager | Sees data for their scope (region, team, department, org). May see unmasked data for direct reports. |
| **Analyst** | Data Analyst, Business Analyst, Data Scientist, Fraud Analyst, Security Analyst, QA Engineer | Read-only, scoped to assigned datasets. PII masked or hidden. May access aggregated data. |
| **Support Agent** | Support Rep, Support Lead, Account Manager | Sees customer data for assigned accounts/tenant. PII partially masked (last-4 SSN, partial email). |
| **Developer / Engineer** | Data Engineer, Developer, QA Lead, Cache Manager, Migration Lead, Rollout Manager | Dev/staging environment access. Production access via break-glass only. |
| **External / Partner** | Partner Integration User, Multi-tenant User, Customer | Restricted to public/shared data or own records. No internal columns visible. |

---

## Priority Tiers

Every story carries a `Priority` label. Use these tiers when planning the roadmap.

| Tier | Label | Criteria |
|------|-------|----------|
| **P0 — MVP** | Must-have for first paying customer | Directly maps to PLAN.md Phases B–E. Core tenant isolation, basic column masking, column hiding, role-based access. |
| **P1 — V1** | Required for production-grade product | Conditional masking, ABAC, basic ReBAC, audit logging, YAML policy-as-code, break-glass access. |
| **P2 — Future** | Differentiators and edge cases | Hierarchical/recursive access, contextual guardrails (geo, time, device), migration tooling, advanced ReBAC. |

**P0 stories (11):** DS-01, DS-02, DS-04, DS-10, MT-01, MT-05, MT-06, RE-01, CC-01, CC-03, AU-01

**P1 stories (20):** DS-03, DS-05, DS-06, DS-09, DS-12, MT-02, MT-03, MT-04, MT-08, RE-02, RE-06, RE-09, CX-01, CX-03, CX-06, AU-02, AU-04, AU-08, CC-07, DM-05

**P2 stories (remaining):** All HI-xx; DS-07, DS-08, DS-11, DS-13, DF-01, DF-02, DF-03, DF-04, DF-05; MT-07, MT-09; RE-03–RE-05, RE-07, RE-08, RE-10; CX-02, CX-04, CX-05, CX-07–CX-10; AU-05–AU-07; CC-02, CC-04–CC-06; DM-01–DM-04

---

## 1. Data Masking & Field-Level Security

_Objective: Controlling visibility of specific attributes within a dataset, transforming sensitive data at query time without altering the underlying storage._

| ID | Priority | User Story | Use Case Example | Logic Requirement | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- | :--- |
| **DS-01** | P0 | As a **Security Lead** (Platform Admin), I want to partially mask sensitive strings (e.g., last 4 digits of SSN, or the first and last four digits of credit cards), so analysts can verify records without seeing full PII. | Masking customer PII for support agents, allowing partial verification. | Pattern-based masking; Conditional logic for string manipulation. | **Original**: `SELECT id, name, ssn, credit_card FROM customers WHERE id = 'cust-123';` <br><br> **Rendered**: `SELECT id, name, CONCAT('***-**-', RIGHT(ssn, 4)) AS ssn, CONCAT(LEFT(credit_card, 4), '****-****-', RIGHT(credit_card, 4)) AS credit_card FROM customers WHERE id = 'cust-123';` |
| **DS-02** | P0 | As a **Finance Manager** (Manager), I want specific financial columns (e.g., `cost_price`, `margin`) to be visible to my team but replaced with `NULL` or a default value for others. | Restricting internal margin/cost data from the general Sales team or external partners. | Role-based column filtering/nullification. | **Original**: `SELECT id, sku, name, unit_price, cost_price, margin FROM products WHERE id = 'prod-456';` <br><br> **Rendered (for Sales User)**: `SELECT id, sku, name, unit_price, NULL AS cost_price, 0.00 AS margin FROM products WHERE id = 'prod-456';` |
| **DS-03** | P1 | As a **Data Privacy Officer** (Security & Compliance), I want to automatically redact or replace patterns (emails, phone numbers, URLs) in unstructured "Notes" or `TEXT` columns to prevent accidental PII exposure. | Scrubbing PII from `support_tickets.comments` or customer feedback forms. | Regular expression-based redaction/replacement. | **Original**: `SELECT id, subject, comments FROM support_tickets WHERE id = 'ticket-789';` <br><br> **Rendered**: `SELECT id, subject, REGEXP_REPLACE(comments, '[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}|\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}', '[REDACTED PII]', 'g') AS comments FROM support_tickets WHERE id = 'ticket-789';` |
| **DS-04** | P0 | As a **Data Analyst** (Analyst), I want the proxy to return generic, non-breaking placeholders (e.g., `[RESTRICTED]`, `N/A`) instead of SQL errors when I query a column for which I lack full visibility. | Preventing Tableau/Looker dashboards or custom reports from breaking when a user lacks granular column permissions. | Error suppression with static placeholder substitution. | **Original**: `SELECT id, name, ssn FROM customers WHERE id = 'cust-123';` (User lacks `ssn` access) <br><br> **Rendered**: `SELECT id, name, '[RESTRICTED]' AS ssn FROM customers WHERE id = 'cust-123';` |
| **DS-05** | P1 | As a **Compliance Officer** (Security & Compliance), I want to implement *conditional masking* where the level of data masking varies based on both the user's role and other attributes (e.g., data sensitivity, context). | Tiered visibility for customer PII: Support sees partial phone, Managers see full phone for *their* direct reports, and Admins see full for all. | Multi-factor conditional logic (role, relationship, data sensitivity). | **Original**: `SELECT u.name, c.phone, c.email FROM users u JOIN customers c ON u.id = c.user_id WHERE c.id = 'cust-123';` <br><br> **Rendered (for Support User)**: `SELECT u.name, CONCAT('***-***-', RIGHT(c.phone, 4)) AS phone, CONCAT(LEFT(c.email, 2), '***@', SUBSTRING(c.email FROM POSITION('@' IN c.email)+1)) AS email FROM users u JOIN customers c ON u.id = c.user_id WHERE c.id = 'cust-123';` <br><br> **Rendered (for Manager of the customer's org)**: `SELECT u.name, c.phone, CONCAT(LEFT(c.email, 2), '***@', SUBSTRING(c.email FROM POSITION('@' IN c.email)+1)) AS email FROM users u JOIN customers c ON u.id = c.user_id WHERE c.id = 'cust-123' AND u.manager_id = CURRENT_USER_ID;` <br><br> **Rendered (for Admin User)**: `SELECT u.name, c.phone, c.email FROM users u JOIN customers c ON u.id = c.user_id WHERE c.id = 'cust-123';` |
| **DS-06** | P1 | As a **Risk Manager** (Security & Compliance), I want to mask numeric values (e.g., account balances, credit scores, order totals) into predefined buckets or ranges to protect exact figures while retaining statistical utility. | Preventing exact financial data from being exposed to sales or junior analysts, only providing approximate values. | Numeric range classification with `CASE` statements. | **Original**: `SELECT id, name, total_amount FROM orders WHERE organization_id = 'org-456';` <br><br> **Rendered**: `SELECT id, name, CASE WHEN total_amount >= 10000 THEN '>= 10k' WHEN total_amount >= 5000 THEN '5k-10k' WHEN total_amount >= 1000 THEN '1k-5k' ELSE '<1k' END AS total_amount_range FROM orders WHERE organization_id = 'org-456';` |
| **DS-07** | P2 | As a **Data Engineer** (Developer / Engineer), I want to tokenize sensitive identifiers (e.g., SSN, internal customer IDs) so that their real value is hidden, but they can still be used for joins and internal system reconciliation. | Allowing joins on PII-related columns across different datasets (e.g., `customers` and `support_tickets`) without exposing raw PII. | Consistent hashing or token lookup for sensitive columns. | **Original**: `SELECT o.id, c.name FROM orders o JOIN customers c ON o.customer_id = c.id WHERE c.ssn = '123-45-6789';` <br><br> **Rendered (using a tokenized SSN)**: `SELECT o.id, c.name FROM orders o JOIN customers c ON o.customer_id = c.id WHERE c.ssn_token = 'XYZ-TOKEN-123';` (Assumes `ssn_token` is stored/generated consistently). <br><br> **Alternative (hashing on the fly)**: `SELECT o.id, c.name FROM orders o JOIN customers c ON o.customer_id = c.id WHERE SHA256(c.ssn || 'SALT') = SHA256('123-45-6789' || 'SALT');` |
| **DS-08** | P2 | As a **Product Manager** (Manager), I want to mask or completely hide future product pricing, unreleased product features, or launch dates until the official announcement or launch. | Keeping confidential product roadmap details from unauthorized internal teams or external partners. | Time-based or status-based conditional column masking/nullification. | **Original**: `SELECT id, name, unit_price, cost_price, created_at FROM products WHERE organization_id = 'org-product-dev';` <br><br> **Rendered (for non-Product Team before launch)**: `SELECT id, name, NULL AS unit_price, NULL AS cost_price, NULL AS created_at FROM products WHERE organization_id = 'org-product-dev' AND is_unreleased = TRUE;` <br><br> **Rendered (for non-Product Team after launch)**: `SELECT id, name, unit_price, cost_price, created_at FROM products WHERE organization_id = 'org-product-dev' AND is_unreleased = FALSE;` |
| **DS-09** | P1 | As an **Auditor** or **Internal Investigator** (Auditor), I want to temporarily bypass all masking rules to see the actual raw data for all fields for specific audit purposes, while other users remain masked. | Full visibility for security reviews, legal discovery, or incident response without changing global policies. | Role-based or temporary elevation for masking bypass. | **Original**: `SELECT * FROM customers WHERE id = 'cust-audit';` (User is Auditor) <br><br> **Rendered**: `SELECT id, organization_id, name, email, ssn, credit_card, phone, billing_address, shipping_address, created_at, account_tier FROM customers WHERE id = 'cust-audit';` (No masking applied for `ssn`, `credit_card`, etc.). |
| **DS-10** | P0 | As a **Support Lead** (Support Agent), I want to completely hide highly sensitive columns (e.g., `ssn`, raw `credit_card` numbers) for all users by removing them from the `SELECT` list, except for a very limited set of users with explicit, elevated access. | Maximum security for critical PII; preventing any accidental exposure even if masked. | Column removal (projection filtering). | **Original**: `SELECT id, name, ssn, credit_card, email FROM customers WHERE id = 'cust-456';` (User lacks SSN/CC access) <br><br> **Rendered**: `SELECT id, name, email FROM customers WHERE id = 'cust-456';` (Columns `ssn` and `credit_card` are entirely removed from the projection). |
| **DS-11** | P2 | As a **Compliance Officer** (Security & Compliance), I want to ensure that if a user has access to a PII column, they must also meet specific contextual conditions (e.g., IP address from corporate network). | Preventing PII data access from untrusted networks, even if the user has a valid role. | Multi-factor conditional access, combining column access with contextual checks. | **Original**: `SELECT id, name, ssn FROM customers WHERE id = 'cust-789';` <br><br> **Rendered (from Public IP)**: `SELECT id, name, '[ACCESS DENIED - UNTRUSTED NETWORK]' AS ssn FROM customers WHERE id = 'cust-789';` <br><br> **Rendered (from Corporate VPN)**: `SELECT id, name, CONCAT('***-**-', RIGHT(ssn, 4)) AS ssn FROM customers WHERE id = 'cust-789';` |
| **DS-12** | P1 | As a **Data Custodian** (Security & Compliance), I want to prevent direct access to certain raw PII columns but allow access to their hashed versions for analytical purposes (e.g., count unique users by hashed SSN). | Allowing privacy-preserving analytics while blocking direct PII exposure. | Column substitution with derived/hashed values. | **Original**: `SELECT ssn FROM customers;` <br><br> **Rendered**: Error "Direct access to SSN is prohibited. Use SSN_HASH for analytics." <br><br> **Original**: `SELECT MD5(ssn) AS ssn_hash FROM customers;` <br><br> **Rendered**: `SELECT MD5(ssn) AS ssn_hash FROM customers;` (Allowed). |
| **DS-13** | P2 | As a **Developer** (Developer / Engineer), I want to see different mock data for sensitive columns in a development environment compared to staging or production. | Environment-specific data masking/faking for testing without real PII. | Environment-aware conditional masking. | **Original**: `SELECT id, name, email, ssn FROM customers;` (In DEV environment) <br><br> **Rendered**: `SELECT id, name, CONCAT('dev_user_', id, '@example.com') AS email, CONCAT('DEV-', LPAD(id::text, 7, '0')) AS ssn FROM customers;` |
| **DF-01** | P2 | As a **Business Analyst** (Analyst), I want to dynamically calculate and show columns based on user permissions (e.g., "Discount" column shows real value for VIP, 0 for standard). | Dynamic column generation. | Permission-conditional column expression. | **Original**: `SELECT id, price, discount FROM products;` <br><br> **Rendered (Standard User)**: `SELECT id, price, 0 AS discount FROM products;` <br><br> **Rendered (VIP User)**: `SELECT id, price, discount FROM products;` |
| **DF-02** | P2 | As a **Privacy Engineer** (Developer / Engineer), I want to transform JSON columns into flattened tables with restricted fields. | Handling semi-structured data. | JSON key stripping at projection time. | **Original**: `SELECT preferences FROM customers;` (JSON: `{"ssn": "123", "diet": "vegan"}`) <br><br> **Rendered**: `SELECT (preferences->>'diet') AS diet FROM customers;` (SSN key stripped). |

---

## 2. Multi-Tenancy & Identity Isolation

_Objective: Enforcing hard data boundaries based on the identity of the logged-in user, ensuring complete data separation between tenants or organizations._

| ID | Priority | User Story | Use Case Example | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- |
| **MT-01** | P0 | As a **SaaS Developer** (Platform Admin), I want the proxy to automatically inject `WHERE tenant_id = 'XYZ'` into every query. | Ensuring Client A never accidentally sees Client B's data in a shared database. | **Original**: `SELECT * FROM orders;` <br><br> **Rendered**: `SELECT * FROM orders WHERE organization_id = 'org-abc-123';` |
| **MT-02** | P1 | As a **Sales VP** (Manager), I want regional reps to only see rows where the `region_id` matches their assigned territory. | Filtering the `leads` table so the "EMEA" team only sees European leads. | **Original**: `SELECT * FROM customers;` (User is in EMEA region) <br><br> **Rendered**: `SELECT * FROM customers WHERE region_id = 'region-emea';` |
| **MT-03** | P1 | As a **Product Owner** (Manager), I want to filter data based on custom attributes (e.g., `clearance_level`) passed via JWT. | Allowing "Level 3" employees to see restricted product roadmap rows. | **Original**: `SELECT * FROM product_roadmap WHERE sensitivity_level <= 3;` <br><br> **Rendered**: `SELECT * FROM product_roadmap WHERE sensitivity_level <= 3;` (User has clearance_level=3 in JWT). |
| **MT-04** | P1 | As an **Account Manager** (Support Agent), I want to see data for all the specific customer accounts assigned to me. | A sales rep managing 50 specific enterprise accounts. | **Original**: `SELECT * FROM customers;` <br><br> **Rendered**: `SELECT * FROM customers WHERE account_manager_id = 'user-789';` |
| **MT-05** | P0 | As a **Platform Admin**, I want to see data across ALL tenants for system monitoring and support. | Super-user access for debugging. | **Original**: `SELECT * FROM orders;` (User is Super Admin) <br><br> **Rendered**: `SELECT * FROM orders;` (No filtering applied). |
| **MT-06** | P0 | As a **Multi-tenant User** (External / Partner), I want to access data from multiple organizations I belong to. | A consultant working with Company A and Company B. | **Original**: `SELECT * FROM orders;` <br><br> **Rendered**: `SELECT * FROM orders WHERE organization_id IN ('org-a', 'org-b');` |
| **MT-07** | P2 | As a **Warehouse Manager** (Manager), I want to see inventory data only for the warehouses I manage. | Inventory visibility per warehouse location. | **Original**: `SELECT * FROM inventory;` <br><br> **Rendered**: `SELECT * FROM inventory WHERE warehouse_id IN (SELECT warehouse_id FROM user_warehouses WHERE user_id = 'user-123');` |
| **MT-08** | P1 | As a **Partner Integration User** (External / Partner), I want to see only shared/public data, not tenant-specific private data. | Third-party API integrations. | **Original**: `SELECT * FROM products;` <br><br> **Rendered**: `SELECT * FROM products WHERE is_public = true;` |
| **MT-09** | P2 | As a **Compliance Manager** (Security & Compliance), I want to ensure that data from "sandbox" or "trial" tenants is completely isolated from production data. | Test environments vs Production. | **Original**: `SELECT * FROM customers;` <br><br> **Rendered**: `SELECT * FROM customers WHERE organization_id = 'org-sandbox-01' AND is_production = false;` |

---

## 3. Advanced ReBAC (Many-to-Many & Transitive)

_Objective: Solving complex "Graph-based" authorization at the SQL layer where direct table columns aren't enough to determine access. This involves traversing relationships through join tables to determine visibility._

### 3.1 Relationship-Based Access Control (ReBAC)

| ID | Priority | User Story | Use Case Example | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- |
| **RE-01** | P0 | As a **Support Rep** (Support Agent), I want the proxy to auto-join `orders` → `customers` to verify the `tenant_id` of the order owner. | Filtering an `orders` table that lacks its own `tenant_id` column but inherits it from the `customers` table. | **Original**: `SELECT * FROM orders;` <br><br> **Rendered**: `SELECT o.* FROM orders o JOIN customers c ON o.customer_id = c.id WHERE c.organization_id = 'org-abc';` |
| **RE-02** | P1 | As a **Project Member** (Analyst), I want to only see rows in `tasks` if my ID exists in a `project_members` bridge table. | Implementing "Team-only" access for collaborative workspaces. | **Original**: `SELECT * FROM project_tasks;` <br><br> **Rendered**: `SELECT pt.* FROM project_tasks pt JOIN project_members pm ON pt.project_id = pm.project_id WHERE pm.user_id = 'user-123';` |
| **RE-03** | P2 | As a **Security Admin** (Platform Admin), I want to force joins from "orphan" tables (like `comments`) back to parent entities to verify access. | Securing a `comments` table by tracing its parent `order_id` to verify the user has access to the order. | **Original**: `SELECT * FROM order_comments;` <br><br> **Rendered**: `SELECT oc.* FROM order_comments oc JOIN orders o ON oc.order_id = o.id JOIN customers c ON o.customer_id = c.id WHERE c.organization_id = 'org-abc';` |
| **RE-04** | P2 | As a **Team Lead** (Manager), I want to see all documents in folders owned by any of my team members. | Manager accessing team resources. | **Original**: `SELECT * FROM documents;` <br><br> **Rendered**: `SELECT d.* FROM documents d JOIN team_members tm ON d.owner_id = tm.user_id WHERE tm.team_id IN (SELECT team_id FROM team_members WHERE user_id = 'user-lead' AND role = 'lead');` |
| **RE-05** | P2 | As a **Department Head** (Manager), I want to see all `assets` owned by any sub-team within my branch. | Transitive "Ownership" traversal. | **Original**: `SELECT * FROM assets;` <br><br> **Rendered**: `SELECT a.* FROM assets a JOIN users u ON a.owner_id = u.id WHERE u.department_id IN (WITH RECURSIVE dept_tree AS (SELECT id FROM departments WHERE id = 'dept-executive' UNION ALL SELECT d.id FROM departments d JOIN dept_tree dt ON d.parent_department_id = dt.id) SELECT id FROM dept_tree);` |
| **RE-06** | P1 | As a **Security Admin** (Platform Admin), I want to block access if a `conflict_of_interest` relationship exists between a user and a client. | Negative Relationship (Exclusion logic). | **Original**: `SELECT * FROM customers;` <br><br> **Rendered**: `SELECT * FROM customers c WHERE c.id NOT IN (SELECT customer_id FROM conflict_of_interest WHERE user_id = 'user-123' AND (expires_at IS NULL OR expires_at > NOW()));` |
| **RE-07** | P2 | As a **User** (External / Partner), I want to see `files` if I am in a `group` that has `viewer` access to the parent `folder`. | Inherited permissions via parent-child relationship. | **Original**: `SELECT * FROM documents;` <br><br> **Rendered**: `SELECT d.* FROM documents d JOIN folders f ON d.folder_id = f.id JOIN folder_permissions fp ON f.id = fp.folder_id JOIN group_members gm ON fp.group_id = gm.group_id WHERE gm.user_id = 'user-123' AND fp.permission_level >= 1;` |
| **RE-08** | P2 | As a **Customer** (External / Partner), I want to see only my own orders, regardless of tenant isolation rules in some contexts (for portal access). | B2B Customer Portal viewing their own history. | **Original**: `SELECT * FROM orders;` <br><br> **Rendered**: `SELECT * FROM orders WHERE customer_id = 'customer-portal-user-id';` (Mapping customer ID to user ID). |
| **RE-09** | P1 | As a **Procurement Manager** (Manager), I want to see all supplier contracts where either I am the owner OR I am a member of the buying group. | Complex multi-group ownership. | **Original**: `SELECT * FROM supplier_contracts;` <br><br> **Rendered**: `SELECT * FROM supplier_contracts sc WHERE sc.owner_id = 'user-123' OR sc.id IN (SELECT contract_id FROM contract_groups WHERE group_id IN (SELECT group_id FROM group_members WHERE user_id = 'user-123'));` |
| **RE-10** | P2 | As an **HR Manager** (Manager), I want to see employee data only for employees who are in the same legal entity as me. | Filtering across subsidiaries. | **Original**: `SELECT * FROM employees;` <br><br> **Rendered**: `SELECT * FROM employees e WHERE e.legal_entity_id = (SELECT legal_entity_id FROM users WHERE id = 'user-hr');` |

---

## 4. Hierarchical & Recursive Access

_Objective: Managing visibility within organizational structures, management chains, and recursive data structures (trees/graphs)._

| ID | Priority | User Story | Use Case Example | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- |
| **HI-01** | P2 | As a **Manager**, I want to automatically see the `salary` of my direct reports without manual permissioning. | Granting visibility based on the `reports_to_id` column. | **Original**: `SELECT name, salary FROM employees;` <br><br> **Rendered**: `SELECT name, salary FROM employees WHERE manager_id = 'user-123';` |
| **HI-02** | P2 | As a **Senior Executive** (Manager), I want the proxy to walk the "org tree" so I can see data for my entire downline (indirect reports). | A VP seeing the performance metrics of everyone in their department. | **Original**: `SELECT * FROM employees;` <br><br> **Rendered**: `SELECT * FROM employees WHERE id IN (WITH RECURSIVE org_tree AS (SELECT id FROM users WHERE manager_id = 'user-vp' UNION ALL SELECT u.id FROM users u JOIN org_tree ot ON u.manager_id = ot.id) SELECT id FROM org_tree);` |
| **HI-03** | P2 | As an **HR Director** (Manager), I want to ensure peers at the same level cannot see each other's data, even with the same role. | Preventing Manager A from seeing Manager B's compensation. | **Original**: `SELECT * FROM employees WHERE role = 'manager';` (Executed by Manager A) <br><br> **Rendered**: `SELECT * FROM employees WHERE role = 'manager' AND id != 'user-manager-b' AND manager_id = 'user-vp';` (Excludes peers). |
| **HI-04** | P2 | As a **Team Lead** (Manager), I want to see all tickets assigned to my team, including those I didn't assign personally. | Support team lead viewing team queue. | **Original**: `SELECT * FROM support_tickets;` <br><br> **Rendered**: `SELECT * FROM support_tickets st WHERE st.assignee_id IN (SELECT user_id FROM team_members WHERE team_id IN (SELECT team_id FROM team_members WHERE user_id = 'user-lead' AND role = 'lead'));` |
| **HI-05** | P2 | As a **Regional Director** (Manager), I want to see aggregated sales data for all sub-regions under my territory. | Rolling up data from child regions. | **Original**: `SELECT region, SUM(sales) FROM sales_data;` <br><br> **Rendered**: `SELECT region, SUM(sales) FROM sales_data WHERE region_id IN (WITH RECURSIVE region_tree AS (SELECT id FROM regions WHERE id = 'region-north-america' UNION ALL SELECT r.id FROM regions r JOIN region_tree rt ON r.parent_region_id = rt.id) SELECT id FROM region_tree) GROUP BY region;` |
| **HI-06** | P2 | As a **Delegated Assistant** (Support Agent), I want to see my manager's tasks as if they were mine. | Executive assistant access. | **Original**: `SELECT * FROM tasks WHERE assignee_id = 'user-manager';` <br><br> **Rendered**: `SELECT * FROM tasks WHERE assignee_id IN ('user-manager', 'user-assistant') OR (SELECT manager_id FROM users WHERE id = 'user-assistant') = 'user-manager';` |
| **HI-07** | P2 | As a **Cross-Functional Lead** (Manager), I want to see data for users who report to me OR who are in my project team (dual hierarchy). | Matrix organization structure. | **Original**: `SELECT * FROM projects;` <br><br> **Rendered**: `SELECT * FROM projects p WHERE p.owner_id IN (SELECT id FROM users WHERE manager_id = 'user-lead') OR p.id IN (SELECT project_id FROM team_members WHERE user_id = 'user-lead');` |

---

## 5. Contextual & Behavioral Guardrails

_Objective: Dynamic security based on the environment, user behavior, geolocation, time, or device characteristics. These rules adapt to the context of the access request._

| ID | Priority | User Story | Use Case Example | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- |
| **CX-01** | P1 | As a **Security Admin** (Platform Admin), I want to grant "Break-Glass" access to a developer that automatically expires. | Providing 2-hour production access for emergency debugging. | **Original**: `SELECT * FROM orders;` <br><br> **Rendered**: `SELECT * FROM orders WHERE organization_id = 'tenant-123' OR 1=1 IN (SELECT 1 FROM temporary_access WHERE user_id = CURRENT_USER AND access_level = 'admin' AND expires_at > NOW());` |
| **CX-02** | P2 | As a **Compliance Officer** (Security & Compliance), I want to allow full access only via Corporate VPN and force masking on public IPs. | Geo-fencing sensitive financial data to the physical office. | **Original**: `SELECT * FROM payments;` <br><br> **Rendered (VPN)**: `SELECT * FROM payments;` <br><br> **Rendered (Public IP)**: `SELECT id, amount, '***' AS transaction_id FROM payments;` (Masked columns). |
| **CX-03** | P1 | As a **Security Analyst** (Analyst), I want to rate-limit the number of rows a user can fetch per hour. | Preventing a "Data Dump" by a disgruntled employee before they quit. | **Original**: `SELECT * FROM customers;` (User has already fetched 9000 rows this hour) <br><br> **Rendered**: `SELECT * FROM customers LIMIT 1000;` (Quota applied). Error if limit exceeded. |
| **CX-04** | P2 | As a **QA Engineer** (Analyst), I want the proxy to anonymize production data being pulled into a Staging environment. | Using real data shapes for testing without risking real PII. | **Original**: `SELECT name, email FROM customers;` (Env: Staging) <br><br> **Rendered**: `SELECT CONCAT('User', id) AS name, CONCAT(id, '@example.com') AS email FROM customers;` |
| **CX-05** | P2 | As a **Security Lead** (Platform Admin), I want to block access during maintenance windows. | Preventing changes during system upgrades. | **Original**: `SELECT * FROM products;` (During maintenance window) <br><br> **Rendered**: Error "Maintenance in progress. Access denied." |
| **CX-06** | P1 | As a **Data Governance Officer** (Security & Compliance), I want to block queries that return > 1000 rows unless explicitly approved. | Preventing mass exfiltration. | **Original**: `SELECT * FROM customers;` (Result set: 50,000 rows) <br><br> **Rendered**: Error "Query exceeds 1000 row limit. Please add a WHERE clause to filter." |
| **CX-07** | P2 | As an **IP Security Team** (Security & Compliance), I want to block access from specific countries or IP ranges (Sanctions). | Legal compliance with sanctions. | **Original**: `SELECT * FROM users;` (User IP: 192.168.1.1 from Sanctioned Country) <br><br> **Rendered**: Error "Access denied from your region." |
| **CX-08** | P2 | As a **Device Trust Officer** (Security & Compliance), I want to allow full writes only from managed devices, read-only from unmanaged. | Endpoint security. | **Original**: `UPDATE orders SET status = 'cancelled' WHERE id = 'order-123';` (From personal phone) <br><br> **Rendered**: Error "Write access restricted to managed devices." |
| **CX-09** | P2 | As a **Time-Based Policy Enforcer** (Platform Admin), I want to restrict sensitive data access to business hours only. | Payroll access during work hours only. | **Original**: `SELECT salary FROM employees;` (Executed at 8PM) <br><br> **Rendered**: Error "Access to salary data is only available between 9AM-5PM." |
| **CX-10** | P2 | As a **Fraud Analyst** (Analyst), I want to trigger a secondary authentication step (MFA) for high-value transactions. | Step-up authentication. | **Original**: `SELECT * FROM high_value_orders WHERE amount > 10000;` <br><br> **Rendered**: Challenge "Enter MFA token to proceed." |

---

## 6. Audit & Operations

_Objective: Maintaining visibility, compliance, and system health through comprehensive logging, monitoring, and operational tooling._

| ID | Priority | User Story | Use Case Example | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- |
| **AU-01** | P0 | As an **Auditor**, I want to see both the original query and the rewritten (filtered) query in the logs. | Proving to regulators that filtering logic is actually working. | **Log Entry**: `{ "user": "user-123", "original_query": "SELECT * FROM orders;", "rewritten_query": "SELECT * FROM orders WHERE organization_id = 'tenant-123';", "timestamp": "2023-10-27T10:00:00Z" }` |
| **AU-02** | P1 | As a **DBA** (Platform Admin), I want to monitor the latency added by the proxy's filtering logic. | Ensuring the security layer doesn't slow down the application. | **Metric**: `query_rewrite_latency_ms: 15ms` (Captured and exposed via /metrics). |
| **AU-03** | P0 | As a **DevOps Engineer** (Platform Admin), I want to manage all rules in a YAML file via Git. | Implementing "Security-as-Code" for database access. | Configuration stored in `access_control.yaml` in repo, applied via CI/CD. |
| **AU-04** | P1 | As a **Compliance Officer** (Security & Compliance), I want to log every time a "Break-Glass" access is used. | Accountability for emergency access. | **Log**: `User user-123 used Break-Glass access on table payments at 2023-10-27 02:00:00Z. Reason: "System down investigation".` |
| **AU-05** | P2 | As a **Security Analyst** (Analyst), I want to alert on unusual access patterns (e.g., accessing a table for the first time). | Anomaly detection. | **Alert**: `User user-123 accessed table audit_logs for the first time at 2023-10-27 15:00:00Z.` |
| **AU-06** | P2 | As a **DBA** (Platform Admin), I want to see which columns are being masked most frequently. | Tuning masking rules. | **Dashboard**: `Top Masked Columns: credit_card (1000 hits), ssn (500 hits), cost_price (200 hits).` |
| **AU-07** | P2 | As a **Legal Hold Manager** (Auditor), I want to ensure specific data cannot be deleted even by admins. | Litigation hold. | **Original**: `DELETE FROM emails WHERE id = 'email-evidence';` <br><br> **Rendered**: Error "Legal hold active on this record. Deletion prohibited." |
| **AU-08** | P1 | As a **GDPR Officer** (Security & Compliance), I want to know exactly who accessed a specific customer's PII in the last 30 days. | Right of Access request fulfillment. | **Query**: `SELECT user_id, timestamp FROM audit_logs WHERE table_name = 'customers' AND resource_id = 'cust-123' AND action = 'SELECT' AND timestamp > NOW() - INTERVAL '30 days';` |

---

## 7. Cross-Cutting Concerns

_Advanced scenarios that span multiple categories or involve complex interactions between rules._

| ID | Priority | User Story | Use Case Example | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- |
| **CC-01** | P0 | As a **System Architect** (Platform Admin), I want to define a "Default Deny" policy where users can only see data if an explicit allow rule exists. | Zero Trust model. | **Original**: `SELECT * FROM secret_plans;` (No explicit permission) <br><br> **Rendered**: `SELECT * FROM secret_plans WHERE 1=0;` (Empty result set). |
| **CC-02** | P2 | As a **Delegation Admin** (Platform Admin), I want to temporarily borrow permissions from another user (like a manager). | Acting on behalf of someone else. | **Original**: `SELECT * FROM quarterly_bonus_plans;` (Acting as Manager B) <br><br> **Rendered**: `SELECT * FROM quarterly_bonus_plans WHERE owner_id = 'user-manager-b';` (Using borrowed context). |
| **CC-03** | P0 | As a **Multi-Table Analyst** (Analyst), I want a complex query joining multiple tables to be automatically secured with different rules for each table. | Ad-hoc analytics across the platform. | **Original**: `SELECT o.id, c.name, p.name FROM orders o JOIN customers c ON o.customer_id = c.id JOIN products p ON o.product_id = p.id;` <br><br> **Rendered**: `SELECT o.id, c.name, p.name FROM orders o JOIN customers c ON o.customer_id = c.id JOIN products p ON o.product_id = p.id WHERE o.organization_id = 'tenant-123' AND p.is_public = true;` |
| **CC-04** | P2 | As a **Query Optimizer** (Platform Admin), I want to ensure my security joins don't kill query performance. | Performance safety. | **Rendered**: Proxy rewrites query to use indexed columns for security joins, adding hints or subqueries to minimize full table scans. |
| **CC-05** | P2 | As a **Data Scientist** (Analyst), I want access to aggregated (anonymized) data, but block access to individual row-level PII. | Statistical analysis access. | **Original**: `SELECT * FROM customer_transactions;` <br><br> **Rendered**: `SELECT region, COUNT(*), AVG(amount) FROM customer_transactions GROUP BY region;` (Auto-aggregation applied). |
| **CC-06** | P2 | As a **Bulk Data Importer** (Developer / Engineer), I want to bypass row-level security for loading data (but not for reading). | ETL pipelines. | **Original**: `INSERT INTO customers (id, name) VALUES ('new', 'test');` <br><br> **Rendered**: `INSERT INTO customers (id, name, organization_id) VALUES ('new', 'test', 'tenant-123');` (Tenant ID auto-injected on write). |
| **CC-07** | P1 | As a **Compliance Lead** (Security & Compliance), I want to version control changes to access policies and see who changed what. | Audit trail for permissions. | **History**: `User admin changed policy MT-01 on 2023-10-01 from 'org_id = X' to 'org_id = X OR Y'.` |
| **DF-03** | P2 | As a **Time Traveler** (Analyst), I want to see data as it existed at a specific point in time (Temporal Tables). | Historical reporting. | **Original**: `SELECT * FROM products;` <br><br> **Rendered**: `SELECT * FROM products AS OF TIMESTAMP '2023-01-01';` (System time travel). |
| **DF-04** | P2 | As a **Federation Lead** (Platform Admin), I want to join local tables with remote tables (other databases) while maintaining local security. | Cross-database queries. | **Original**: `SELECT o.*, p.* FROM orders o JOIN remote_db.products p ON o.product_id = p.id;` <br><br> **Rendered**: `SELECT o.*, p.* FROM orders o JOIN remote_db.products p ON o.product_id = p.id WHERE o.organization_id = 'tenant-123';` |
| **DF-05** | P2 | As a **Cache Manager** (Developer / Engineer), I want to invalidate cached query results if the underlying permission rules change. | Real-time security invalidation. | **Event**: Policy MT-01 changed. <br><br> **Action**: Invalidate cache for all queries matching `SELECT * FROM orders`. |

---

## 8. Deprecation, Migration & Compatibility

_Managing the permission system itself, handling legacy scenarios, and ensuring smooth transitions._

| ID | Priority | User Story | Use Case Example | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- |
| **DM-01** | P2 | As a **Migration Lead** (Developer / Engineer), I want to gradually migrate from legacy ACLs to this proxy without downtime. | Blue/Green migration. | **Config**: `mode: hybrid` (Proxy enforces new rules, legacy system still active for verification). |
| **DM-02** | P2 | As a **Legacy User** (External / Partner), I want to continue using my old query patterns even after security policies change. | Backwards compatibility. | **Original**: `SELECT * FROM orders WHERE tenant = 'abc';` <br><br> **Rendered**: `SELECT * FROM orders WHERE organization_id = 'abc';` (Column aliasing). |
| **DM-03** | P2 | As a **QA Lead** (Analyst), I want to test new permission rules against a copy of production traffic without affecting real users. | Shadow mode. | **Original**: `SELECT * FROM payments;` <br><br> **Shadow Result**: New policy would return 100 rows, Old policy returns 100 rows. (Both logged, only Old returned to user). |
| **DM-04** | P2 | As a **Rollout Manager** (Developer / Engineer), I want to enable a new policy for 1% of users first (Canary). | Phased rollout. | **User**: `user-canary-1` (1% sample) <br><br> **Rendered**: New filtering logic applied. <br><br> **Other Users**: Old filtering logic applied. |
| **DM-05** | P1 | As a **Developer** (Developer / Engineer), I want a "verbose" mode that explains WHY a row was filtered or masked in the result. | Debugging permission issues. | **Original**: `SELECT * FROM orders WHERE id = 'order-123';` <br><br> **Result**: `[Empty]` <br><br> **Explanation**: `Reason: No row-level match found. Tried joins: orders->customers (tenant_id match FAILED).` |

---

## 9. Composition Stories

_Scenarios where multiple rules must fire correctly on a single query. These validate that the policy engine combines row filters, column masks, column access controls, and contextual guardrails without conflicts or ordering issues._

| ID | Priority | Title | Rules Combined | SQL Example (E-commerce Context) |
| :-------- | :--- | :--- | :--- | :--- |
| **CO-01** | P1 | **Tenant isolation + column masking** | MT-01 + DS-01 | **Original**: `SELECT id, name, ssn FROM customers;` <br><br> **Rendered**: `SELECT id, name, CONCAT('***-**-', RIGHT(ssn, 4)) AS ssn FROM customers WHERE organization_id = 'org-abc';` (Tenant filter injected AND SSN masked in the same query.) |
| **CO-02** | P1 | **Tenant isolation + column removal + placeholder** | MT-01 + DS-10 + DS-04 | **Original**: `SELECT id, name, ssn, credit_card, email FROM customers;` <br><br> **Rendered**: `SELECT id, name, '[RESTRICTED]' AS ssn, email FROM customers WHERE organization_id = 'org-abc';` (`credit_card` removed entirely; `ssn` replaced with placeholder; `email` passes through; tenant filter added.) |
| **CO-03** | P1 | **Multi-table join with per-table rules** | CC-03 + DS-02 + MT-01 | **Original**: `SELECT o.id, c.name, p.name, p.cost_price FROM orders o JOIN customers c ON o.customer_id = c.id JOIN products p ON o.product_id = p.id;` <br><br> **Rendered**: `SELECT o.id, c.name, p.name, NULL AS cost_price FROM orders o JOIN customers c ON o.customer_id = c.id JOIN products p ON o.product_id = p.id WHERE o.organization_id = 'org-abc' AND p.organization_id = 'org-abc';` (`cost_price` nullified per DS-02; tenant filter applied independently to `orders` and `products` per CC-03 + MT-01.) |
| **CO-04** | P1 | **Deny overrides permit** | CC-01 + DS-01 | **Original**: `SELECT id, name, ssn FROM customers;` (User has a permit policy with SSN masking on `customers`, but a deny policy blocks `customers` entirely for their role.) <br><br> **Rendered**: `SELECT id, name, ssn FROM customers WHERE 1=0;` (Deny wins; masking obligation from DS-01 is never applied because the deny short-circuits.) |
| **CO-05** | P1 | **Break-glass bypasses masking + tenant filter** | CX-01 + MT-01 + DS-01 | **During break-glass window**: `SELECT id, ssn FROM customers;` → `SELECT id, ssn FROM customers;` (No masking, no tenant filter — temporary_access grant is active.) <br><br> **After expiry**: `SELECT id, ssn FROM customers;` → `SELECT id, CONCAT('***-**-', RIGHT(ssn, 4)) AS ssn FROM customers WHERE organization_id = 'org-abc';` (Normal rules resume automatically.) |
| **CO-06** | P1 | **Role-based masking tiers on same query** | DS-05 composite | Two users run `SELECT id, name, ssn FROM customers WHERE id = 'cust-123';` simultaneously. <br><br> **Analyst**: `SELECT id, name, CONCAT('***-**-', RIGHT(ssn, 4)) AS ssn FROM customers WHERE id = 'cust-123';` <br><br> **Manager**: `SELECT id, name, ssn FROM customers WHERE id = 'cust-123';` (Same policy, different obligations matched by role — masking tier varies per user context.) |
| **CO-07** | P1 | **Row filter + result limit** | MT-01 + CX-06 | **Original**: `SELECT * FROM orders;` <br><br> **Rendered**: `SELECT * FROM orders WHERE organization_id = 'org-abc' LIMIT 1000;` (Tenant filter applied first to scope the result set, then the row cap from CX-06 is enforced on the already-filtered query.) |

---

---

## Implementation Status

### P0 (Implemented)

The following stories are implemented in the current release via the policy system:

| Story | Status | Implementation |
|---|---|---|
| DS-01 | ✅ | `column_mask` obligation with partial SSN expression |
| DS-02 | ✅ | `column_access` deny on `cost_price`, `margin` |
| DS-04 | ✅ | `column_mask` obligation with `'[RESTRICTED]'` literal |
| DS-10 | ✅ | `column_access` deny on `ssn`, `credit_card` |
| MT-01 | ✅ | `row_filter` with `organization_id = {user.tenant}` |
| MT-05 | ✅ | Explicit `row_filter: 1=1` policy assigned to admin user |
| MT-06 | ✅ | Multiple row_filter obligations OR'd across permit policies |
| RE-01 | ✅ | `row_filter` with `join_through` for indirect tenant column |
| CC-01 | ✅ | `access_mode: "policy_required"` on datasource |
| CC-03 | ✅ | Per-table row_filter + column_mask applied in a single JOIN query |
| AU-01 | ✅ | `GET /api/v1/audit/queries` with pagination and filtering |
| AU-03 | ⏸ Deferred | Design decision pending: import should call the REST API via HTTP (not bypass it with direct DB writes), so that validation, version snapshots, cache invalidation, and effect validation are handled by the existing handlers. Requires adding `reqwest`, storing the admin base URL in `AdminState`, and forwarding the Bearer token. Export is straightforward but deferred together with import for consistency. |

**Key design decisions for P0:**
- Policies assign directly to users or all users (`user_id = NULL`). No roles/groups.
- `is_admin` grants management API access only — does NOT bypass data policies.
- Datasource `access_mode`: `"open"` (default) or `"policy_required"`.
- `column_access deny` obligations are enforced on **both** permit and deny-effect policies. Deny policies with `row_filter` obligations short-circuit with an error; deny policies with `column_access deny` strip columns silently.
- Disabled policies (`is_enabled: false`) are fully inert — no query-time enforcement, no schema hiding.
- Version snapshots for audit: every policy mutation increments `version` and creates a `policy_version` snapshot.
- Template variables (`{user.tenant}`, etc.) use parse-then-substitute — immune to injection.

### P1 (Deferred)

The following story categories require roles/groups or advanced features not yet implemented:

| Category | Examples | Notes |
|---|---|---|
| Role-based policies | DS-03, DS-05, DS-06, DS-07, DS-08, DS-09 | Requires roles/groups (P1) |
| Group-based policies | MT-02, MT-03, MT-04 | Requires group membership (P1) |
| Dynamic/context-based | CX-01 (break-glass), CX-02, CX-03, CX-04, CX-05, CX-06 | Requires session context / time-based rules (P1) |
| ReBAC | RB-01 through RB-05 | Relationship-based access control (P1+) |
| ABAC | AB-01 through AB-05 | Attribute-based access control (P1) |
| Composition (CO) | All CO stories | Most depend on roles — P1 |

_End of Document_
