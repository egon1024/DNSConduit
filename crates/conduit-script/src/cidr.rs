//! IPv4/IPv6 longest-prefix CIDR store: file-backed `type: cidr` data source
//! and the host `lookup_ip` primitive (client-acls design decision 2).

use crate::error::ScriptError;
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

/// Longest-prefix match over separate IPv4/IPv6 prefix lists. Empty values
/// are valid entries (a hit with `Some("")`); a miss is `None`.
#[derive(Debug, Clone, Default)]
pub struct CidrTable {
    v4: Vec<(Ipv4Net, String)>,
    v6: Vec<(Ipv6Net, String)>,
}

impl CidrTable {
    pub fn lookup(&self, addr: IpAddr) -> Option<&str> {
        match addr {
            IpAddr::V4(a) => longest_match_v4(&self.v4, a),
            IpAddr::V6(a) => longest_match_v6(&self.v6, a),
        }
    }

    fn insert(&mut self, net: IpNet, value: String) {
        match net {
            IpNet::V4(n) => self.v4.push((n, value)),
            IpNet::V6(n) => self.v6.push((n, value)),
        }
    }
}

fn longest_match_v4(entries: &[(Ipv4Net, String)], addr: Ipv4Addr) -> Option<&str> {
    entries
        .iter()
        .filter(|(net, _)| net.contains(&addr))
        .max_by_key(|(net, _)| net.prefix_len())
        .map(|(_, v)| v.as_str())
}

fn longest_match_v6(entries: &[(Ipv6Net, String)], addr: Ipv6Addr) -> Option<&str> {
    entries
        .iter()
        .filter(|(net, _)| net.contains(&addr))
        .max_by_key(|(net, _)| net.prefix_len())
        .map(|(_, v)| v.as_str())
}

/// Parses `10.0.0.0/8`-style prefixes, or a bare `IpAddr` as `/32` or `/128`.
fn parse_prefix(token: &str) -> Result<IpNet, String> {
    if let Ok(net) = token.parse::<IpNet>() {
        return Ok(net);
    }
    let addr: IpAddr = token
        .parse()
        .map_err(|_| format!("invalid CIDR prefix or address '{token}'"))?;
    Ok(match addr {
        IpAddr::V4(a) => IpNet::V4(Ipv4Net::new(a, 32).expect("/32 is always valid")),
        IpAddr::V6(a) => IpNet::V6(Ipv6Net::new(a, 128).expect("/128 is always valid")),
    })
}

