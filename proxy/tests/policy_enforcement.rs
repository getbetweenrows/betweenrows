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
        .create_and_assign_policy(
            "tenant-filter",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "tenant-filter-t2",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "tenant-filter-t3",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "tenant-filter-t4",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "tenant-filter-t5",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "deny-sensitive-t6",
            "deny",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "employees",
                    "columns": ["ssn", "salary"],
                    "action": "deny"
                }
            })],
            ds_id,
            None,
        )
        .await;

    // Permit policy for remaining columns
    server
        .create_and_assign_policy(
            "allow-rest-t6",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "employees",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "mask-ssn-t7",
            "permit",
            vec![
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "customers",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
                json!({
                    "obligation_type": "column_mask",
                    "definition": {
                        "schema": schema,
                        "table": "customers",
                        "column": "ssn",
                        "mask_expression": "CONCAT('***-**-', RIGHT(ssn, 4))"
                    }
                }),
            ],
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

    // Row filter on both tables
    server
        .create_and_assign_policy(
            "tenant-filter-t8",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "customers",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "customers",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
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
        .create_and_assign_policy(
            "deny-ssn-t10",
            "deny",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "employees",
                    "columns": ["ssn"],
                    "action": "deny"
                }
            })],
            ds_id,
            None,
        )
        .await;

    // Permit policy for remaining columns
    server
        .create_and_assign_policy(
            "allow-all-t10",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "employees",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
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
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
            false, // is_enabled = false
        )
        .await;

    // Need an active permit policy so the user can query
    server
        .create_and_assign_policy(
            "allow-all-t11",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
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

    // Deny access to analytics schema
    server
        .create_and_assign_policy(
            "deny-analytics-t12",
            "deny",
            vec![json!({
                "obligation_type": "object_access",
                "definition": {
                    "schema": analytics_schema,
                    "action": "deny"
                }
            })],
            ds_id,
            None,
        )
        .await;

    // Allow public schema
    server
        .create_and_assign_policy(
            "allow-public-t12",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": public_schema,
                    "table": "*",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
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

    // Policy 1: filter by org = user.tenant
    server
        .create_and_assign_policy(
            "org-filter-t13",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "org = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
        .await;

    // Policy 2: filter by status = 'active' (static value, no template var)
    server
        .create_and_assign_policy(
            "status-filter-t13",
            "permit",
            vec![json!({
                "obligation_type": "row_filter",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "filter_expression": "status = 'active'"
                }
            })],
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

    // Deny policy with row_filter — deny + row_filter short-circuits to an
    // access-denied error rather than silently filtering rows.
    server
        .create_and_assign_policy(
            "deny-rowfilter-c1",
            "deny",
            vec![json!({
                "obligation_type": "row_filter",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "filter_expression": "tenant = {user.tenant}"
                }
            })],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "allow-limited-c3",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "employees",
                    "columns": ["id", "name"],
                    "action": "allow"
                }
            })],
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
        .create_and_assign_policy(
            "allow-all-c4",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "employees",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
        .await;

    // Deny policy explicitly blocks ssn — deny must win over the wildcard allow
    server
        .create_and_assign_policy(
            "deny-ssn-c4",
            "deny",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "employees",
                    "columns": ["ssn"],
                    "action": "deny"
                }
            })],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "deny-payments-i1",
            "deny",
            vec![json!({
                "obligation_type": "object_access",
                "definition": {
                    "schema": schema,
                    "table": "payments",
                    "action": "deny"
                }
            })],
            ds_id,
            None,
        )
        .await;

    // Allow all columns on the orders table
    server
        .create_and_assign_policy(
            "allow-orders-i1",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "deny-ssn-i2",
            "deny",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "customers",
                    "columns": ["ssn"],
                    "action": "deny"
                }
            })],
            ds_id,
            None,
        )
        .await;

    // Permit policy — allows all and includes a mask on ssn
    // Deny should take precedence: ssn must not appear at all, not appear masked.
    server
        .create_and_assign_policy(
            "allow-mask-ssn-i2",
            "permit",
            vec![
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "customers",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
                json!({
                    "obligation_type": "column_mask",
                    "definition": {
                        "schema": schema,
                        "table": "customers",
                        "column": "ssn",
                        "mask_expression": "CONCAT('***-**-', RIGHT(ssn, 4))"
                    }
                }),
            ],
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
        .create_and_assign_policy(
            "username-filter-i3a",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "docs",
                        "filter_expression": "owner = {user.username}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "docs",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "uid-filter-i3b",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "items",
                        "filter_expression": "user_uuid = {user.id}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "items",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "tenant-filter-i4",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
// I7: Single policy with multiple row_filters on same table (within-policy AND)
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

    // A single policy with TWO row_filter obligations on the same table.
    // Both filters must be satisfied simultaneously (AND semantics within a policy).
    server
        .create_and_assign_policy(
            "dual-filter-i7",
            "permit",
            vec![
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "tenant = {user.tenant}"
                    }
                }),
                json!({
                    "obligation_type": "row_filter",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "filter_expression": "status = 'active'"
                    }
                }),
                json!({
                    "obligation_type": "column_access",
                    "definition": {
                        "schema": schema,
                        "table": "orders",
                        "columns": ["*"],
                        "action": "allow"
                    }
                }),
            ],
            ds_id,
            None,
        )
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
        "Within-policy AND: only row satisfying both tenant=acme AND status=active"
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
        .create_and_assign_policy(
            "allow-all-i8",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None, // wildcard — applies to all users
        )
        .await;

    // User-specific policy for alice only: restrict to org='acme' rows
    server
        .create_and_assign_policy(
            "alice-only-filter-i8",
            "permit",
            vec![json!({
                "obligation_type": "row_filter",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "filter_expression": "org = 'acme'"
                }
            })],
            ds_id,
            Some(alice_id), // user-specific assignment
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
        .create_and_assign_policy(
            "deny-secret-glob-n1",
            "deny",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "vault",
                    "columns": ["secret_*"],
                    "action": "deny"
                }
            })],
            ds_id,
            None,
        )
        .await;

    server
        .create_and_assign_policy(
            "allow-all-n1",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "vault",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "allow-all-n4",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "deny-tenant-n4",
            "deny",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "columns": ["tenant"],
                    "action": "deny"
                }
            })],
            ds_id,
            None,
        )
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
        .create_and_assign_policy(
            "allow-orders-n6",
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "orders",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
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
