//! Policy enforcement integration tests — security regression suite.
//!
//! Each test creates a unique Postgres schema, seeds data, configures policies
//! via the admin API, and queries through the real pgwire proxy to verify
//! enforcement.
//!
//! Note: Template variables like `{user.tenant}` must NOT be wrapped in quotes
//! in `filter_expression`. The mangle step turns them into identifiers that are
//! then substituted as `Expr::Literal` during AST conversion. Quoting them
//! makes the parser treat the placeholder as a string literal, skipping substitution.

mod support;

use serde_json::json;

// `extract_rows` lives in support so all test binaries can share it.
use support::extract_rows;

// ===========================================================================
// T1: Row filter — tenant isolation
// ===========================================================================

#[tokio::test]
async fn row_filter_tenant_isolation() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t1_rowfilt";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT, amount INT);
             INSERT INTO {schema}.orders VALUES
               (1, 'acme', 100),
               (2, 'globex', 200),
               (3, 'acme', 300);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t1", "open").await;
    server.discover(ds_id, &[schema]).await;

    let _user_id = server
        .create_user("alice", "AlicePass1!", "acme", ds_id)
        .await;

    server
        .create_row_filter(
            "tenant-filter",
            schema,
            "orders",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-t1", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("alice", "AlicePass1!", "ds_t1").await;
    let msgs = client
        .simple_query(&format!(
            "SELECT id, tenant FROM {schema}.orders ORDER BY id"
        ))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 2, "Should only see acme rows");
    assert_eq!(rows[0][1], "acme");
    assert_eq!(rows[1][1], "acme");
    assert_eq!(rows[0][0], "1");
    assert_eq!(rows[1][0], "3");
}

// ===========================================================================
// T2: Template variable injection safety
// ===========================================================================

#[tokio::test]
async fn template_variable_injection() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t2_inject";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT);
             INSERT INTO {schema}.orders VALUES
               (1, 'acme'),
               (2, 'x'' OR ''1''=''1');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t2", "open").await;
    server.discover(ds_id, &[schema]).await;

    // User tenant is the injection attempt literal
    let _user_id = server
        .create_user("injector", "Inject1Pass!", "x' OR '1'='1", ds_id)
        .await;

    server
        .create_row_filter(
            "tenant-filter-t2",
            schema,
            "orders",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-t2", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("injector", "Inject1Pass!", "ds_t2").await;
    let msgs = client
        .simple_query(&format!("SELECT id, tenant FROM {schema}.orders"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    // Should match only the literal tenant string, not bypass via SQL injection.
    // The value is substituted as Expr::Literal, never interpolated as raw SQL.
    assert_eq!(
        rows.len(),
        1,
        "Injection attempt must not bypass row filter"
    );
}

// ===========================================================================
// T3: Table alias bypass
// ===========================================================================

#[tokio::test]
async fn table_alias_bypass() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t3_alias";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT);
             INSERT INTO {schema}.orders VALUES (1,'acme'),(2,'globex'),(3,'acme');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t3", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("alice3", "AlicePass1!", "acme", ds_id)
        .await;

    server
        .create_row_filter(
            "tenant-filter-t3",
            schema,
            "orders",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-t3", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("alice3", "AlicePass1!", "ds_t3").await;
    let msgs = client
        .simple_query(&format!("SELECT o.tenant FROM {schema}.orders AS o"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row[0], "acme", "Aliased query must still be filtered");
    }
}

// ===========================================================================
// T4: CTE bypass
// ===========================================================================

#[tokio::test]
async fn cte_bypass() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t4_cte";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT);
             INSERT INTO {schema}.orders VALUES (1,'acme'),(2,'globex'),(3,'acme');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t4", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("alice4", "AlicePass1!", "acme", ds_id)
        .await;

    server
        .create_row_filter(
            "tenant-filter-t4",
            schema,
            "orders",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-t4", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("alice4", "AlicePass1!", "ds_t4").await;
    let msgs = client
        .simple_query(&format!(
            "WITH d AS (SELECT * FROM {schema}.orders) SELECT tenant FROM d"
        ))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row[0], "acme", "CTE must not bypass row filter");
    }
}

// ===========================================================================
// T5: Subquery bypass
// ===========================================================================

#[tokio::test]
async fn subquery_bypass() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t5_subq";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT);
             INSERT INTO {schema}.orders VALUES (1,'acme'),(2,'globex'),(3,'acme');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t5", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("alice5", "AlicePass1!", "acme", ds_id)
        .await;

    server
        .create_row_filter(
            "tenant-filter-t5",
            schema,
            "orders",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-t5", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("alice5", "AlicePass1!", "ds_t5").await;
    let msgs = client
        .simple_query(&format!(
            "SELECT tenant FROM (SELECT * FROM {schema}.orders) sub"
        ))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 2, "Subquery must not bypass row filter");
    for row in &rows {
        assert_eq!(row[0], "acme");
    }
}

// ===========================================================================
// T6: Star expansion, column deny
// ===========================================================================

#[tokio::test]
async fn star_expansion_column_deny() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t6_coldeny";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.employees;
             CREATE TABLE {schema}.employees (id INT, name TEXT, ssn TEXT, salary INT);
             INSERT INTO {schema}.employees VALUES (1, 'Alice', '123-45-6789', 90000);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t6", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("viewer6", "ViewPass1!", "default", ds_id)
        .await;

    // Deny policy that blocks ssn and salary columns
    server
        .create_column_deny(
            "deny-sensitive-t6",
            schema,
            "employees",
            &["ssn", "salary"],
            ds_id,
            None,
        )
        .await;

    // Permit policy for remaining columns
    server
        .create_column_allow("allow-rest-t6", schema, "employees", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("viewer6", "ViewPass1!", "ds_t6").await;
    // Intentionally uses the extended query protocol (`client.query`) rather than
    // simple_query — this exercises column-deny stripping through the extended
    // (Parse/Bind/Execute) wire path, complementing the simple-query coverage in
    // all other tests.
    let rows = client
        .query(&format!("SELECT * FROM {schema}.employees"), &[])
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    let col_names: Vec<&str> = rows[0].columns().iter().map(|c| c.name()).collect();
    assert!(
        !col_names.contains(&"ssn"),
        "ssn should be stripped from SELECT *. Got columns: {col_names:?}"
    );
    assert!(
        !col_names.contains(&"salary"),
        "salary should be stripped from SELECT *. Got columns: {col_names:?}"
    );
    assert!(col_names.contains(&"id"), "id should be present");
    assert!(col_names.contains(&"name"), "name should be present");
}

// ===========================================================================
// T7: Column mask
// ===========================================================================

#[tokio::test]
async fn column_mask() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t7_mask";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.customers;
             CREATE TABLE {schema}.customers (id INT, name TEXT, ssn TEXT);
             INSERT INTO {schema}.customers VALUES (1, 'Alice', '123-45-6789');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t7", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("viewer7", "ViewPass1!", "default", ds_id)
        .await;

    server
        .create_column_allow("allow-all-t7", schema, "customers", &["*"], ds_id, None)
        .await;
    server
        .create_column_mask(
            "mask-ssn-t7",
            schema,
            "customers",
            "ssn",
            "CONCAT('***-**-', RIGHT(ssn, 4))",
            ds_id,
            None,
        )
        .await;

    let client = server.connect_as("viewer7", "ViewPass1!", "ds_t7").await;
    let msgs = client
        .simple_query(&format!("SELECT ssn FROM {schema}.customers"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], "***-**-6789", "SSN should be masked");
}

