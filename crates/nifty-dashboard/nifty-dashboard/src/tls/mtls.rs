use axum_server::accept::Accept;
use dashmap::DashMap;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::TcpStream;

/// Peer certificates (DER-encoded) keyed by socket address.
pub type PeerCertMap = Arc<DashMap<SocketAddr, Vec<Vec<u8>>>>;

/// Wraps an inner TLS acceptor and captures peer certificates into a shared map
/// after the TLS handshake completes.
///
/// Works with any acceptor whose output stream is a `tokio_rustls::server::TlsStream<TcpStream>`
/// (e.g. `RustlsAcceptor`, ACME acceptors).
#[derive(Clone)]
pub struct CertCaptureAcceptor<A> {
    inner: A,
    peer_certs: PeerCertMap,
}

impl<A> CertCaptureAcceptor<A> {
    pub fn new(inner: A, peer_certs: PeerCertMap) -> Self {
        Self { inner, peer_certs }
    }
}

impl<A, S> Accept<TcpStream, S> for CertCaptureAcceptor<A>
where
    A: Accept<TcpStream, S, Stream = tokio_rustls::server::TlsStream<TcpStream>>
        + Clone
        + Send
        + Sync
        + 'static,
    A::Service: Send,
    A::Future: Send,
    S: Send + 'static,
{
    type Stream = tokio_rustls::server::TlsStream<TcpStream>;
    type Service = A::Service;
    type Future = Pin<Box<dyn Future<Output = io::Result<(Self::Stream, Self::Service)>> + Send>>;

    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        let peer_addr = stream.peer_addr().ok();
        let inner = self.inner.clone();
        let peer_certs = self.peer_certs.clone();

        Box::pin(async move {
            let (tls_stream, service) = inner.accept(stream, service).await?;

            if let Some(addr) = peer_addr {
                let (_, session) = tls_stream.get_ref();
                let certs = session
                    .peer_certificates()
                    .map(|cs| cs.iter().map(|c| c.as_ref().to_vec()).collect())
                    .unwrap_or_default();
                peer_certs.insert(addr, certs);
            }

            Ok((tls_stream, service))
        })
    }
}
