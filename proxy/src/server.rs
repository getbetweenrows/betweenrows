use std::io;
use std::time::Duration;

use futures::StreamExt;
use pgwire::api::{ClientInfo, ErrorHandler, PgWireConnectionState, PgWireServerHandlers};
use pgwire::tokio::server::{negotiate_tls, process_error, process_message};
use tokio::net::TcpStream;
use tokio::time::sleep;

const STARTUP_TIMEOUT_MILLIS: u64 = 60_000;

/// Wraps pgwire's `process_socket` with a per-message idle timeout.
///
/// After authentication, if no message arrives within `idle_timeout` the
/// connection is closed gracefully. This allows Fly.io `auto_stop_machines`
/// to work correctly: idle clients like TablePlus that hold a connection
/// open indefinitely will be disconnected, letting the machine stop.
pub async fn process_socket_with_idle_timeout<H>(
    tcp_socket: TcpStream,
    handlers: H,
    idle_timeout: Duration,
) -> Result<(), io::Error>
where
    H: PgWireServerHandlers,
{
    // Startup timeout matches pgwire's hardcoded 60 s.
    let startup_timeout = sleep(Duration::from_millis(STARTUP_TIMEOUT_MILLIS));
    tokio::pin!(startup_timeout);

    // Negotiate TLS (or plain) — races against the startup timeout.
    let socket = tokio::select! {
        _ = &mut startup_timeout => return Ok(()),
        socket = negotiate_tls(tcp_socket, None) => socket?,
    };
    let Some(mut socket) = socket else {
        return Ok(());
    };

    let startup_handler = handlers.startup_handler();
    let simple_query_handler = handlers.simple_query_handler();
    let extended_query_handler = handlers.extended_query_handler();
    let copy_handler = handlers.copy_handler();
    let cancel_handler = handlers.cancel_handler();
    let error_handler = handlers.error_handler();

    loop {
        let msg = if matches!(
            socket.state(),
            PgWireConnectionState::AwaitingStartup
                | PgWireConnectionState::AuthenticationInProgress
        ) {
            // During startup/auth use the same startup timer as pgwire.
            tokio::select! {
                _ = &mut startup_timeout => None,
                msg = socket.next() => msg,
            }
        } else {
            // After auth, race each next message against the idle timer.
            // Creating a fresh sleep() each iteration effectively resets
            // the timer after every received message.
            tokio::select! {
                _ = sleep(idle_timeout) => {
                    tracing::info!("Idle connection timed out after {:?}", idle_timeout);
                    break;
                }
                msg = socket.next() => msg,
            }
        };

        if let Some(Ok(msg)) = msg {
            let is_extended_query = match socket.state() {
                PgWireConnectionState::CopyInProgress(iq) => iq,
                _ => msg.is_extended_query(),
            };
            if let Err(mut e) = process_message(
                msg,
                &mut socket,
                startup_handler.clone(),
                simple_query_handler.clone(),
                extended_query_handler.clone(),
                copy_handler.clone(),
                cancel_handler.clone(),
            )
            .await
            {
                error_handler.on_error(&socket, &mut e);
                process_error(&mut socket, e, is_extended_query).await?;
            }
        } else {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::process_socket_with_idle_timeout;
    use pgwire::api::PgWireServerHandlers;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;

    /// Minimal server-side handler for tests.
    ///
    /// Uses pgwire's default `NoopStartupHandler` impl, which sends
    /// `AuthenticationOk` + `ReadyForQuery` unconditionally, accepting any
    /// credentials. All other handlers are no-ops.
    struct TestHandlers;
    impl PgWireServerHandlers for TestHandlers {}

    /// A silent client (no data sent after TCP connect) causes the 60-second
    /// startup timeout to fire and the function to return `Ok(())`.
    #[tokio::test]
    async fn test_startup_timeout_fires_on_silent_client() {
        tokio::time::pause();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (server_sock, _) = listener.accept().await.unwrap();
            process_socket_with_idle_timeout(server_sock, TestHandlers, Duration::from_secs(900))
                .await
        });

        // Connect but send no data — startup never completes.
        let _client = TcpStream::connect(addr).await.unwrap();
        // Yield so the server task enters its select! and registers the timer.
        tokio::task::yield_now().await;

        // Jump past the 60-second startup timeout.
        tokio::time::advance(Duration::from_secs(61)).await;

        let result = server.await.unwrap();
        assert!(
            result.is_ok(),
            "server should exit cleanly on startup timeout"
        );
    }

    /// Dropping the client socket before completing the handshake causes the
    /// function to return `Ok(())` — no panic, no error.
    #[tokio::test]
    async fn test_client_disconnect_before_auth_returns_ok() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client = TcpStream::connect(addr).await.unwrap();
        let (server_sock, _) = listener.accept().await.unwrap();
        drop(client); // close client side immediately

        let result =
            process_socket_with_idle_timeout(server_sock, TestHandlers, Duration::from_secs(900))
                .await;
        assert!(
            result.is_ok(),
            "server should exit cleanly on client disconnect"
        );
    }

    /// After a successful authentication handshake, the idle timer fires when
    /// no further messages arrive, and the function returns `Ok(())`.
    ///
    /// Uses `tokio::time::pause()` + `advance()` so the test is instant rather
    /// than waiting wall-clock seconds.
    #[tokio::test]
    async fn test_idle_timeout_fires_after_auth() {
        tokio::time::pause();

        let idle_timeout = Duration::from_secs(5);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (server_sock, _) = listener.accept().await.unwrap();
            process_socket_with_idle_timeout(server_sock, TestHandlers, idle_timeout).await
        });

        // tokio-postgres connects and completes the auth handshake.
        // TestHandlers sends AuthenticationOk + ReadyForQuery without any
        // credential check, so any username/password is accepted.
        let (client, conn) = tokio_postgres::connect(
            &format!("host=127.0.0.1 port={port} user=test dbname=test"),
            tokio_postgres::NoTls,
        )
        .await
        .expect("connect with NoopHandler should succeed");
        tokio::spawn(conn);

        // Yield to ensure the server task has looped back and registered its
        // idle sleep() future before we advance the clock.
        tokio::task::yield_now().await;

        // Jump past the idle timeout — server should close the connection.
        tokio::time::advance(idle_timeout + Duration::from_secs(1)).await;
        tokio::task::yield_now().await;

        // Server task should have returned Ok.
        let result = server.await.unwrap();
        assert!(result.is_ok(), "server should exit cleanly on idle timeout");

        // The client should now see the connection as closed.
        let query_result = client.execute("SELECT 1", &[]).await;
        assert!(
            query_result.is_err(),
            "query should fail after server closed idle connection"
        );
    }

    /// The idle timer is reset after each message: a query sent within the
    /// idle window keeps the connection alive; only sustained inactivity
    /// triggers the timeout.
    ///
    /// Timeline (all simulated time):
    ///   t=0   auth completes
    ///   t=3s  advance → idle timer has 2s remaining, no timeout yet
    ///   t=3s  client sends a query → timer resets to t=3+5=8s
    ///   t=9s  advance → 6s since last message, timer fires
    #[tokio::test]
    async fn test_idle_timer_resets_after_query() {
        tokio::time::pause();

        let idle_timeout = Duration::from_secs(5);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (server_sock, _) = listener.accept().await.unwrap();
            process_socket_with_idle_timeout(server_sock, TestHandlers, idle_timeout).await
        });

        let (client, conn) = tokio_postgres::connect(
            &format!("host=127.0.0.1 port={port} user=test dbname=test"),
            tokio_postgres::NoTls,
        )
        .await
        .unwrap();
        tokio::spawn(conn);
        tokio::task::yield_now().await;

        // Advance 3s — within the 5s idle window.
        tokio::time::advance(Duration::from_secs(3)).await;
        tokio::task::yield_now().await;

        // Server should still be alive: send a query to reset the idle timer.
        // TestHandlers' SimpleQueryHandler is a no-op, so it returns no rows
        // and no results — the client may see an error, but the connection
        // itself stays open at the server level.
        let _ = client.simple_query("SELECT 1").await;
        tokio::task::yield_now().await;

        // Advance another 6s past the reset point — timer fires now.
        tokio::time::advance(Duration::from_secs(6)).await;
        tokio::task::yield_now().await;

        let result = server.await.unwrap();
        assert!(
            result.is_ok(),
            "server should exit cleanly after reset timer expires"
        );
    }
}