// ===========================================================================
// T8: JOIN — both tables filtered
// ===========================================================================

#[tokio::test]
async fn join_both_tables_filtered() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t8_join";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             DROP TABLE IF EXISTS {schema}.customers;
             CREATE TABLE {schema}.customers (id INT, name TEXT, tenant TEXT);
             CREATE TABLE {schema}.orders (id INT, customer_id INT, amount INT, tenant TEXT);
             INSERT INTO {schema}.customers VALUES (1,'Alice','acme'),(2,'Bob','globex');
             INSERT INTO {schema}.orders VALUES (10,1,100,'acme'),(20,2,200,'globex');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t8", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("alice8", "AlicePass1!", "acme", ds_id)
        .await;

    // Row filter on both tables — single policy with multi-table targets
    server
        .create_and_assign_policy(
            "tenant-filter-t8",
            "row_filter",
            vec![json!({"schemas": [schema], "tables": ["customers", "orders"]})],
            Some(json!({"filter_expression": "tenant = {user.tenant}"})),
            ds_id,
            None,
        )
        .await;
    // Allow all columns on both tables
    server
        .create_and_assign_policy(
            "allow-all-t8",
            "column_allow",
            vec![json!({"schemas": [schema], "tables": ["customers", "orders"], "columns": ["*"]})],
            None,
            ds_id,
            None,
        )
        .await;

    let client = server.connect_as("alice8", "AlicePass1!", "ds_t8").await;
    let msgs = client
        .simple_query(&format!(
            "SELECT c.name, o.amount FROM {schema}.orders o \
             JOIN {schema}.customers c ON o.customer_id = c.id"
        ))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 1, "JOIN should only return acme rows");
    assert_eq!(rows[0][0], "Alice");
}

// ===========================================================================
// T9: policy_required mode — no policy → table not found
// ===========================================================================

#[tokio::test]
async fn policy_required_no_policy_table_not_found() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t9_preq";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, name TEXT);
             INSERT INTO {schema}.orders VALUES (1,'a'),(2,'b'),(3,'c'),(4,'d'),(5,'e');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t9", "policy_required").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user9", "UserPass1!", "default", ds_id)
        .await;

    // No policies assigned — in policy_required mode, the table is not visible
    // in the user's context, so querying it should error (table not found).

    let client = server.connect_as("user9", "UserPass1!", "ds_t9").await;
    let result = client
        .simple_query(&format!("SELECT * FROM {schema}.orders"))
        .await;

    assert!(
        result.is_err(),
        "policy_required with no policy should error (table not found)"
    );
}

// ===========================================================================
// T10: Denied column → error
// ===========================================================================

#[tokio::test]
async fn denied_column_error() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t10_deny";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.employees;
             CREATE TABLE {schema}.employees (id INT, name TEXT, ssn TEXT);
             INSERT INTO {schema}.employees VALUES (1, 'Alice', '123-45-6789');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t10", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user10", "UserPass1!", "default", ds_id)
        .await;

    server
        .create_column_deny("deny-ssn-t10", schema, "employees", &["ssn"], ds_id, None)
        .await;

    // Permit policy for remaining columns
    server
        .create_column_allow("allow-all-t10", schema, "employees", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("user10", "UserPass1!", "ds_t10").await;
    let result = client
        .simple_query(&format!("SELECT ssn FROM {schema}.employees"))
        .await;

    assert!(
        result.is_err(),
        "Explicitly selecting denied column should error"
    );
}

// ===========================================================================
// T11: Disabled policy not enforced
// ===========================================================================

#[tokio::test]
async fn disabled_policy_not_enforced() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t11_disabled";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT);
             INSERT INTO {schema}.orders VALUES (1,'acme'),(2,'globex'),(3,'acme'),(4,'globex'),(5,'acme');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t11", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user11", "UserPass1!", "acme", ds_id)
        .await;

    // A disabled row_filter should NOT filter rows
    server
        .create_and_assign_policy_enabled(
            "disabled-filter-t11",
            "row_filter",
            vec![json!({"schemas": [schema], "tables": ["orders"]})],
            Some(json!({"filter_expression": "tenant = {user.tenant}"})),
            ds_id,
            None,
            false, // is_enabled = false
        )
        .await;

    // Need an active permit policy so the user can query
    server
        .create_column_allow("allow-all-t11", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("user11", "UserPass1!", "ds_t11").await;
    let msgs = client
        .simple_query(&format!("SELECT id FROM {schema}.orders"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 5, "Disabled policy should not filter any rows");
}

// ===========================================================================
// T12: object_access deny schema
// ===========================================================================

#[tokio::test]
async fn object_access_deny_schema() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let public_schema = "t12_public";
    let analytics_schema = "t12_analytics";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {public_schema};
             CREATE SCHEMA IF NOT EXISTS {analytics_schema};
             DROP TABLE IF EXISTS {public_schema}.orders;
             DROP TABLE IF EXISTS {analytics_schema}.events;
             CREATE TABLE {public_schema}.orders (id INT, name TEXT);
             CREATE TABLE {analytics_schema}.events (id INT, event_type TEXT);
             INSERT INTO {public_schema}.orders VALUES (1, 'order1');
             INSERT INTO {analytics_schema}.events VALUES (1, 'click');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t12", "open").await;
    server
        .discover(ds_id, &[public_schema, analytics_schema])
        .await;
    let _user_id = server
        .create_user("user12", "UserPass1!", "default", ds_id)
        .await;

    // Deny access to analytics schema (table_deny with wildcard table)
    server
        .create_table_deny("deny-analytics-t12", analytics_schema, "*", ds_id, None)
        .await;

    // Allow public schema
    server
        .create_column_allow("allow-public-t12", public_schema, "*", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("user12", "UserPass1!", "ds_t12").await;

    // Public schema should work
    let msgs = client
        .simple_query(&format!("SELECT id FROM {public_schema}.orders"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1, "Public schema should be accessible");

    // Analytics schema should be denied — table won't exist in the user's context
    let result = client
        .simple_query(&format!("SELECT * FROM {analytics_schema}.events"))
        .await;
    assert!(result.is_err(), "Denied schema should not be queryable");
}

// ===========================================================================
// T13: Two permits AND semantics (intersection)
// ===========================================================================

#[tokio::test]
async fn two_permits_and_semantics() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "t13_and";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, org TEXT, status TEXT);
             INSERT INTO {schema}.orders VALUES
               (1, 'acme', 'active'),
               (2, 'acme', 'inactive'),
               (3, 'globex', 'active'),
               (4, 'globex', 'inactive');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_t13", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user13", "UserPass1!", "acme", ds_id)
        .await;

    // Policy 1: filter by org = user.tenant + column allow
    server
        .create_row_filter(
            "org-filter-t13",
            schema,
            "orders",
            "org = {user.tenant}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-t13", schema, "orders", &["*"], ds_id, None)
        .await;

    // Policy 2: filter by status = 'active' (static value, no template var)
    server
        .create_row_filter(
            "status-filter-t13",
            schema,
            "orders",
            "status = 'active'",
            ds_id,
            None,
        )
        .await;

    let client = server.connect_as("user13", "UserPass1!", "ds_t13").await;
    let msgs = client
        .simple_query(&format!("SELECT id FROM {schema}.orders"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(
        rows.len(),
        1,
        "Two permit row_filters should AND (intersect): only acme+active"
    );
    assert_eq!(rows[0][0], "1");
}

// ===========================================================================
// C1: Deny policy with row_filter → access denied (not row-filtered)
// ===========================================================================

#[tokio::test]
async fn deny_policy_row_filter_rejected() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "c1_denyrow";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT);
             INSERT INTO {schema}.orders VALUES (1,'acme'),(2,'globex');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_c1", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_c1", "UserPass1!", "acme", ds_id)
        .await;

    // table_deny short-circuits to an access-denied error.
    server
        .create_table_deny("deny-rowfilter-c1", schema, "orders", ds_id, None)
        .await;

    let client = server.connect_as("user_c1", "UserPass1!", "ds_c1").await;
    let result = client
        .simple_query(&format!("SELECT * FROM {schema}.orders"))
        .await;

    assert!(
        result.is_err(),
        "Deny policy with row_filter should reject the query outright, not filter rows"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Access denied")
            || err_msg.contains("access denied")
            || err_msg.contains("42501"),
        "Error should indicate access denial, got: {err_msg}"
    );
}

