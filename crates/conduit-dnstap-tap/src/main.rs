//! Dev dnstap collector: bind unix socket, decode frames (including `extra`), stream to stdout.

mod decode;
mod dns;
mod format;
mod message_meta;

use anyhow::{bail, Context, Result};
use conduit_observation::fstrm::{
    accept_bidirectional, accept_unidirectional, read_frame, write_control_frame, IncomingFrame,
    CONTROL_FINISH,
};
use format::{write_frame, OutputFormat};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;

const DEFAULT_CONTENT_TYPE: &str = "protobuf:dnstap.Dnstap";

fn usage() -> &'static str {
    "conduit-dnstap-tap — dev dnstap listener with extra field support\n\
     \n\
     Usage:\n\
       conduit-dnstap-tap -u <unix-socket-path> [-f log|json|yaml] [--unidirectional]\n\
       conduit-dnstap-tap -a <host:port> [-f log|json|yaml] [--unidirectional]\n\
     \n\
     Options:\n\
       -u, --unix PATH       Unix socket path to bind (removes existing socket file)\n\
       -a, --tcp ADDR        TCP address to bind (e.g. 127.0.0.1:6000)\n\
       -f, --format FMT      Output format: log (default), json, yaml\n\
       -t, --content-type S  Frame Streams content type (default: protobuf:dnstap.Dnstap)\n\
       --unidirectional      Accept START-only clients (no READY/ACCEPT)\n\
       --once                Exit after the first connection closes\n\
       -h, --help            Show this help\n"
}

struct Args {
    unix: Option<PathBuf>,
    tcp: Option<SocketAddr>,
    format: OutputFormat,
    content_type: String,
    unidirectional: bool,
    once: bool,
}

fn parse_args() -> Result<Args> {
    let mut unix = None;
    let mut tcp = None;
    let mut format = OutputFormat::Log;
    let mut content_type = DEFAULT_CONTENT_TYPE.to_string();
    let mut unidirectional = false;
    let mut once = false;

    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{}", usage());
                std::process::exit(0);
            }
            "-u" | "--unix" => {
                unix = Some(PathBuf::from(it.next().context("-u requires path")?));
            }
            "-a" | "--tcp" => {
                let s = it.next().context("-a requires host:port")?;
                tcp = Some(s.parse().context("invalid tcp address")?);
            }
            "-f" | "--format" => {
                let s = it.next().context("-f requires format")?;
                format = OutputFormat::parse(&s)
                    .with_context(|| format!("unknown format '{s}' (use log, json, yaml)"))?;
            }
            "-t" | "--content-type" => {
                content_type = it.next().context("-t requires value")?;
            }
            "--unidirectional" => unidirectional = true,
            "--once" => once = true,
            other => bail!("unknown argument: {other}"),
        }
    }

    match (&unix, &tcp) {
        (Some(_), Some(_)) => bail!("use only one of -u or -a"),
        (None, None) => bail!("one of -u or -a is required"),
        _ => {}
    }

    Ok(Args {
        unix,
        tcp,
        format,
        content_type,
        unidirectional,
        once,
    })
}

fn serve_connection(
    mut stream: impl io::Read + io::Write,
    args: &Args,
    out: &mut impl Write,
) -> Result<()> {
    if args.unidirectional {
        accept_unidirectional(&mut stream, &args.content_type)?;
    } else {
        accept_bidirectional(&mut stream, &args.content_type)?;
    }

    loop {
        match read_frame(&mut stream)? {
            IncomingFrame::Data(payload) => match decode::decode_dnstap(&payload) {
                Ok(frame) => {
                    write_frame(out, args.format, &frame)?;
                    out.flush()?;
                }
                Err(e) => {
                    eprintln!("decode error: {e:#}");
                }
            },
            IncomingFrame::Stop => {
                let _ = write_control_frame(&mut stream, CONTROL_FINISH, None);
                break;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn run_unix(args: &Args) -> Result<()> {
    use std::os::unix::net::UnixListener;

    let path = args.unix.as_ref().expect("unix path");
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("bind unix socket {}", path.display()))?;
    eprintln!(
        "conduit-dnstap-tap: listening on {} (format={:?}, uni={})",
        path.display(),
        args.format,
        args.unidirectional
    );

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for conn in listener.incoming() {
        let mut stream = conn.context("accept")?;
        eprintln!("conduit-dnstap-tap: connection accepted");
        if let Err(e) = serve_connection(&mut stream, args, &mut out) {
            eprintln!("conduit-dnstap-tap: connection error: {e:#}");
        }
        if args.once {
            break;
        }
    }
    Ok(())
}

fn run_tcp(args: &Args) -> Result<()> {
    let addr = args.tcp.expect("tcp addr");
    let listener = std::net::TcpListener::bind(addr).with_context(|| format!("bind tcp {addr}"))?;
    eprintln!(
        "conduit-dnstap-tap: listening on {addr} (format={:?}, uni={})",
        args.format, args.unidirectional
    );

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for conn in listener.incoming() {
        let mut stream = conn.context("accept")?;
        eprintln!("conduit-dnstap-tap: connection accepted");
        if let Err(e) = serve_connection(&mut stream, args, &mut out) {
            eprintln!("conduit-dnstap-tap: connection error: {e:#}");
        }
        if args.once {
            break;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = parse_args()?;
    if args.unix.is_some() {
        #[cfg(unix)]
        return run_unix(&args);
        #[cfg(not(unix))]
        bail!("unix sockets require a unix platform");
    }
    run_tcp(&args)
}
