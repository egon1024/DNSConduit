//! TCP upstream forward (RFC 7766 length-prefixed).

use socket2::{Domain, Socket, Type};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// Send a DNS query over TCP and read the length-prefixed response.
pub fn forward_tcp(
    backend: SocketAddr,
    query_wire: &[u8],
    timeout: Duration,
    bind_source_v4: Option<Ipv4Addr>,
    bind_source_v6: Option<Ipv6Addr>,
) -> std::io::Result<Vec<u8>> {
    let domain = if backend.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, None)?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;
    if backend.is_ipv4() {
        if let Some(ip) = bind_source_v4 {
            socket.bind(&SocketAddr::from((ip, 0)).into())?;
        }
    } else if let Some(ip) = bind_source_v6 {
        socket.bind(&SocketAddr::from((ip, 0)).into())?;
    }
    socket.connect(&backend.into())?;
    let mut stream = TcpStream::from(socket);
    let len = u16::try_from(query_wire.len()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "query too large for TCP DNS",
        )
    })?;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(query_wire)?;
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf)?;
    let resp_len = usize::from(u16::from_be_bytes(len_buf));
    let mut buf = vec![0u8; resp_len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}