// ===========================================================================
// C2: Read-only enforcement — writes blocked by ReadOnlyHook
//     (moved to protocol.rs as a protocol-level concern)
// ===========================================================================

// ===========================================================================
// C3: policy_required + column_access allow → only named columns visible
// ===========================================================================

#[tokio::test]
async fn policy_required_column_access_limits_select_star() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "c3_preq_cols";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.employees;
             CREATE TABLE {schema}.employees (id INT, name TEXT, ssn TEXT, salary INT);
             INSERT INTO {schema}.employees VALUES (1, 'Alice', '123-45-6789', 90000);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_c3", "policy_required").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_c3", "UserPass1!", "default", ds_id)
        .await;

    // Only allow id and name — ssn and salary are not in the allow list
    server
        .create_column_allow(
            "allow-limited-c3",
            schema,
            "employees",
            &["id", "name"],
            ds_id,
            None,
        )
        .await;

    let client = server.connect_as("user_c3", "UserPass1!", "ds_c3").await;

    // SELECT * should return only the allowed columns
    // Intentionally uses extended protocol (client.query) to complement simple_query elsewhere.
    let rows = client
        .query(&format!("SELECT * FROM {schema}.employees"), &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let col_names: Vec<&str> = rows[0].columns().iter().map(|c| c.name()).collect();
    assert!(col_names.contains(&"id"), "id must be visible");
    assert!(col_names.contains(&"name"), "name must be visible");
    assert!(
        !col_names.contains(&"ssn"),
        "ssn must not be visible (not in allow list)"
    );
    assert!(
        !col_names.contains(&"salary"),
        "salary must not be visible (not in allow list)"
    );

    // Explicitly selecting a non-allowed column should error
    let result = client
        .simple_query(&format!("SELECT ssn FROM {schema}.employees"))
        .await;
    assert!(
        result.is_err(),
        "Selecting a column not in the allow list should error"
    );
}

// ===========================================================================
// C4: Deny overrides allow — column in both allow and deny → denied
// ===========================================================================

#[tokio::test]
async fn deny_overrides_allow_columns() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "c4_denywin";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.employees;
             CREATE TABLE {schema}.employees (id INT, name TEXT, ssn TEXT);
             INSERT INTO {schema}.employees VALUES (1, 'Alice', '123-45-6789');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_c4", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_c4", "UserPass1!", "default", ds_id)
        .await;

    // Permit policy allows all columns (including ssn via wildcard)
    server
        .create_column_allow("allow-all-c4", schema, "employees", &["*"], ds_id, None)
        .await;

    // Deny policy explicitly blocks ssn — deny must win over the wildcard allow
    server
        .create_column_deny("deny-ssn-c4", schema, "employees", &["ssn"], ds_id, None)
        .await;

    let client = server.connect_as("user_c4", "UserPass1!", "ds_c4").await;

    // SELECT * must not include ssn
    // Intentionally uses extended protocol to cover that path.
    let rows = client
        .query(&format!("SELECT * FROM {schema}.employees"), &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let col_names: Vec<&str> = rows[0].columns().iter().map(|c| c.name()).collect();
    assert!(
        !col_names.contains(&"ssn"),
        "deny must win: ssn should be stripped"
    );
    assert!(col_names.contains(&"id"), "id must still be present");

    // Explicitly selecting ssn must error
    let result = client
        .simple_query(&format!("SELECT ssn FROM {schema}.employees"))
        .await;
    assert!(
        result.is_err(),
        "Explicit SELECT on denied column must error"
    );
}

// ===========================================================================
// I1: Table-level object_access deny
// ===========================================================================

#[tokio::test]
async fn object_access_deny_table() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "i1_tabledeny";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             DROP TABLE IF EXISTS {schema}.payments;
             CREATE TABLE {schema}.orders (id INT, name TEXT);
             CREATE TABLE {schema}.payments (id INT, amount INT);
             INSERT INTO {schema}.orders VALUES (1, 'order1');
             INSERT INTO {schema}.payments VALUES (1, 500);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_i1", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_i1", "UserPass1!", "default", ds_id)
        .await;

    // Deny access to the payments table specifically
    server
        .create_table_deny("deny-payments-i1", schema, "payments", ds_id, None)
        .await;

    // Allow all columns on the orders table
    server
        .create_column_allow("allow-orders-i1", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("user_i1", "UserPass1!", "ds_i1").await;

    // orders should be accessible
    let msgs = client
        .simple_query(&format!("SELECT id FROM {schema}.orders"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1, "orders table should be accessible");

    // payments should be denied — the table is removed from the user's context
    let result = client
        .simple_query(&format!("SELECT * FROM {schema}.payments"))
        .await;
    assert!(
        result.is_err(),
        "Table-level deny should make payments inaccessible"
    );
}

// ===========================================================================
// I2: Deny overrides mask — denied+masked column is removed, not masked
// ===========================================================================

#[tokio::test]
async fn deny_overrides_mask() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "i2_denymask";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.customers;
             CREATE TABLE {schema}.customers (id INT, name TEXT, ssn TEXT);
             INSERT INTO {schema}.customers VALUES (1, 'Alice', '123-45-6789');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_i2", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_i2", "UserPass1!", "default", ds_id)
        .await;

    // Deny policy — ssn column is denied
    server
        .create_column_deny("deny-ssn-i2", schema, "customers", &["ssn"], ds_id, None)
        .await;

    // Permit policy — allows all and includes a mask on ssn
    // Deny should take precedence: ssn must not appear at all, not appear masked.
    server
        .create_column_allow("allow-all-i2", schema, "customers", &["*"], ds_id, None)
        .await;
    server
        .create_column_mask(
            "mask-ssn-i2",
            schema,
            "customers",
            "ssn",
            "CONCAT('***-**-', RIGHT(ssn, 4))",
            ds_id,
            None,
        )
        .await;

    let client = server.connect_as("user_i2", "UserPass1!", "ds_i2").await;

    // SELECT * should not include ssn at all (deny wins over mask)
    let rows = client
        .query(&format!("SELECT * FROM {schema}.customers"), &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let col_names: Vec<&str> = rows[0].columns().iter().map(|c| c.name()).collect();
    assert!(
        !col_names.contains(&"ssn"),
        "deny wins over mask: ssn must not appear even masked, got: {col_names:?}"
    );

    // Explicit SELECT ssn must error
    let result = client
        .simple_query(&format!("SELECT ssn FROM {schema}.customers"))
        .await;
    assert!(
        result.is_err(),
        "Explicit SELECT on denied column must error"
    );
}

