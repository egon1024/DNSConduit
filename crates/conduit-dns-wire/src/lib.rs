//! DNS wire-number enums — canonical IANA names, numeric aliases, and YAML selector parsing.
//!
//! Known constants are generated from vendored IANA CSV files; regenerate with
//! `scripts/generate_dns_wire_iana.py`.

#[path = "iana.rs"]
pub mod iana;

pub struct WireEnumEntry {
    pub number: u16,
    pub name: &'static str,
}

fn canonical_from_entries(entries: &[WireEnumEntry], number: u16, unknown_prefix: &str) -> String {
    for entry in entries {
        if entry.number == number {
            return entry.name.to_string();
        }
    }
    format!("{unknown_prefix}{number}")
}

fn parse_name_from_entries(
    entries: &[WireEnumEntry],
    name: &str,
    unknown_prefix: &str,
) -> Option<u16> {
    let trimmed = name.trim();
    for entry in entries {
        if entry.name.eq_ignore_ascii_case(trimmed) {
            return Some(entry.number);
        }
        let alias = format!("{unknown_prefix}{}", entry.number);
        if alias.eq_ignore_ascii_case(trimmed) {
            return Some(entry.number);
        }
    }
    let upper = trimmed.to_ascii_uppercase();
    let prefix = unknown_prefix.to_ascii_uppercase();
    if let Some(rest) = upper.strip_prefix(&prefix) {
        if let Ok(n) = rest.parse::<u16>() {
            return Some(n);
        }
    }
    None
}

fn parse_name_with_aliases(
    entries: &[WireEnumEntry],
    name_aliases: &[(&str, u16)],
    name: &str,
    unknown_prefix: &str,
) -> Option<u16> {
    parse_name_from_entries(entries, name, unknown_prefix).or_else(|| {
        let trimmed = name.trim();
        name_aliases
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(trimmed))
            .map(|(_, number)| *number)
    })
}

