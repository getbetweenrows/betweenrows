//! Wire protocol integration tests.
//!
//! These tests verify basic pgwire protocol behavior through the full proxy
//! stack using a real Postgres container.

mod support;

use serde_json::json;
use support::TEST_PASS;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Set up a datasource with a simple `public.orders` table in open mode.
async fn setup_open_datasource(
    server: &support::ProxyTestServer,
    schema: &str,
) -> (uuid::Uuid, uuid::Uuid) {
    server
        .seed_upstream(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema};
             DROP TABLE IF EXISTS {schema}.orders;
             CREATE TABLE {schema}.orders (id INT, name TEXT);"
        ))
        .await;
    server
        .seed_upstream(&format!(
            "INSERT INTO {schema}.orders VALUES (1, 'Alice'), (2, 'Bob');"
        ))
        .await;

    let ds_id = server
        .create_datasource(&format!("proto_{schema}"), "open")
        .await;
    server.discover(ds_id, &[schema]).await;

    let user_id = server
        .create_user("testuser", TEST_PASS, "default", ds_id)
        .await;

    // In open mode, add a basic column_access allow-all policy so queries work
    server
        .create_and_assign_policy(
            &format!("allow-all-{schema}"),
            "permit",
            vec![json!({
                "obligation_type": "column_access",
                "definition": {
                    "schema": schema,
                    "table": "*",
                    "columns": ["*"],
                    "action": "allow"
                }
            })],
            ds_id,
            None,
        )
        .await;

    (ds_id, user_id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn version_query() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_ver";
    setup_open_datasource(&server, schema).await;

    let client = server
        .connect_as("testuser", TEST_PASS, &format!("proto_{schema}"))
        .await;
    let rows = client.query("SELECT version()", &[]).await.unwrap();
    assert_eq!(rows.len(), 1);
    let version: String = rows[0].get(0);
    assert!(
        !version.is_empty(),
        "version() should return a non-empty string"
    );
}

#[tokio::test]
async fn explain_returns_query_plan_column() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_explain";
    setup_open_datasource(&server, schema).await;

    let client = server
        .connect_as("testuser", TEST_PASS, &format!("proto_{schema}"))
        .await;
    let rows = client
        .query("EXPLAIN SELECT id FROM orders", &[])
        .await
        .unwrap();
    assert!(!rows.is_empty(), "EXPLAIN should return at least one row");
    // The proxy reformats EXPLAIN into PostgreSQL's single "QUERY PLAN" column,
    // or DataFusion may return "plan_type" + "plan" columns.
    let col_names: Vec<&str> = rows[0].columns().iter().map(|c| c.name()).collect();
    let has_expected = col_names.contains(&"QUERY PLAN")
        || col_names.contains(&"plan_type")
        || col_names.contains(&"plan");
    assert!(
        has_expected,
        "EXPLAIN should have recognizable column names, got: {col_names:?}"
    );
}

#[tokio::test]
async fn show_tables_returns_result_set() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_show";
    setup_open_datasource(&server, schema).await;

    let client = server
        .connect_as("testuser", TEST_PASS, &format!("proto_{schema}"))
        .await;
    // SHOW TABLES should succeed without error
    let result = client.simple_query("SHOW TABLES").await;
    assert!(
        result.is_ok(),
        "SHOW TABLES should not error: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn multi_statement_batch_execute() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_batch";
    setup_open_datasource(&server, schema).await;

    let client = server
        .connect_as("testuser", TEST_PASS, &format!("proto_{schema}"))
        .await;
    let result = client.simple_query("SELECT 1; SELECT 2;").await;
    assert!(
        result.is_ok(),
        "Multi-statement batch should not error: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn wrong_password_rejected() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_badpw";
    setup_open_datasource(&server, schema).await;

    let result = server
        .try_connect_as("testuser", "WrongPassword1!", &format!("proto_{schema}"))
        .await;
    assert!(
        result.is_err(),
        "Connection with wrong password should be rejected"
    );
}

#[tokio::test]
async fn missing_datasource_rejected() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_nods";
    setup_open_datasource(&server, schema).await;

    let result = server
        .try_connect_as("testuser", TEST_PASS, "nonexistent_datasource")
        .await;
    assert!(
        result.is_err(),
        "Connection to nonexistent datasource should be rejected"
    );
}

#[tokio::test]
async fn invalid_sql_returns_error() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_badsql";
    setup_open_datasource(&server, schema).await;

    let client = server
        .connect_as("testuser", TEST_PASS, &format!("proto_{schema}"))
        .await;
    let result = client
        .query("SELECT * FROM nonexistent_xyz_table", &[])
        .await;
    assert!(
        result.is_err(),
        "Query on nonexistent table should return an error, not panic"
    );
}

// ===========================================================================
// C2: Read-only enforcement — INSERT / UPDATE / DELETE blocked by ReadOnlyHook
// ===========================================================================

#[tokio::test]
async fn insert_blocked_by_read_only_hook() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_insert";
    setup_open_datasource(&server, schema).await;

    let client = server
        .connect_as("testuser", TEST_PASS, &format!("proto_{schema}"))
        .await;

    // ReadOnlyHook must block INSERT with SQLSTATE 25006
    let result = client
        .simple_query(&format!(
            "INSERT INTO {schema}.orders (id, name) VALUES (99, 'Eve')"
        ))
        .await;
    assert!(result.is_err(), "INSERT should be blocked by ReadOnlyHook");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("25006") || err_msg.contains("read") || err_msg.contains("read-only"),
        "Error should indicate read-only violation (SQLSTATE 25006), got: {err_msg}"
    );
}

#[tokio::test]
async fn update_blocked_by_read_only_hook() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_update";
    setup_open_datasource(&server, schema).await;

    let client = server
        .connect_as("testuser", TEST_PASS, &format!("proto_{schema}"))
        .await;

    // ReadOnlyHook must block UPDATE with SQLSTATE 25006
    let result = client
        .simple_query(&format!(
            "UPDATE {schema}.orders SET name = 'Eve' WHERE id = 1"
        ))
        .await;
    assert!(result.is_err(), "UPDATE should be blocked by ReadOnlyHook");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("25006") || err_msg.contains("read") || err_msg.contains("read-only"),
        "Error should indicate read-only violation (SQLSTATE 25006), got: {err_msg}"
    );
}

#[tokio::test]
async fn delete_blocked_by_read_only_hook() {
    let _pg = require_postgres!();
    let server = support::ProxyTestServer::start().await;
    let schema = "proto_delete";
    setup_open_datasource(&server, schema).await;

    let client = server
        .connect_as("testuser", TEST_PASS, &format!("proto_{schema}"))
        .await;

    // ReadOnlyHook must block DELETE with SQLSTATE 25006
    let result = client
        .simple_query(&format!("DELETE FROM {schema}.orders WHERE id = 1"))
        .await;
    assert!(result.is_err(), "DELETE should be blocked by ReadOnlyHook");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("25006") || err_msg.contains("read") || err_msg.contains("read-only"),
        "Error should indicate read-only violation (SQLSTATE 25006), got: {err_msg}"
    );
}