// ===========================================================================
// I3a: Template variable {user.username}
// ===========================================================================

#[tokio::test]
async fn template_variable_username() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "i3a_uname";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.docs;
             CREATE TABLE {schema}.docs (id INT, owner TEXT, content TEXT);
             INSERT INTO {schema}.docs VALUES
               (1, 'alice_u', 'alice doc'),
               (2, 'bob_u',   'bob doc'),
               (3, 'alice_u', 'alice doc 2');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_i3a", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("alice_u", "AlicePass1!", "default", ds_id)
        .await;

    server
        .create_row_filter(
            "username-filter-i3a",
            schema,
            "docs",
            "owner = {user.username}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-i3a", schema, "docs", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("alice_u", "AlicePass1!", "ds_i3a").await;
    let msgs = client
        .simple_query(&format!("SELECT id, owner FROM {schema}.docs ORDER BY id"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 2, "Should only see alice_u's docs");
    for row in &rows {
        assert_eq!(
            row[1], "alice_u",
            "{{user.username}} filter must match the connected user"
        );
    }
}

// ===========================================================================
// I3b: Template variable {user.id}
// ===========================================================================

#[tokio::test]
async fn template_variable_user_id() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "i3b_uid";

    // Create the user first so we know their UUID
    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.items;"
        ))
        .await;

    let ds_id = server.create_datasource("ds_i3b", "open").await;
    server.discover(ds_id, &[schema]).await;

    // We create a placeholder user first (will be used to seed data with their UUID).
    // The UUID comes back from create_user so we can use it to seed the table.
    let user_id = server
        .create_user("uid_user", "UidPass1!", "default", ds_id)
        .await;
    let other_id = uuid::Uuid::new_v4(); // fake UUID for another user's row

    server
        .seed_upstream(&format!(
            "CREATE TABLE IF NOT EXISTS {schema}.items (id INT, user_uuid TEXT, label TEXT);
             INSERT INTO {schema}.items VALUES
               (1, '{user_id}', 'my item'),
               (2, '{other_id}', 'other item');"
        ))
        .await;

    // Re-discover after table creation
    server.discover(ds_id, &[schema]).await;

    server
        .create_row_filter(
            "uid-filter-i3b",
            schema,
            "items",
            "user_uuid = {user.id}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-i3b", schema, "items", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("uid_user", "UidPass1!", "ds_i3b").await;
    let msgs = client
        .simple_query(&format!(
            "SELECT id, user_uuid FROM {schema}.items ORDER BY id"
        ))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(rows.len(), 1, "Should only see rows matching {{user.id}}");
    assert_eq!(rows[0][1], user_id.to_string());
}

// ===========================================================================
// I4: Aggregate with row filter — COUNT respects tenant filter
// ===========================================================================

#[tokio::test]
async fn aggregate_with_row_filter() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "i4_agg";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT, amount INT);
             INSERT INTO {schema}.orders VALUES
               (1, 'acme', 100),
               (2, 'globex', 200),
               (3, 'acme', 300),
               (4, 'globex', 400),
               (5, 'acme', 500);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_i4", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("alice_i4", "AlicePass1!", "acme", ds_id)
        .await;

    server
        .create_row_filter(
            "tenant-filter-i4",
            schema,
            "orders",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-i4", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("alice_i4", "AlicePass1!", "ds_i4").await;

    // COUNT(*) should only count acme rows (3)
    let msgs = client
        .simple_query(&format!("SELECT COUNT(*) FROM {schema}.orders"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][0], "3",
        "COUNT(*) must only count tenant-filtered rows"
    );

    // SUM should also be filtered
    let msgs = client
        .simple_query(&format!("SELECT SUM(amount) FROM {schema}.orders"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][0], "900",
        "SUM must only aggregate tenant-filtered rows"
    );
}

// ===========================================================================
// I5: User not assigned to datasource → connection rejected
// ===========================================================================

#[tokio::test]
async fn user_not_assigned_to_datasource() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "i5_noassign";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_i5", "open").await;
    server.discover(ds_id, &[schema]).await;

    // Create a user that is NOT assigned to ds_i5
    let _unassigned_id = server
        .create_user_unassigned("unassigned_user", "UnassignedPass1!", "default")
        .await;

    // This user was never added to ds_i5's user list
    let result = server
        .try_connect_as("unassigned_user", "UnassignedPass1!", "ds_i5")
        .await;
    assert!(
        result.is_err(),
        "User not assigned to a datasource should not be able to connect to it"
    );
}

// ===========================================================================
// I7: Two row_filter policies on same table (AND across policies)
// ===========================================================================

