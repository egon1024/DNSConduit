//! Bidirectional Frame Streams client (dnstap transport).
//!
//! Collectors such as `fstrm_capture` expect READY → ACCEPT → START before data frames.
//! Tools like `dnstap-receiver` also handle this sequence and remain compatible.

use std::io::{self, Read, Write};

pub const CONTROL_ACCEPT: u32 = 0x01;
pub const CONTROL_START: u32 = 0x02;
pub const CONTROL_STOP: u32 = 0x03;
pub const CONTROL_READY: u32 = 0x04;
#[allow(dead_code)]
pub const CONTROL_FINISH: u32 = 0x05;
pub const CONTROL_FIELD_CONTENT_TYPE: u32 = 0x01;

const CONTROL_FRAME_LENGTH_MAX: usize = 512;

/// Perform the bidirectional handshake and return a writer ready for data frames.
pub fn connect_bidirectional<S>(stream: S, content_type: &str) -> io::Result<FrameStreamWriter<S>>
where
    S: Read + Write + Send,
{
    let mut stream = stream;
    write_control_frame(&mut stream, CONTROL_READY, Some(content_type))?;
    let accepted = read_control_frame(&mut stream)?;
    if accepted.frame_type != CONTROL_ACCEPT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected ACCEPT control frame, got {}", accepted.frame_type),
        ));
    }
    if !accepted.matches_content_type(content_type) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ACCEPT content type mismatch",
        ));
    }
    write_control_frame(&mut stream, CONTROL_START, Some(content_type))?;
    Ok(FrameStreamWriter { inner: stream })
}

/// Writer for data frames after a successful bidirectional handshake.
pub struct FrameStreamWriter<S> {
    inner: S,
}

impl<S: Write> FrameStreamWriter<S> {
    pub fn write_data_frame(&mut self, payload: &[u8]) -> io::Result<()> {
        write_data_frame(&mut self.inner, payload)?;
        self.inner.flush()
    }

    pub fn finish(mut self) -> io::Result<()> {
        write_control_frame(&mut self.inner, CONTROL_STOP, None)?;
        self.inner.flush()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedControlFrame {
    pub frame_type: u32,
    pub content_types: Vec<Vec<u8>>,
}

impl DecodedControlFrame {
    pub fn matches_content_type(&self, expected: &str) -> bool {
        if self.content_types.is_empty() {
            return true;
        }
        self.content_types
            .iter()
            .any(|ct| ct.as_slice() == expected.as_bytes())
    }
}

pub fn write_control_frame(
    w: &mut impl Write,
    frame_type: u32,
    content_type: Option<&str>,
) -> io::Result<()> {
    let mut body = Vec::new();
    write_u32_be(&mut body, frame_type)?;
    if let Some(ct) = content_type {
        let ct_bytes = ct.as_bytes();
        write_u32_be(&mut body, CONTROL_FIELD_CONTENT_TYPE)?;
        write_u32_be(&mut body, ct_bytes.len() as u32)?;
        body.extend_from_slice(ct_bytes);
    }

    let mut out = Vec::with_capacity(8 + body.len());
    write_u32_be(&mut out, 0)?; // data frame length 0 => control frame
    write_u32_be(&mut out, body.len() as u32)?;
    out.extend_from_slice(&body);
    w.write_all(&out)?;
    w.flush()
}

pub fn write_data_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    write_u32_be(w, payload.len() as u32)?;
    w.write_all(payload)?;
    Ok(())
}

pub fn read_control_frame(r: &mut impl Read) -> io::Result<DecodedControlFrame> {
    let data_len = read_u32_be(r)?;
    read_control_frame_after_data_length(r, data_len)
}

fn read_control_frame_after_data_length(
    r: &mut impl Read,
    data_len: u32,
) -> io::Result<DecodedControlFrame> {
    if data_len != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected control frame (data length 0), got data length {data_len}"),
        ));
    }
    let control_len = read_u32_be(r)? as usize;
    if !(4..=CONTROL_FRAME_LENGTH_MAX).contains(&control_len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid control frame length {control_len}"),
        ));
    }
    let mut body = vec![0u8; control_len];
    r.read_exact(&mut body)?;
    parse_control_body(&body)
}

fn parse_control_body(body: &[u8]) -> io::Result<DecodedControlFrame> {
    let frame_type = read_u32_from_slice(body, 0)?;
    let mut content_types = Vec::new();
    let mut offset = 4usize;
    while offset + 8 <= body.len() {
        let field_type = read_u32_from_slice(body, offset)?;
        offset += 4;
        let field_len = read_u32_from_slice(body, offset)? as usize;
        offset += 4;
        if field_type != CONTROL_FIELD_CONTENT_TYPE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported control field",
            ));
        }
        if offset + field_len > body.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated content type field",
            ));
        }
        content_types.push(body[offset..offset + field_len].to_vec());
        offset += field_len;
    }
    if offset != body.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "trailing bytes in control frame",
        ));
    }
    Ok(DecodedControlFrame {
        frame_type,
        content_types,
    })
}

/// Read one data frame payload (after handshake).
pub fn read_data_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let len = read_u32_be(r)? as usize;
    if len == 0 {
        // Control frame (e.g. STOP) — not a data frame.
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected control frame while reading data",
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Frame read after the initial handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingFrame {
    Data(Vec<u8>),
    Stop,
}

/// Read the next data or STOP control frame.
pub fn read_frame(r: &mut impl Read) -> io::Result<IncomingFrame> {
    let len = read_u32_be(r)?;
    if len > 0 {
        let mut buf = vec![0u8; len as usize];
        r.read_exact(&mut buf)?;
        return Ok(IncomingFrame::Data(buf));
    }
    let control = read_control_frame_after_data_length(r, 0)?;
    if control.frame_type == CONTROL_STOP {
        Ok(IncomingFrame::Stop)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected control frame type {}", control.frame_type),
        ))
    }
}

