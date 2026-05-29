//! Low-cardinality label helpers for built-in metrics.

use std::net::SocketAddr;

pub fn ip_family_label(addr: &SocketAddr) -> &'static str {
    if addr.is_ipv4() {
        "v4"
    } else {
        "v6"
    }
}

pub fn qtype_label(qtype: u16) -> String {
    match qtype {
        1 => "A".into(),
        28 => "AAAA".into(),
        15 => "MX".into(),
        16 => "TXT".into(),
        2 => "NS".into(),
        5 => "CNAME".into(),
        6 => "SOA".into(),
        12 => "PTR".into(),
        33 => "SRV".into(),
        255 => "ANY".into(),
        n => format!("TYPE{n}"),
    }
}

pub fn qclass_label(qclass: u16) -> String {
    match qclass {
        1 => "IN".into(),
        3 => "CH".into(),
        4 => "HS".into(),
        n => format!("CLASS{n}"),
    }
}

pub fn rcode_class_label(rcode: Option<u16>) -> &'static str {
    match rcode {
        Some(0) => "NOERROR",
        Some(3) => "NXDOMAIN",
        Some(2) => "SERVFAIL",
        Some(5) => "REFUSED",
        Some(1) => "OTHER",
        _ => "OTHER",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qtype_label_maps_a() {
        assert_eq!(qtype_label(1), "A");
    }
}
