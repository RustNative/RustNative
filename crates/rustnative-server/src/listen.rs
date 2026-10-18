//! Accepting connections on a listener that must keep serving: an accept
//! error caused by one peer or by a passing shortage (out of file
//! descriptors, a connection aborted before it was accepted) is waited out,
//! never returned, so a burst of connections cannot stop the server.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};

/// The first wait after a resource shortage; it doubles up to
/// [`MAX_BACKOFF`] while the shortage lasts.
const MIN_BACKOFF: Duration = Duration::from_millis(10);
const MAX_BACKOFF: Duration = Duration::from_millis(100);

/// The next connection on `listener`. Errors that concern one connection
/// are skipped; a shortage (`EMFILE`, `ENFILE`, `ENOBUFS`, `ENOMEM`) is
/// logged once and retried with a short backoff until it passes.
///
/// # Errors
///
/// Only an error that says the listener itself is unusable (`EINVAL`):
/// retrying it would never succeed.
pub(crate) async fn accept(listener: &TcpListener) -> io::Result<(TcpStream, SocketAddr)> {
    let mut backoff = MIN_BACKOFF;
    loop {
        match listener.accept().await {
            Ok(accepted) => return Ok(accepted),
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => return Err(error),
            Err(error) if concerns_one_connection(&error) => {}
            Err(error) => {
                if backoff == MIN_BACKOFF {
                    eprintln!("rustnative-server: accepting a connection: {error}; retrying");
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

fn concerns_one_connection(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::Interrupted
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::TimedOut
    )
}