/// Server: bidirectional READY → ACCEPT → START.
pub fn accept_bidirectional(
    stream: &mut (impl Read + Write),
    content_type: &str,
) -> io::Result<()> {
    let ready = read_control_frame(stream)?;
    if ready.frame_type != CONTROL_READY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected READY, got {}", ready.frame_type),
        ));
    }
    if !ready.matches_content_type(content_type) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "READY content type mismatch",
        ));
    }
    write_control_frame(stream, CONTROL_ACCEPT, Some(content_type))?;
    let start = read_control_frame(stream)?;
    if start.frame_type != CONTROL_START {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected START, got {}", start.frame_type),
        ));
    }
    if !start.matches_content_type(content_type) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "START content type mismatch",
        ));
    }
    Ok(())
}

/// Server: accept START-only clients (legacy uni-directional).
pub fn accept_unidirectional(
    stream: &mut (impl Read + Write),
    content_type: &str,
) -> io::Result<()> {
    let start = read_control_frame(stream)?;
    if start.frame_type != CONTROL_START {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected START, got {}", start.frame_type),
        ));
    }
    if !start.matches_content_type(content_type) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "START content type mismatch",
        ));
    }
    Ok(())
}

fn write_u32_be(w: &mut impl Write, n: u32) -> io::Result<()> {
    w.write_all(&n.to_be_bytes())
}

fn read_u32_be(r: &mut impl Read) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

fn read_u32_from_slice(buf: &[u8], offset: usize) -> io::Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "offset overflow"))?;
    if end > buf.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "slice too short for u32",
        ));
    }
    Ok(u32::from_be_bytes(buf[offset..end].try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn ready_frame_matches_fstrm_layout() {
        let mut buf = Vec::new();
        write_control_frame(&mut buf, CONTROL_READY, Some("protobuf:dnstap.Dnstap")).unwrap();
        assert_eq!(buf.len(), 42);
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);
        assert!(buf.windows(8).any(|w| w == b"protobuf"));
        let decoded = read_control_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(decoded.frame_type, CONTROL_READY);
        assert!(decoded.matches_content_type("protobuf:dnstap.Dnstap"));
    }

    #[test]
    fn roundtrip_ready_accept_start() {
        let ct = "protobuf:dnstap.Dnstap";
        let mut wire = Vec::new();
        write_control_frame(&mut wire, CONTROL_READY, Some(ct)).unwrap();
        write_control_frame(&mut wire, CONTROL_ACCEPT, Some(ct)).unwrap();
        write_control_frame(&mut wire, CONTROL_START, Some(ct)).unwrap();
        write_data_frame(&mut wire, b"payload").unwrap();

        let mut cursor = Cursor::new(wire);
        let ready = read_control_frame(&mut cursor).unwrap();
        assert_eq!(ready.frame_type, CONTROL_READY);
        let accept = read_control_frame(&mut cursor).unwrap();
        assert_eq!(accept.frame_type, CONTROL_ACCEPT);
        assert!(accept.matches_content_type(ct));
        let start = read_control_frame(&mut cursor).unwrap();
        assert_eq!(start.frame_type, CONTROL_START);
        let data = read_data_frame(&mut cursor).unwrap();
        assert_eq!(data, b"payload");
    }

    #[cfg(unix)]
    #[test]
    fn data_frames_reach_peer_before_stop() {
        use std::os::unix::net::UnixStream;
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let (client, server) = UnixStream::pair().expect("socketpair");
        let (tx, rx) = mpsc::sync_channel(1);

        thread::spawn(move || {
            let mut server = server;
            let ready = read_control_frame(&mut server).unwrap();
            assert_eq!(ready.frame_type, CONTROL_READY);
            write_control_frame(&mut server, CONTROL_ACCEPT, Some("protobuf:dnstap.Dnstap"))
                .unwrap();
            let start = read_control_frame(&mut server).unwrap();
            assert_eq!(start.frame_type, CONTROL_START);
            let payload = read_data_frame(&mut server).unwrap();
            tx.send(payload).unwrap();
        });

        let mut writer = connect_bidirectional(client, "protobuf:dnstap.Dnstap").unwrap();
        writer.write_data_frame(b"payload-first").unwrap();
        writer.finish().unwrap();
        let payload = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(payload, b"payload-first");
    }

    #[cfg(unix)]
    #[test]
    fn bidirectional_handshake_with_mock_collector() {
        use std::os::unix::net::{UnixListener, UnixStream};
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dnstap.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = mpsc::sync_channel(1);

        thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let ready = read_control_frame(&mut conn).unwrap();
            assert_eq!(ready.frame_type, CONTROL_READY);
            write_control_frame(&mut conn, CONTROL_ACCEPT, Some("protobuf:dnstap.Dnstap")).unwrap();
            let start = read_control_frame(&mut conn).unwrap();
            assert_eq!(start.frame_type, CONTROL_START);
            let payload = read_data_frame(&mut conn).unwrap();
            tx.send(payload).unwrap();
        });

        let client = UnixStream::connect(&path).unwrap();
        let ct = "protobuf:dnstap.Dnstap";
        let mut writer = connect_bidirectional(client, ct).unwrap();
        writer.write_data_frame(b"dnstap-bytes").unwrap();
        let payload = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(payload, b"dnstap-bytes");
    }
}
