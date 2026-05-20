//! Acceptor wrapper that detects plain HTTP on a TLS port and sends
//! a `301 Moved Permanently` redirect to the `https://` equivalent,
//! so users who accidentally type `http://host:port` get sent to the
//! right place instead of seeing a garbled TLS handshake error.

use axum_server::accept::Accept;
use std::{future::Future, io, pin::Pin};
use tokio::net::TcpStream;

/// Wraps an inner [`Accept`] implementation and intercepts plain-HTTP
/// connections before they reach the TLS handshake.
///
/// A TLS ClientHello always starts with byte `0x16`. Anything else is
/// assumed to be plain HTTP; we reply with a redirect and close.
///
/// `redirect_port` is the public-facing HTTPS port used in the redirect
/// URL (may differ from the actual listen port when behind nftables redirect).
#[derive(Clone, Debug)]
pub struct HttpRedirectAcceptor<A> {
    inner: A,
    redirect_port: u16,
}

impl<A> HttpRedirectAcceptor<A> {
    pub fn new(inner: A, redirect_port: u16) -> Self {
        Self { inner, redirect_port }
    }
}

impl<A, S> Accept<TcpStream, S> for HttpRedirectAcceptor<A>
where
    A: Accept<TcpStream, S> + Clone + Send + Sync + 'static,
    A::Stream: Send,
    A::Service: Send,
    A::Future: Send,
    S: Send + 'static,
{
    type Stream = A::Stream;
    type Service = A::Service;
    type Future = Pin<Box<dyn Future<Output = io::Result<(Self::Stream, Self::Service)>> + Send>>;

    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        let inner = self.inner.clone();
        let redirect_port = self.redirect_port;

        Box::pin(async move {
            // Peek at first byte without consuming it.
            let mut buf = [0u8; 1];
            let n = stream.peek(&mut buf).await?;

            if n > 0 && buf[0] == 0x16 {
                // TLS ClientHello — delegate to the real TLS acceptor.
                return inner.accept(stream, service).await;
            }

            // Plain HTTP — try to read the request so we can extract Host
            // and path for a proper redirect.
            send_redirect(&stream, redirect_port).await;

            Err(io::Error::new(
                io::ErrorKind::Other,
                "plain HTTP on TLS port — sent redirect",
            ))
        })
    }
}

/// Read enough of the HTTP request to build a redirect URL, then
/// write a `301 Moved Permanently` response and shut down the socket.
async fn send_redirect(stream: &TcpStream, port: u16) {
    // Read up to 4 KiB — plenty for the request line + Host header.
    let mut buf = vec![0u8; 4096];
    let n = match stream.try_read(&mut buf) {
        Ok(n) => n,
        Err(_) => {
            // Readable event not ready — just try peek + a small read.
            match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                read_available(stream, &mut buf),
            )
            .await
            {
                Ok(Ok(n)) => n,
                _ => 0,
            }
        }
    };

    let head = std::str::from_utf8(&buf[..n]).unwrap_or("");

    let host = extract_host(head).unwrap_or("localhost");
    let path = extract_path(head).unwrap_or("/");

    let location = if port == 443 {
        format!("https://{host}{path}")
    } else {
        format!("https://{host}:{port}{path}")
    };

    let body = format!(
        "<html><body>Redirecting to <a href=\"{location}\">{location}</a></body></html>\r\n"
    );
    let response = format!(
        "HTTP/1.1 301 Moved Permanently\r\n\
         Location: {location}\r\n\
         Content-Type: text/html\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len(),
    );

    // Best-effort write; ignore errors — the connection is being dropped anyway.
    let stream_ref = stream;
    let _ = write_all_best_effort(stream_ref, response.as_bytes()).await;
}

/// Fallback read using `readable()` + `try_read()`.
async fn read_available(stream: &TcpStream, buf: &mut [u8]) -> io::Result<usize> {
    stream.readable().await?;
    stream.try_read(buf)
}

/// Best-effort write — ignores partial/failed writes.
async fn write_all_best_effort(stream: &TcpStream, data: &[u8]) -> io::Result<()> {
    stream.writable().await?;
    // try_write is non-blocking; for a small redirect response it should fit
    // in the kernel buffer in one go.
    let _ = stream.try_write(data);
    Ok(())
}

fn extract_path(head: &str) -> Option<&str> {
    // "GET /foo/bar HTTP/1.1\r\n..."
    let first_line = head.lines().next()?;
    let mut parts = first_line.split_whitespace();
    let _method = parts.next()?;
    let path = parts.next()?;
    Some(path)
}

fn extract_host(head: &str) -> Option<&str> {
    for line in head.lines() {
        // Case-insensitive match on "Host:"
        if line.len() > 5 && line[..5].eq_ignore_ascii_case("Host:") {
            let value = line[5..].trim();
            // Strip port if present (e.g. "example.com:3000" → "example.com")
            return Some(value.split(':').next().unwrap_or(value));
        }
    }
    None
}