/// Loads a `type: cidr` file into a [`CidrTable`], enforcing load-safety
/// limits and failing closed on any violation. One prefix per line; an
/// optional whitespace-separated value follows the prefix; `#` comments and
/// blank lines are ignored. A line with no value stores `"1"` so a Rhai
/// membership hit is never mistaken for the miss sentinel (`""`).
///
/// Returns the table and the number of file bytes read (for aggregate
/// `data_source_limits` accounting alongside CSV tables).
pub fn load_cidr_table(
    path: &Path,
    name: &str,
    max_file_bytes: u64,
    max_entries: u64,
    max_value_bytes: u32,
) -> Result<(CidrTable, u64), ScriptError> {
    let read_err = |e: std::io::Error| ScriptError::DataSource {
        name: name.to_string(),
        message: format!("failed to read {}: {e}", path.display()),
    };
    let file = std::fs::File::open(path).map_err(read_err)?;
    // Read at most max_file_bytes + 1 so an oversized file is detected without
    // an unbounded read.
    let read_cap = max_file_bytes.saturating_add(1);
    let mut buf = Vec::new();
    file.take(read_cap)
        .read_to_end(&mut buf)
        .map_err(read_err)?;
    if buf.len() as u64 > max_file_bytes {
        return Err(ScriptError::DataSource {
            name: name.to_string(),
            message: format!(
                "file {} exceeds max_file_bytes {max_file_bytes}",
                path.display()
            ),
        });
    }
    let bytes_read = buf.len() as u64;
    let content = String::from_utf8(buf).map_err(|e| ScriptError::DataSource {
        name: name.to_string(),
        message: format!("failed to read {}: invalid UTF-8: {e}", path.display()),
    })?;

    let mut table = CidrTable::default();
    let mut entry_count: u64 = 0;
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let prefix_tok = fields.next().expect("non-empty line has a first field");
        let value = fields.next().unwrap_or("1");
        if fields.next().is_some() {
            return Err(ScriptError::DataSource {
                name: name.to_string(),
                message: format!("line {}: too many fields", line_no + 1),
            });
        }
        if value.len() as u64 > max_value_bytes as u64 {
            return Err(ScriptError::DataSource {
                name: name.to_string(),
                message: format!(
                    "line {}: value length {} exceeds max_value_bytes {max_value_bytes}",
                    line_no + 1,
                    value.len()
                ),
            });
        }
        let net = parse_prefix(prefix_tok).map_err(|e| ScriptError::DataSource {
            name: name.to_string(),
            message: format!("line {}: {e}", line_no + 1),
        })?;
        entry_count += 1;
        if entry_count > max_entries {
            return Err(ScriptError::DataSource {
                name: name.to_string(),
                message: format!("exceeds max_entries {max_entries}"),
            });
        }
        table.insert(net, value.to_string());
    }
    Ok((table, bytes_read))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn temp_cidr(tag: &str, content: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "conduit-cidr-{tag}-{}-{:?}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        path.push(unique);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn overlapping_prefixes_most_specific_wins() {
        let path = temp_cidr(
            "overlap",
            "10.0.0.0/8 outer\n10.1.0.0/16 inner\n# comment\n\n",
        );
        let (table, _bytes) = load_cidr_table(&path, "t", 1024, 100, 64).unwrap();
        assert_eq!(table.lookup("10.1.2.3".parse().unwrap()), Some("inner"));
        assert_eq!(table.lookup("10.2.2.3".parse().unwrap()), Some("outer"));
    }

    #[test]
    fn v4_and_v6_prefixes_load_and_match() {
        let path = temp_cidr("v4v6", "10.0.0.0/8 v4\n2001:db8::/32 v6\n");
        let (table, _bytes) = load_cidr_table(&path, "t", 1024, 100, 64).unwrap();
        assert_eq!(table.lookup("10.0.0.1".parse().unwrap()), Some("v4"));
        assert_eq!(table.lookup("2001:db8::1".parse().unwrap()), Some("v6"));
    }

    #[test]
    fn bare_ip_loads_as_host_prefix() {
        let path = temp_cidr("bare", "192.0.2.5\n::1\n");
        let (table, _bytes) = load_cidr_table(&path, "t", 1024, 100, 64).unwrap();
        assert_eq!(table.lookup("192.0.2.5".parse().unwrap()), Some("1"));
        assert_eq!(table.lookup("192.0.2.6".parse().unwrap()), None);
        assert_eq!(table.lookup("::1".parse().unwrap()), Some("1"));
    }

    #[test]
    fn miss_returns_none() {
        let path = temp_cidr("miss", "10.0.0.0/8 v4\n");
        let (table, _bytes) = load_cidr_table(&path, "t", 1024, 100, 64).unwrap();
        assert_eq!(table.lookup("192.0.2.1".parse().unwrap()), None);
    }

    #[test]
    fn bad_prefix_syntax_fails_load() {
        let path = temp_cidr("bad", "not-a-prefix\n");
        let err = load_cidr_table(&path, "t", 1024, 100, 64).unwrap_err();
        assert!(
            format!("{err}").contains("invalid CIDR prefix"),
            "got: {err}"
        );
    }

    #[test]
    fn missing_file_fails_load() {
        let path = std::env::temp_dir().join("conduit-cidr-does-not-exist.txt");
        let err = load_cidr_table(&path, "t", 1024, 100, 64).unwrap_err();
        assert!(format!("{err}").contains("failed to read"), "got: {err}");
    }

    #[test]
    fn oversize_file_rejected() {
        let big = "10.0.0.0/8 v\n".repeat(100);
        let path = temp_cidr("oversize", &big);
        let err = load_cidr_table(&path, "t", 8, 100, 64).unwrap_err();
        assert!(format!("{err}").contains("max_file_bytes"), "got: {err}");
    }

    #[test]
    fn too_many_entries_rejected() {
        let content = "10.0.0.0/8 a\n10.1.0.0/16 b\n10.2.0.0/16 c\n";
        let path = temp_cidr("entries", content);
        let err = load_cidr_table(&path, "t", 1024, 2, 64).unwrap_err();
        assert!(format!("{err}").contains("max_entries"), "got: {err}");
    }

    #[test]
    fn oversize_value_rejected() {
        let path = temp_cidr("value", "10.0.0.0/8 aaaaaaaaaa\n");
        let err = load_cidr_table(&path, "t", 1024, 100, 4).unwrap_err();
        assert!(format!("{err}").contains("max_value_bytes"), "got: {err}");
    }
}
