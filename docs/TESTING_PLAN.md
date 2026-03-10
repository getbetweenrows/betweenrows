# Testing Strategy & Roadmap

## 1. Core Philosophy: Defense-in-Depth
BetweenRows is a security-critical governance layer. The strategy ensures that no query can bypass policies due to SQL complexity and that the administrative state (policies, users) is always consistent with the proxy's enforcement engine.

## 2. The 4-Tier Strategy

### **Tier 1: Core Engine (Logic & Security Hardening)**
*   **Focus:** `PolicyHook`, `RewriteStatement`, `PolicyMatch`.
*   **Approach:** **Property-based testing** (using `proptest`).
*   **Goal:** Generate thousands of random SQL patterns to verify that if a policy denies a column, that column **never** appears in the resulting logical plan, regardless of query nesting or complexity.

### **Tier 2: Admin API (Consistency & Side Effects)**
*   **Focus:** Axum Handlers (`user`, `policy`, `datasource`).
*   **Approach:** **SQLite In-Memory testing** (decided over `MockDatabase`).
*   **Decision Rationale:** We will continue using SQLite in-memory for 90% of handler tests. This provides **high-fidelity schema validation** and **constraint testing** (e.g., verifying unique indexes and foreign keys) that a MockDatabase would miss.
*   **Goal:** Verify that CRUD operations update the DB and trigger side effects like invalidating the `PolicyHook` cache and rebuilding background `SessionContext`s.

### **Tier 3: Protocol Integration (Compatibility)**
*   **Focus:** PostgreSQL Wire Protocol & Data Source Drivers.
*   **Approach:** Automated E2E tests using **Testcontainers**.
*   **Goal:** Replace the current `#[ignore]` tests in `integration.rs`. Spin up a real Postgres container to verify that complex PG-specific queries from BI tools (like Grafana) work seamlessly and honor RLS filters.

### **Tier 4: UI & Catalog (UX & Async Reliability)**
*   **Focus:** Discovery Wizard & Drift Reporting.
*   **Approach:** Component testing (Vitest) + SSE Mocking.
*   **Goal:** Ensure the UI handles asynchronous discovery jobs correctly and renders "Breaking Change" warnings without blocking the user.
*   **Architecture Dependency:** **Frontend Architecture Guidelines** (logic decoupling, atomic design) must be completed **before** expanding UI tests. This ensures test stability, easier mocking via custom hooks, and isolation of UI primitives.

---

## 3. Implementation Roadmap

### **Phase 1: Admin API Coverage & Frontend Refactor (High Priority)**
*   **Backend:** Implement full coverage for `Policy` and `DataSource` handlers using the established SQLite in-memory pattern.
*   **Frontend:** Execute the **Architecture Guideline** (install `cva`, `cn()` helper, refactor to logic-only hooks) to build a testable foundation.
*   **Roadmap Note:** Explicitly maintain the SQLite in-memory infrastructure to ensure migrations and schema integrity are tested during every API test run.

### **Phase 2: Automated Integration (Medium Priority)**
*   Introduce `testcontainers` to the `Cargo.toml` and refactor `proxy/tests/integration.rs` to run automatically in CI without manual environment setup.

### **Phase 3: Security Regression Suite (High Priority)**
*   Add a specialized "Security Regression" suite that specifically targets RLS bypass attempts using known SQL obfuscation techniques (nested subqueries, CTEs).