macro_rules! impl_wire_enum {
    (
        $(#[$struct_meta:meta])*
        pub struct $Wire:ident($num_ty:ty);
        known: $known:ident;
        parse_aliases: $parse_aliases:ident;
    ) => {
        $(#[$struct_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $Wire(pub $num_ty);

        impl $Wire {
            pub fn new(number: $num_ty) -> Self {
                Self(number)
            }

            pub fn number(self) -> $num_ty {
                self.0
            }

            pub fn from_wire(number: u16) -> Result<Self, String> {
                if !(0..=65535).contains(&(number as i64)) {
                    return Err(format!(
                        "{} number out of range: {number}",
                        stringify!($Wire)
                    ));
                }
                Ok(Self(number as $num_ty))
            }

            pub fn canonical_name(self) -> String {
                Self::name_for(self.0 as u16)
            }

            pub fn name_for(number: u16) -> String {
                canonical_from_entries($known, number, Self::UNKNOWN_PREFIX)
            }

            pub fn parse_name(name: &str) -> Option<Self> {
                parse_name_with_aliases($known, $parse_aliases, name, Self::UNKNOWN_PREFIX)
                    .map(|n| Self(n as $num_ty))
            }

            pub fn parse_name_or_err(name: &str) -> Result<Self, String> {
                Self::parse_name(name).ok_or_else(|| {
                    format!(
                        "unknown {} value '{name}' (use IANA name or {}N)",
                        stringify!($Wire),
                        Self::UNKNOWN_PREFIX
                    )
                })
            }
        }

        impl From<u16> for $Wire {
            fn from(number: u16) -> Self {
                Self(number as $num_ty)
            }
        }
    };
}

use iana::{
    DNS_OPCODE_PARSE_ALIASES, EDNS_OPTION_CODE_PARSE_ALIASES, KNOWN_DNS_OPCODES,
    KNOWN_EDNS_OPTION_CODES, KNOWN_QUERY_CLASSES, KNOWN_RCODES, KNOWN_RECORD_TYPES,
    QUERY_CLASS_PARSE_ALIASES, RCODE_PARSE_ALIASES, RECORD_TYPE_PARSE_ALIASES,
};

impl_wire_enum! {
    pub struct RecordType(u16);
    known: KNOWN_RECORD_TYPES;
    parse_aliases: RECORD_TYPE_PARSE_ALIASES;
}

impl RecordType {
    const UNKNOWN_PREFIX: &'static str = "TYPE";
}

impl_wire_enum! {
    pub struct Rcode(u16);
    known: KNOWN_RCODES;
    parse_aliases: RCODE_PARSE_ALIASES;
}

impl Rcode {
    const UNKNOWN_PREFIX: &'static str = "RCODE";
}

impl_wire_enum! {
    pub struct QueryClass(u16);
    known: KNOWN_QUERY_CLASSES;
    parse_aliases: QUERY_CLASS_PARSE_ALIASES;
}

impl QueryClass {
    const UNKNOWN_PREFIX: &'static str = "CLASS";
}

impl_wire_enum! {
    pub struct DnsOpcode(u8);
    known: KNOWN_DNS_OPCODES;
    parse_aliases: DNS_OPCODE_PARSE_ALIASES;
}

impl DnsOpcode {
    const UNKNOWN_PREFIX: &'static str = "OPCODE";
}

impl_wire_enum! {
    pub struct EdnsOptionCode(u16);
    known: KNOWN_EDNS_OPTION_CODES;
    parse_aliases: EDNS_OPTION_CODE_PARSE_ALIASES;
}

impl EdnsOptionCode {
    const UNKNOWN_PREFIX: &'static str = "CODE";
}

pub fn qtype_canonical_name(number: u16) -> String {
    RecordType::name_for(number)
}

pub fn rcode_canonical_name(number: u16) -> String {
    Rcode::name_for(number)
}

pub fn qclass_canonical_name(number: u16) -> String {
    QueryClass::name_for(number)
}

/// Parse a YAML selector `value` for wire-enum selector types.
pub fn parse_selector_wire_value(selector_type: &str, value: &str) -> Result<u16, String> {
    match selector_type {
        "qtype" => RecordType::parse_name_or_err(value).map(RecordType::number),
        "rcode" => Rcode::parse_name_or_err(value).map(Rcode::number),
        "qclass" => QueryClass::parse_name_or_err(value).map(QueryClass::number),
        "opcode" => DnsOpcode::parse_name_or_err(value).map(|o| o.number() as u16),
        "edns_option" => EdnsOptionCode::parse_name_or_err(value).map(EdnsOptionCode::number),
        other => Err(format!("selector type '{other}' is not a wire-enum selector")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_enums_support_name_and_numeric_aliases() {
        assert_eq!(RecordType::parse_name("A").map(RecordType::number), Some(1));
        assert_eq!(
            RecordType::parse_name("TYPE1").map(RecordType::number),
            Some(1)
        );
        assert_eq!(
            Rcode::parse_name("SERVFAIL").map(Rcode::number),
            Some(2)
        );
        assert_eq!(Rcode::parse_name("RCODE2").map(Rcode::number), Some(2));
        assert_eq!(QueryClass::parse_name("IN").map(QueryClass::number), Some(1));
        assert_eq!(
            QueryClass::parse_name("CLASS1").map(QueryClass::number),
            Some(1)
        );
        assert_eq!(
            DnsOpcode::parse_name("QUERY").map(DnsOpcode::number),
            Some(0)
        );
        assert_eq!(
            DnsOpcode::parse_name("OPCODE0").map(DnsOpcode::number),
            Some(0)
        );
        assert_eq!(
            EdnsOptionCode::parse_name("COOKIE").map(EdnsOptionCode::number),
            Some(10)
        );
        assert_eq!(
            EdnsOptionCode::parse_name("CODE10").map(EdnsOptionCode::number),
            Some(10)
        );
    }

    #[test]
    fn rcode_parse_name_covers_selectors() {
        assert_eq!(Rcode::parse_name("NXDOMAIN").map(Rcode::number), Some(3));
        assert_eq!(Rcode::parse_name("RCODE5").map(Rcode::number), Some(5));
        assert_eq!(Rcode::parse_name("BADSIG").map(Rcode::number), Some(16));
        assert_eq!(Rcode::parse_name("DSOTYPENI").map(Rcode::number), Some(11));
        assert_eq!(Rcode::parse_name("UNKNOWN"), None);
    }

    #[test]
    fn edns_option_parse_name_covers_friendly_aliases() {
        assert_eq!(
            EdnsOptionCode::parse_name("UMBRELLA").map(EdnsOptionCode::number),
            Some(20292)
        );
        assert_eq!(
            EdnsOptionCode::parse_name("CLIENT_SUBNET").map(EdnsOptionCode::number),
            Some(8)
        );
    }

    #[test]
    fn record_type_covers_iana_assignments() {
        assert!(KNOWN_RECORD_TYPES.len() >= 99);
        assert_eq!(RecordType::name_for(65305), "ANAME");
    }

    #[test]
    fn type1_matches_a_in_selector_parse() {
        assert_eq!(parse_selector_wire_value("qtype", "TYPE1").unwrap(), 1);
        assert_eq!(parse_selector_wire_value("qtype", "A").unwrap(), 1);
    }
}