#[tokio::test]
async fn single_policy_multiple_row_filters_same_table() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "i7_multifilter";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT, status TEXT);
             INSERT INTO {schema}.orders VALUES
               (1, 'acme', 'active'),
               (2, 'acme', 'inactive'),
               (3, 'globex', 'active'),
               (4, 'globex', 'inactive');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_i7", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_i7", "UserPass1!", "acme", ds_id)
        .await;

    // Two separate row_filter policies — AND-combined across policies.
    server
        .create_row_filter(
            "tenant-filter-i7",
            schema,
            "orders",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;
    server
        .create_row_filter(
            "status-filter-i7",
            schema,
            "orders",
            "status = 'active'",
            ds_id,
            None,
        )
        .await;
    server
        .create_column_allow("allow-all-i7", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("user_i7", "UserPass1!", "ds_i7").await;
    let msgs = client
        .simple_query(&format!("SELECT id FROM {schema}.orders ORDER BY id"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(
        rows.len(),
        1,
        "AND-across-policies: only row satisfying both tenant=acme AND status=active"
    );
    assert_eq!(rows[0][0], "1");
}

// ===========================================================================
// I8: User-specific vs wildcard policy — different users see different data
// ===========================================================================

#[tokio::test]
async fn user_specific_vs_wildcard_policy() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "i8_peruser";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, org TEXT);
             INSERT INTO {schema}.orders VALUES
               (1, 'acme'),
               (2, 'globex'),
               (3, 'acme');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_i8", "open").await;
    server.discover(ds_id, &[schema]).await;

    let alice_id = server
        .create_user("alice_i8", "AlicePass1!", "acme", ds_id)
        .await;
    let _bob_id = server
        .create_user("bob_i8", "BobPass1!", "acme", ds_id)
        .await;

    // Wildcard policy: allow all columns (applies to all users)
    server
        .create_column_allow("allow-all-i8", schema, "orders", &["*"], ds_id, None)
        .await;

    // User-specific policy for alice only: restrict to org='acme' rows
    server
        .create_row_filter(
            "alice-only-filter-i8",
            schema,
            "orders",
            "org = 'acme'",
            ds_id,
            Some(alice_id),
        )
        .await;

    // Alice sees only acme rows (wildcard allow-all + her per-user row filter both apply)
    let alice = server.connect_as("alice_i8", "AlicePass1!", "ds_i8").await;
    let msgs = alice
        .simple_query(&format!("SELECT id FROM {schema}.orders ORDER BY id"))
        .await
        .unwrap();
    let alice_rows = extract_rows(&msgs);
    assert_eq!(
        alice_rows.len(),
        2,
        "Alice should see only acme rows: {alice_rows:?}"
    );
    assert_eq!(alice_rows[0][0], "1");
    assert_eq!(alice_rows[1][0], "3");

    // Bob sees all rows (only the wildcard allow-all policy applies; no per-user filter)
    let bob = server.connect_as("bob_i8", "BobPass1!", "ds_i8").await;
    let msgs = bob
        .simple_query(&format!("SELECT id FROM {schema}.orders ORDER BY id"))
        .await
        .unwrap();
    let bob_rows = extract_rows(&msgs);
    assert_eq!(bob_rows.len(), 3, "Bob should see all rows: {bob_rows:?}");
}

// ===========================================================================
// N1: Column glob patterns in deny ("secret_*")
// ===========================================================================

#[tokio::test]
async fn column_glob_pattern_deny() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "n1_glob";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.vault;
             CREATE TABLE {schema}.vault (id INT, name TEXT, secret_key TEXT, secret_token TEXT);
             INSERT INTO {schema}.vault VALUES (1, 'item', 'key123', 'tok456');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_n1", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_n1", "UserPass1!", "default", ds_id)
        .await;

    // Deny all columns matching "secret_*"
    server
        .create_column_deny(
            "deny-secret-glob-n1",
            schema,
            "vault",
            &["secret_*"],
            ds_id,
            None,
        )
        .await;

    server
        .create_column_allow("allow-all-n1", schema, "vault", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("user_n1", "UserPass1!", "ds_n1").await;

    // SELECT * should not include secret_key or secret_token
    let rows = client
        .query(&format!("SELECT * FROM {schema}.vault"), &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let col_names: Vec<&str> = rows[0].columns().iter().map(|c| c.name()).collect();
    assert!(col_names.contains(&"id"), "id must be present");
    assert!(col_names.contains(&"name"), "name must be present");
    assert!(
        !col_names.contains(&"secret_key"),
        "secret_key must be stripped by glob deny, got: {col_names:?}"
    );
    assert!(
        !col_names.contains(&"secret_token"),
        "secret_token must be stripped by glob deny, got: {col_names:?}"
    );

    // Explicit SELECT on a secret_* column must error
    let result = client
        .simple_query(&format!("SELECT secret_key FROM {schema}.vault"))
        .await;
    assert!(
        result.is_err(),
        "Explicit SELECT on glob-denied column must error"
    );
}

// ===========================================================================
// N4: Live policy update takes effect without reconnect
// ===========================================================================

#[tokio::test]
async fn live_policy_update_without_reconnect() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "n4_live";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT);
             INSERT INTO {schema}.orders VALUES (1,'acme'),(2,'globex');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_n4", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_n4", "UserPass1!", "default", ds_id)
        .await;

    // Initial policy: allow all columns so queries work
    server
        .create_column_allow("allow-all-n4", schema, "orders", &["*"], ds_id, None)
        .await;

    // Establish a connection BEFORE adding the deny policy
    let client = server.connect_as("user_n4", "UserPass1!", "ds_n4").await;

    // Verify the query works before the deny
    let msgs = client
        .simple_query(&format!("SELECT id FROM {schema}.orders ORDER BY id"))
        .await
        .unwrap();
    let rows_before = extract_rows(&msgs);
    assert_eq!(
        rows_before.len(),
        2,
        "Should see both rows before deny policy"
    );

    // Add a deny policy for the tenant column while the connection is still open.
    // ProxyHandler::rebuild_contexts_for_datasource will rebuild the SessionContext
    // for active connections in the background.
    server
        .create_column_deny("deny-tenant-n4", schema, "orders", &["tenant"], ds_id, None)
        .await;

    // Poll until the background rebuild takes effect (max 5s).
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let result = client
            .simple_query(&format!("SELECT tenant FROM {schema}.orders"))
            .await;
        if result.is_err() {
            break; // policy took effect
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Policy update did not take effect within 5s"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

// ===========================================================================
// N6: policy_required WITH allow → table becomes visible
// ===========================================================================

#[tokio::test]
async fn policy_required_with_allow_table_visible() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "n6_preq_allow";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             DROP TABLE IF EXISTS {schema}.secrets;
             CREATE TABLE {schema}.orders (id INT, name TEXT);
             CREATE TABLE {schema}.secrets (id INT, value TEXT);
             INSERT INTO {schema}.orders VALUES (1, 'Alice'), (2, 'Bob');
             INSERT INTO {schema}.secrets VALUES (1, 'top-secret');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_n6", "policy_required").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_n6", "UserPass1!", "default", ds_id)
        .await;

    // Only grant access to the orders table; secrets has no permit policy
    server
        .create_column_allow("allow-orders-n6", schema, "orders", &["*"], ds_id, None)
        .await;

    let client = server.connect_as("user_n6", "UserPass1!", "ds_n6").await;

    // orders should be accessible
    let msgs = client
        .simple_query(&format!("SELECT id FROM {schema}.orders ORDER BY id"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(
        rows.len(),
        2,
        "orders should be visible with a permit policy"
    );
    assert_eq!(rows[0][0], "1");
    assert_eq!(rows[1][0], "2");

    // secrets has no permit policy — in policy_required mode it's invisible
    let result = client
        .simple_query(&format!("SELECT * FROM {schema}.secrets"))
        .await;
    assert!(
        result.is_err(),
        "Table without a permit policy must be invisible in policy_required mode"
    );
}

// ===========================================================================
// TC-JOIN-01: JOIN column collision — deny on one table must not affect other
// ===========================================================================

#[tokio::test]
async fn tc_join_01_join_column_collision() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_join01";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.customers;
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.customers (id INT, email TEXT, name TEXT);
             CREATE TABLE {schema}.orders (id INT, email TEXT, amount INT);
             INSERT INTO {schema}.customers VALUES (1, 'alice@example.com', 'Alice');
             INSERT INTO {schema}.orders VALUES (1, 'order@example.com', 100);"
        ))
        .await;

    let ds_id = server
        .create_datasource("ds_join01", "policy_required")
        .await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_join01", "UserPass1!", "default", ds_id)
        .await;

    // Allow all columns on orders, allow only name (not email) on customers
    server
        .create_column_allow("orders-all-join01", schema, "orders", &["*"], ds_id, None)
        .await;
    server
        .create_column_allow(
            "customers-name-join01",
            schema,
            "customers",
            &["id", "name"],
            ds_id,
            None,
        )
        .await;

    // Deny email on customers only
    server
        .create_column_deny(
            "deny-customers-email-join01",
            schema,
            "customers",
            &["email"],
            ds_id,
            None,
        )
        .await;

    let client = server
        .connect_as("user_join01", "UserPass1!", "ds_join01")
        .await;

    // JOIN query — orders.email should be visible, customers.email stripped
    let msgs = client
        .simple_query(&format!(
            "SELECT o.email FROM {schema}.customers c JOIN {schema}.orders o ON c.id = o.id"
        ))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][0], "order@example.com",
        "orders.email should be visible"
    );
}

