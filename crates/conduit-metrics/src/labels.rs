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

/// Coarse response-code bucket for `metrics.profile: minimal`.
pub fn rcode_class_label(rcode: Option<u16>) -> &'static str {
    match rcode {
        Some(0) => "NOERROR",
        Some(3) => "NXDOMAIN",
        Some(2) => "SERVFAIL",
        Some(5) => "REFUSED",
        _ => "OTHER",
    }
}

/// Per-IANA response code name for `metrics.profile: full` (0–23); unknown → `OTHER`.
pub fn rcode_label(rcode: Option<u16>) -> &'static str {
    match rcode {
        Some(0) => "NOERROR",
        Some(1) => "FORMERR",
        Some(2) => "SERVFAIL",
        Some(3) => "NXDOMAIN",
        Some(4) => "NOTIMP",
        Some(5) => "REFUSED",
        Some(6) => "YXDOMAIN",
        Some(7) => "YXRRSET",
        Some(8) => "NXRRSET",
        Some(9) => "NOTAUTH",
        Some(10) => "NOTZONE",
        Some(16) => "BADSIG",
        Some(17) => "BADKEY",
        Some(18) => "BADTIME",
        Some(19) => "BADMODE",
        Some(20) => "BADNAME",
        Some(21) => "BADALG",
        Some(22) => "BADTRUNC",
        Some(23) => "BADCOOKIE",
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

    #[test]
    fn rcode_class_groups_uncommon_codes() {
        assert_eq!(rcode_class_label(Some(1)), "OTHER");
        assert_eq!(rcode_class_label(Some(9)), "OTHER");
        assert_eq!(rcode_class_label(Some(0)), "NOERROR");
        assert_eq!(rcode_class_label(Some(5)), "REFUSED");
    }

    #[test]
    fn rcode_label_maps_iana_codes() {
        assert_eq!(rcode_label(Some(1)), "FORMERR");
        assert_eq!(rcode_label(Some(9)), "NOTAUTH");
        assert_eq!(rcode_label(Some(23)), "BADCOOKIE");
        assert_eq!(rcode_label(Some(11)), "OTHER");
        assert_eq!(rcode_label(None), "OTHER");
    }
}
