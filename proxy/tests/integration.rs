//! Integration tests for QueryProxy
//!
//! These tests connect to a running proxy instance via tokio-postgres
//! and test end-to-end query flows.
//!
//! Prerequisites:
//! - Proxy must be running: `cargo run`
//! - Supabase backend must be accessible
//!
//! Run with: `cargo test -- --ignored`

use tokio_postgres::{Error, NoTls};

/// Connect to the running proxy
async fn connect() -> Result<tokio_postgres::Client, Error> {
    let (client, connection) = tokio_postgres::connect(
        "host=127.0.0.1 port=5434 user=postgres password=postgres dbname=postgres",
        NoTls,
    )
    .await?;

    // Spawn the connection in the background
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("Connection error: {}", e);
        }
    });

    Ok(client)
}

#[tokio::test]
#[ignore] // Requires running proxy
async fn test_simple_select() {
    let client = connect().await.expect("Failed to connect to proxy");

    let rows = client
        .query("SELECT 'hello' AS msg, 'world' AS msg2", &[])
        .await
        .expect("Failed to execute query");

    assert_eq!(rows.len(), 1, "Should return exactly one row");

    // Test string columns (these work fine)
    let msg: String = rows[0].get(0);
    let msg2: String = rows[0].get(1);

    assert_eq!(msg, "hello");
    assert_eq!(msg2, "world");
}

#[tokio::test]
#[ignore] // Requires running proxy
async fn test_version_query() {
    let client = connect().await.expect("Failed to connect to proxy");

    let row = client
        .query_one("SELECT version()", &[])
        .await
        .expect("Failed to execute version query");

    let version: String = row.get(0);
    assert!(!version.is_empty(), "Version string should not be empty");
    // DataFusion returns its own version, not Postgres version
    assert!(
        version.to_lowercase().contains("datafusion"),
        "Version string should contain 'datafusion'"
    );
}

#[tokio::test]
#[ignore] // Requires running proxy
async fn test_system_catalog_query() {
    let client = connect().await.expect("Failed to connect to proxy");

    // Query information_schema (tests pg_catalog federation)
    let rows = client
        .query(
            "SELECT table_name FROM information_schema.tables LIMIT 5",
            &[],
        )
        .await
        .expect("Failed to query information_schema");

    assert!(!rows.is_empty(), "Should return at least one table");
}

#[tokio::test]
#[ignore] // Requires running proxy + performer table
async fn test_rls_filter_applied() {
    let client = connect().await.expect("Failed to connect to proxy");

    // Query performer table - RLS hook should filter by tenant='foo'
    let rows = client
        .query("SELECT tenant FROM performer", &[])
        .await
        .expect("Failed to query performer table");

    // All rows should have tenant='foo'
    for row in rows {
        let tenant: String = row.get(0);
        assert_eq!(tenant, "foo", "RLS filter should restrict to tenant='foo'");
    }
}

#[tokio::test]
#[ignore] // Requires running proxy
async fn test_multi_statement() {
    let client = connect().await.expect("Failed to connect to proxy");

    // Execute multiple statements
    client
        .batch_execute("SELECT 1; SELECT 2;")
        .await
        .expect("Failed to execute multiple statements");
}

#[tokio::test]
#[ignore] // Requires running proxy
async fn test_sql_rewrite_in_context() {
    let client = connect().await.expect("Failed to connect to proxy");

    // Query pg_class without pg_catalog prefix
    // sql_rewrite should prepend pg_catalog automatically
    let rows = client
        .query("SELECT relname FROM pg_class LIMIT 5", &[])
        .await
        .expect("Failed to query pg_class (sql_rewrite should qualify it)");

    assert!(!rows.is_empty(), "Should return pg_class rows");
}

#[tokio::test]
#[ignore] // Requires running proxy + performer table
async fn test_aggregate_with_rls() {
    let client = connect().await.expect("Failed to connect to proxy");

    let row = client
        .query_one("SELECT COUNT(*) FROM performer", &[])
        .await
        .expect("Failed to execute COUNT query with RLS");

    let count: i64 = row.get(0);
    assert!(count >= 0, "Count should be non-negative");
}

#[tokio::test]
#[ignore] // Requires running proxy
async fn test_invalid_sql_returns_error() {
    let client = connect().await.expect("Failed to connect to proxy");

    // Malformed SQL should return an error, not crash
    let result = client
        .query("SELECT * FROM nonexistent_table_xyz", &[])
        .await;

    assert!(result.is_err(), "Invalid SQL should return an error");
}

#[tokio::test]
#[ignore] // Requires running proxy
async fn test_pg_catalog_function_call() {
    let client = connect().await.expect("Failed to connect to proxy");

    // Test calling a pg_catalog function
    let row = client
        .query_one("SELECT pg_catalog.current_database()", &[])
        .await
        .expect("Failed to call pg_catalog function");

    let db_name: String = row.get(0);
    // DataFusion returns "datafusion" as the database name
    assert_eq!(db_name, "datafusion", "Should return database name");
}

#[tokio::test]
#[ignore] // Requires running proxy
async fn test_join_query() {
    let client = connect().await.expect("Failed to connect to proxy");

    // Test a simple self-join on information_schema
    let rows = client
        .query(
            "SELECT t1.table_name FROM information_schema.tables t1 \
             JOIN information_schema.tables t2 ON t1.table_name = t2.table_name \
             LIMIT 3",
            &[],
        )
        .await
        .expect("Failed to execute JOIN query");

    assert!(!rows.is_empty(), "JOIN query should return results");
}
