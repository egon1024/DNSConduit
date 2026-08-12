//! Control-plane TCP accept + optional TLS handshake with operator-visible failure logs.

use std::io;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

/// Plaintext accept loop: TCP accept failures are logged at **warn** and skipped.
pub fn plain_control_incoming(
    listener: TcpListener,
) -> impl Stream<Item = Result<TcpStream, io::Error>> + Send + 'static {
    let (tx, rx) = mpsc::channel::<Result<TcpStream, io::Error>>(128);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _peer)) => {
                            if tx.send(Ok(stream)).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                tls = false,
                                "control plane connection failed"
                            );
                        }
                    }
                }
            }
        }
    });
    ReceiverStream::new(rx)
}

/// TLS accept loop: handshake failures (and TCP accept failures) are logged at **warn**
/// with peer address when known, then skipped so tonic never sees them.
pub fn tls_control_incoming(
    listener: TcpListener,
    acceptor: TlsAcceptor,
) -> impl Stream<Item = Result<TlsStream<TcpStream>, io::Error>> + Send + 'static {
    let (tx, rx) = mpsc::channel::<Result<TlsStream<TcpStream>, io::Error>>(128);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, peer)) => {
                            let acceptor = acceptor.clone();
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                match acceptor.accept(stream).await {
                                    Ok(tls) => {
                                        let _ = tx.send(Ok(tls)).await;
                                    }
                                    Err(e) => {
                                        log_handshake_failure(Some(peer), &e);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                tls = true,
                                "control plane connection failed"
                            );
                        }
                    }
                }
            }
        }
    });
    ReceiverStream::new(rx)
}

fn log_handshake_failure(peer: Option<SocketAddr>, err: &impl std::fmt::Display) {
    match peer {
        Some(peer) => {
            tracing::warn!(
                peer = %peer,
                error = %err,
                tls = true,
                "control plane connection failed"
            );
        }
        None => {
            tracing::warn!(
                error = %err,
                tls = true,
                "control plane connection failed"
            );
        }
    }
}