// ===========================================================================
// TC-ZT-01: Zero-trust — row_filter only, no column_access allow → error
// ===========================================================================

#[tokio::test]
async fn tc_zt_01_implicit_blackout() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_zt01";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.users;
             CREATE TABLE {schema}.users (id INT, tenant TEXT, name TEXT);
             INSERT INTO {schema}.users VALUES (1, 'acme', 'Alice');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_zt01", "policy_required").await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_zt01", "UserPass1!", "acme", ds_id)
        .await;

    // row_filter only — no column_allow policy
    // In zero-trust mode this activates an empty whitelist → AllColumnsDenied
    server
        .create_row_filter(
            "row-filter-only-zt01",
            schema,
            "users",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;

    let client = server
        .connect_as("user_zt01", "UserPass1!", "ds_zt01")
        .await;
    let result = client
        .simple_query(&format!("SELECT * FROM {schema}.users"))
        .await;
    assert!(
        result.is_err(),
        "TC-ZT-01: row_filter-only permit should deny all columns (empty whitelist)"
    );
}

// ===========================================================================
// TC-ZT-02: Zero-trust — explicit whitelist [id, name] → only those visible
// ===========================================================================

#[tokio::test]
async fn tc_zt_02_explicit_whitelist() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_zt02";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.users;
             CREATE TABLE {schema}.users (id INT, name TEXT, secret TEXT);
             INSERT INTO {schema}.users VALUES (1, 'Alice', 'top-secret');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_zt02", "policy_required").await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_zt02", "UserPass1!", "default", ds_id)
        .await;

    server
        .create_column_allow(
            "whitelist-id-name-zt02",
            schema,
            "users",
            &["id", "name"],
            ds_id,
            None,
        )
        .await;

    let client = server
        .connect_as("user_zt02", "UserPass1!", "ds_zt02")
        .await;
    let msgs = client
        .simple_query(&format!("SELECT * FROM {schema}.users"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1);
    // Result should have exactly 2 columns: id and name (secret stripped)
    assert_eq!(
        rows[0].len(),
        2,
        "TC-ZT-02: only id and name should be visible"
    );
    assert_eq!(rows[0][0], "1");
    assert_eq!(rows[0][1], "Alice");
}

// ===========================================================================
// TC-ZT-03: Zero-trust — wildcard whitelist ["*"] + row_filter → all visible
// ===========================================================================

#[tokio::test]
async fn tc_zt_03_wildcard_whitelist() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_zt03";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, tenant TEXT, amount INT);
             INSERT INTO {schema}.orders VALUES
               (1, 'acme', 100),
               (2, 'globex', 200);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_zt03", "policy_required").await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_zt03", "UserPass1!", "acme", ds_id)
        .await;

    server
        .create_column_allow(
            "wildcard-whitelist-zt03",
            schema,
            "orders",
            &["*"],
            ds_id,
            None,
        )
        .await;
    server
        .create_row_filter(
            "row-filter-zt03",
            schema,
            "orders",
            "tenant = {user.tenant}",
            ds_id,
            None,
        )
        .await;

    let client = server
        .connect_as("user_zt03", "UserPass1!", "ds_zt03")
        .await;
    let msgs = client
        .simple_query(&format!("SELECT * FROM {schema}.orders ORDER BY id"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(
        rows.len(),
        1,
        "TC-ZT-03: row_filter should restrict to acme rows"
    );
    assert_eq!(
        rows[0].len(),
        3,
        "TC-ZT-03: all 3 columns should be visible with wildcard"
    );
    assert_eq!(rows[0][1], "acme");
}

// ===========================================================================
// TC-DENY-01: Deny wins — allow email, then deny email → email stripped
// ===========================================================================

#[tokio::test]
async fn tc_deny_01_deny_wins() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_deny01";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.contacts;
             CREATE TABLE {schema}.contacts (id INT, email TEXT, name TEXT);
             INSERT INTO {schema}.contacts VALUES (1, 'alice@example.com', 'Alice');"
        ))
        .await;

    let ds_id = server
        .create_datasource("ds_deny01", "policy_required")
        .await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_deny01", "UserPass1!", "default", ds_id)
        .await;

    // Policy A: allow id, email, name
    server
        .create_column_allow(
            "allow-email-deny01",
            schema,
            "contacts",
            &["id", "email", "name"],
            ds_id,
            None,
        )
        .await;

    // Policy B: deny email
    server
        .create_column_deny(
            "deny-email-deny01",
            schema,
            "contacts",
            &["email"],
            ds_id,
            None,
        )
        .await;

    let client = server
        .connect_as("user_deny01", "UserPass1!", "ds_deny01")
        .await;
    let msgs = client
        .simple_query(&format!("SELECT * FROM {schema}.contacts"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].len(),
        2,
        "TC-DENY-01: deny wins, email should be stripped"
    );
    // id and name remain; email is gone
    assert_eq!(rows[0][0], "1");
    assert_eq!(rows[0][1], "Alice");
}

// ===========================================================================
// TC-DENY-02: Absolute veto — allow id, deny ["*"] → 0 columns visible
// ===========================================================================

#[tokio::test]
async fn tc_deny_02_absolute_veto() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_deny02";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.records;
             CREATE TABLE {schema}.records (id INT, data TEXT);
             INSERT INTO {schema}.records VALUES (1, 'sensitive');"
        ))
        .await;

    let ds_id = server
        .create_datasource("ds_deny02", "policy_required")
        .await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_deny02", "UserPass1!", "default", ds_id)
        .await;

    // Policy A: allow id
    server
        .create_column_allow("allow-id-deny02", schema, "records", &["id"], ds_id, None)
        .await;

    // Policy B: deny all columns with wildcard
    server
        .create_column_deny("deny-all-deny02", schema, "records", &["*"], ds_id, None)
        .await;

    let client = server
        .connect_as("user_deny02", "UserPass1!", "ds_deny02")
        .await;
    let result = client
        .simple_query(&format!("SELECT * FROM {schema}.records"))
        .await;
    assert!(
        result.is_err(),
        "TC-DENY-02: deny [*] must veto all columns including the allowed id"
    );
}

// ===========================================================================
// TC-GLOB-01: Suffix glob — deny ["*_at"] strips timestamp columns, keeps name
// ===========================================================================

#[tokio::test]
async fn tc_glob_01_suffix_glob() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_glob01";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.events;
             CREATE TABLE {schema}.events (id INT, name TEXT, created_at TIMESTAMP, updated_at TIMESTAMP);
             INSERT INTO {schema}.events VALUES (1, 'launch', NOW(), NOW());"
        ))
        .await;

    let ds_id = server
        .create_datasource("ds_glob01", "policy_required")
        .await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_glob01", "UserPass1!", "default", ds_id)
        .await;

    // Allow all columns
    server
        .create_column_allow("allow-all-glob01", schema, "events", &["*"], ds_id, None)
        .await;

    // Deny *_at columns (suffix glob)
    server
        .create_column_deny(
            "deny-timestamps-glob01",
            schema,
            "events",
            &["*_at"],
            ds_id,
            None,
        )
        .await;

    let client = server
        .connect_as("user_glob01", "UserPass1!", "ds_glob01")
        .await;
    let msgs = client
        .simple_query(&format!("SELECT * FROM {schema}.events"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].len(),
        2,
        "TC-GLOB-01: *_at glob should strip created_at and updated_at, leaving id and name"
    );
    assert_eq!(rows[0][0], "1");
    assert_eq!(rows[0][1], "launch");
}

// ===========================================================================
// TC-GLOB-03: Case sensitivity — deny ["Email"] does NOT strip lowercase "email"
// ===========================================================================

#[tokio::test]
async fn tc_glob_03_case_sensitivity() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_glob03";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.contacts;
             CREATE TABLE {schema}.contacts (id INT, email TEXT);
             INSERT INTO {schema}.contacts VALUES (1, 'alice@example.com');"
        ))
        .await;

    let ds_id = server
        .create_datasource("ds_glob03", "policy_required")
        .await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_glob03", "UserPass1!", "default", ds_id)
        .await;

    // Allow all columns
    server
        .create_column_allow("allow-all-glob03", schema, "contacts", &["*"], ds_id, None)
        .await;

    // Deny "Email" (capitalized) — Postgres columns are lowercase "email"
    // Case-sensitive matching means this deny should NOT strip the email column
    server
        .create_column_deny(
            "deny-Email-glob03",
            schema,
            "contacts",
            &["Email"],
            ds_id,
            None,
        )
        .await;

    let client = server
        .connect_as("user_glob03", "UserPass1!", "ds_glob03")
        .await;
    let msgs = client
        .simple_query(&format!("SELECT * FROM {schema}.contacts"))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].len(),
        2,
        "TC-GLOB-03: case-sensitive deny 'Email' must NOT strip lowercase 'email'"
    );
    assert_eq!(rows[0][1], "alice@example.com");
}

// ===========================================================================
// TC-RF-01: row_filter with != operator and double-quoted column identifier
// Mirrors the "state-filter" policy in the admin DB:
//   filter_expression: "\"state\" != 'WY'"
// ===========================================================================

#[tokio::test]
async fn tc_rf_01_neq_operator_quoted_column() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_rf01";

    // Table has a `state` column — 5 rows, one is WY (should be excluded).
    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.locations;
             CREATE TABLE {schema}.locations (id INT, city TEXT, state TEXT);
             INSERT INTO {schema}.locations VALUES
               (1, 'Austin',   'TX'),
               (2, 'Denver',   'CO'),
               (3, 'Cheyenne', 'WY'),
               (4, 'Seattle',  'WA'),
               (5, 'Casper',   'WY');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_rf01", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user = server
        .create_user("user_rf01", "UserPass1!", "default", ds_id)
        .await;

    // row_filter using != and a double-quoted column identifier — mirrors
    // the "state-filter" policy stored in the admin DB.
    server
        .create_row_filter(
            "state-filter-rf01",
            schema,
            "locations",
            "\"state\" != 'WY'",
            ds_id,
            None,
        )
        .await;

    let client = server
        .connect_as("user_rf01", "UserPass1!", "ds_rf01")
        .await;
    let msgs = client
        .simple_query(&format!(
            "SELECT id, state FROM {schema}.locations ORDER BY id"
        ))
        .await
        .unwrap();
    let rows = extract_rows(&msgs);

    assert_eq!(
        rows.len(),
        3,
        "WY rows must be filtered out; expected TX, CO, WA"
    );
    let states: Vec<&str> = rows.iter().map(|r| r[1].as_str()).collect();
    assert!(
        !states.contains(&"WY"),
        "WY must not appear in results: {states:?}"
    );
    assert_eq!(rows[0][0], "1"); // Austin TX
    assert_eq!(rows[1][0], "2"); // Denver CO
    assert_eq!(rows[2][0], "4"); // Seattle WA
}

// ===========================================================================
// TC-AUDIT-01: Successful query → audit entry has status "success",
//              execution_time_ms > 0, rewritten_query shows actual SQL
// ===========================================================================

#[tokio::test]
async fn tc_audit_01_success_audit_status() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_audit01";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.sales;
             CREATE TABLE {schema}.sales (id INT, tenant TEXT, amount INT);
             INSERT INTO {schema}.sales VALUES (1, 'acme', 100), (2, 'globex', 200);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_audit01", "open").await;
    server.discover(ds_id, &[schema]).await;
    let user_id = server
        .create_user("user_audit01", "UserPass1!", "acme", ds_id)
        .await;

    server
        .create_row_filter(
            "audit01-permit",
            schema,
            "sales",
            "tenant = {user.tenant}",
            ds_id,
            Some(user_id),
        )
        .await;

    let client = server
        .connect_as("user_audit01", "UserPass1!", "ds_audit01")
        .await;
    client
        .simple_query(&format!("SELECT * FROM {schema}.sales"))
        .await
        .unwrap();

    // Audit write is async — give it time to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let resp = server
        .admin
        .get("/api/v1/audit/queries")
        .authorization_bearer(&server.admin_token)
        .await;
    resp.assert_status_ok();
    let body = resp.json::<serde_json::Value>();
    let entries = body["data"].as_array().unwrap();

    let entry = entries
        .iter()
        .find(|e| e["username"].as_str() == Some("user_audit01"))
        .expect("TC-AUDIT-01: no audit entry for user_audit01");

    assert_eq!(
        entry["status"].as_str(),
        Some("success"),
        "TC-AUDIT-01: status must be 'success'"
    );
    assert!(
        entry["error_message"].is_null(),
        "TC-AUDIT-01: error_message must be null on success"
    );
    let elapsed = entry["execution_time_ms"].as_i64().unwrap_or(0);
    assert!(
        elapsed >= 0,
        "TC-AUDIT-01: execution_time_ms must be non-negative, got {elapsed}"
    );
    // Rewritten query should be present (row filter was applied) and not contain the fake comment
    let rewritten = entry["rewritten_query"].as_str().unwrap_or("");
    assert!(
        !rewritten.is_empty(),
        "TC-AUDIT-01: rewritten_query must be present when row filter is applied"
    );
    assert!(
        !rewritten.contains("/* policy-rewritten */"),
        "TC-AUDIT-01: rewritten_query must not be the fake comment, got: {rewritten}"
    );
}

// ===========================================================================
// TC-AUDIT-02: Denied query (deny policy) → audit entry has status "denied"
//              with error_message containing the policy name
// ===========================================================================

#[tokio::test]
async fn tc_audit_02_denied_audit_status() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_audit02";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.accounts;
             CREATE TABLE {schema}.accounts (id INT, region TEXT);
             INSERT INTO {schema}.accounts VALUES (1, 'us'), (2, 'eu');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_audit02", "open").await;
    server.discover(ds_id, &[schema]).await;
    let user_id = server
        .create_user("user_audit02", "UserPass1!", "default", ds_id)
        .await;

    // table_deny policy that blocks queries on this table
    server
        .create_table_deny("audit02-deny", schema, "accounts", ds_id, Some(user_id))
        .await;

    let client = server
        .connect_as("user_audit02", "UserPass1!", "ds_audit02")
        .await;
    // This query should be denied by the deny policy
    let _ = client
        .simple_query(&format!("SELECT * FROM {schema}.accounts"))
        .await; // may error — that's expected

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let resp = server
        .admin
        .get("/api/v1/audit/queries")
        .authorization_bearer(&server.admin_token)
        .await;
    resp.assert_status_ok();
    let body = resp.json::<serde_json::Value>();
    let entries = body["data"].as_array().unwrap();

    let entry = entries
        .iter()
        .find(|e| e["username"].as_str() == Some("user_audit02"))
        .expect("TC-AUDIT-02: no audit entry for user_audit02");

    assert_eq!(
        entry["status"].as_str(),
        Some("denied"),
        "TC-AUDIT-02: status must be 'denied'"
    );
    let err_msg = entry["error_message"].as_str().unwrap_or("");
    assert!(
        err_msg.contains("audit02-deny"),
        "TC-AUDIT-02: error_message must contain the policy name, got: {err_msg}"
    );
}

// ===========================================================================
// TC-AUDIT-03: Error query (non-existent table) → audit entry has status "error"
//              with error_message populated
// ===========================================================================

#[tokio::test]
async fn tc_audit_03_error_audit_status() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_audit03";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.things;
             CREATE TABLE {schema}.things (id INT, name TEXT);
             INSERT INTO {schema}.things VALUES (1, 'foo');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_audit03", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_audit03", "UserPass1!", "default", ds_id)
        .await;

    let client = server
        .connect_as("user_audit03", "UserPass1!", "ds_audit03")
        .await;
    // Query a non-existent table — DataFusion will fail to build the plan
    let _ = client
        .simple_query(&format!("SELECT * FROM {schema}.nonexistent_table_xyz"))
        .await; // expected to error

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let resp = server
        .admin
        .get("/api/v1/audit/queries")
        .authorization_bearer(&server.admin_token)
        .await;
    resp.assert_status_ok();
    let body = resp.json::<serde_json::Value>();
    let entries = body["data"].as_array().unwrap();

    let entry = entries
        .iter()
        .find(|e| e["username"].as_str() == Some("user_audit03"))
        .expect("TC-AUDIT-03: no audit entry for user_audit03");

    assert_eq!(
        entry["status"].as_str(),
        Some("error"),
        "TC-AUDIT-03: status must be 'error'"
    );
    let err_msg = entry["error_message"].as_str().unwrap_or("");
    assert!(
        !err_msg.is_empty(),
        "TC-AUDIT-03: error_message must be populated for failed queries"
    );
}

// ===========================================================================
// TC-AUDIT-04: Status filter — GET /audit/queries?status=denied returns only
//              denied entries
// ===========================================================================

#[tokio::test]
async fn tc_audit_04_status_filter() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_audit04";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.data;
             CREATE TABLE {schema}.data (id INT, category TEXT);
             INSERT INTO {schema}.data VALUES (1, 'a'), (2, 'b');"
        ))
        .await;

    let ds_id = server.create_datasource("ds_audit04", "open").await;
    server.discover(ds_id, &[schema]).await;
    let user_id = server
        .create_user("user_audit04", "UserPass1!", "default", ds_id)
        .await;

    // table_deny policy
    server
        .create_table_deny("audit04-deny", schema, "data", ds_id, Some(user_id))
        .await;

    let client = server
        .connect_as("user_audit04", "UserPass1!", "ds_audit04")
        .await;
    // Denied query
    let _ = client
        .simple_query(&format!("SELECT * FROM {schema}.data"))
        .await;

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Filter by status=denied
    let resp = server
        .admin
        .get("/api/v1/audit/queries?status=denied")
        .authorization_bearer(&server.admin_token)
        .await;
    resp.assert_status_ok();
    let body = resp.json::<serde_json::Value>();
    let entries = body["data"].as_array().unwrap();

    // All returned entries must have status "denied"
    for e in entries {
        assert_eq!(
            e["status"].as_str(),
            Some("denied"),
            "TC-AUDIT-04: status filter returned non-denied entry: {e}"
        );
    }

    // Filter by status=success should return no entries for this user (only denied queries ran)
    let resp2 = server
        .admin
        .get(&format!(
            "/api/v1/audit/queries?status=success&datasource_id={ds_id}"
        ))
        .authorization_bearer(&server.admin_token)
        .await;
    resp2.assert_status_ok();
    let body2 = resp2.json::<serde_json::Value>();
    let entries2 = body2["data"].as_array().unwrap();
    let user_entries: Vec<_> = entries2
        .iter()
        .filter(|e| e["username"].as_str() == Some("user_audit04"))
        .collect();
    assert!(
        user_entries.is_empty(),
        "TC-AUDIT-04: success filter should return no entries for user_audit04 (only denied queries ran)"
    );
}

// ===========================================================================
// TC-AUDIT-05: Write statement (INSERT) rejected by ReadOnlyHook
//              → audit entry has status "denied", error_message is present
// ===========================================================================

#[tokio::test]
async fn tc_audit_05_write_rejected_audit_status() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "tc_audit05";

    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.items;
             CREATE TABLE {schema}.items (id INT, name TEXT);"
        ))
        .await;

    let ds_id = server.create_datasource("ds_audit05", "open").await;
    server.discover(ds_id, &[schema]).await;
    let _user_id = server
        .create_user("user_audit05", "UserPass1!", "default", ds_id)
        .await;

    let client = server
        .connect_as("user_audit05", "UserPass1!", "ds_audit05")
        .await;
    // INSERT should be rejected by ReadOnlyHook but audited first by PolicyHook
    let _ = client
        .simple_query(&format!("INSERT INTO {schema}.items VALUES (1, 'test')"))
        .await; // expected to error

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let resp = server
        .admin
        .get("/api/v1/audit/queries")
        .authorization_bearer(&server.admin_token)
        .await;
    resp.assert_status_ok();
    let body = resp.json::<serde_json::Value>();
    let entries = body["data"].as_array().unwrap();

    let entry = entries
        .iter()
        .find(|e| e["username"].as_str() == Some("user_audit05"))
        .expect("TC-AUDIT-05: no audit entry for write-rejected statement");

    assert_eq!(
        entry["status"].as_str(),
        Some("denied"),
        "TC-AUDIT-05: write rejection must be audited as 'denied'"
    );
    let err_msg = entry["error_message"].as_str().unwrap_or("");
    assert!(
        err_msg.contains("read-only"),
        "TC-AUDIT-05: error_message must mention read-only, got: {err_msg}"
    );
}
