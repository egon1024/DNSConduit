//! DNS wire-number enums for Rule Rhai — canonical names, numeric aliases, and registration.

use rhai::{CustomType, Dynamic, Engine, EvalAltResult, Module, TypeBuilder};

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

fn register_wire_enum_module<T>(
    engine: &mut Engine,
    module_name: &str,
    alias_prefix: &str,
    entries: &[WireEnumEntry],
    from_number: fn(i64) -> Result<T, Box<EvalAltResult>>,
    extra_aliases: &[(&str, u16)],
    make: fn(u16) -> T,
) where
    T: CustomType + Clone + PartialEq + 'static,
{
    engine.build_type::<T>();

    let mut module = Module::new();
    module.set_custom_type::<T>(module_name);
    for entry in entries {
        let value = Dynamic::from(make(entry.number));
        let _ = module.set_var(entry.name, value.clone());
        let alias = format!("{alias_prefix}{}", entry.number);
        let _ = module.set_var(alias, value);
    }
    for (name, number) in extra_aliases {
        let _ = module.set_var(*name, Dynamic::from(make(*number)));
    }
    module.set_native_fn("from_number", from_number);
    engine.register_static_module(module_name, module.into());
}

macro_rules! impl_wire_enum {
    (
        $(#[$struct_meta:meta])*
        pub struct $Wire:ident($num_ty:ty);
        rhai: $rhai_name:literal;
        known: $known:ident;
        entries: [$($num:expr => $label:literal),* $(,)?];
    ) => {
        $(#[$struct_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $Wire(pub $num_ty);

        const $known: &[WireEnumEntry] = &[
            $(WireEnumEntry { number: $num, name: $label },)*
        ];

        impl $Wire {
            pub fn new(number: $num_ty) -> Self {
                Self(number)
            }

            pub fn number(self) -> $num_ty {
                self.0
            }

            pub fn from_number(number: i64) -> Result<Self, Box<EvalAltResult>> {
                if !(0..=65535).contains(&number) {
                    return Err(format!(
                        "{rhai} number out of range: {number}",
                        rhai = $rhai_name
                    )
                    .into());
                }
                Ok(Self(number as $num_ty))
            }

            pub fn canonical_name(self) -> String {
                $Wire::name_for(self.0 as u16)
            }

            pub fn name_for(number: u16) -> String {
                canonical_from_entries($known, number, $Wire::UNKNOWN_PREFIX)
            }

            pub fn parse_name(name: &str) -> Option<Self> {
                parse_name_from_entries($known, name, Self::UNKNOWN_PREFIX)
                    .map(|n| Self(n as $num_ty))
            }
        }

        impl From<u16> for $Wire {
            fn from(number: u16) -> Self {
                Self(number as $num_ty)
            }
        }

        impl CustomType for $Wire {
            fn build(mut builder: TypeBuilder<Self>) {
                builder
                    .with_name($rhai_name)
                    .on_print(|v| v.canonical_name())
                    .on_debug(|v| format!("{}({}: {})", $rhai_name, v.0, v.canonical_name()))
                    .with_fn("number", |v: $Wire| v.0 as i64)
                    .with_fn("name", |v: $Wire| v.canonical_name())
                    .with_fn("==", |a: $Wire, b: $Wire| a == b)
                    .with_fn("!=", |a: $Wire, b: $Wire| a != b);
            }
        }
    };
}

impl_wire_enum! {
    pub struct RecordType(u16);
    rhai: "RecordType";
    known: KNOWN_RECORD_TYPES;
    entries: [
        0 => "ZERO",
        1 => "A",
        2 => "NS",
        5 => "CNAME",
        6 => "SOA",
        10 => "NULL",
        12 => "PTR",
        13 => "HINFO",
        15 => "MX",
        16 => "TXT",
        24 => "SIG",
        25 => "KEY",
        28 => "AAAA",
        33 => "SRV",
        35 => "NAPTR",
        41 => "OPT",
        43 => "DS",
        44 => "SSHFP",
        46 => "RRSIG",
        47 => "NSEC",
        48 => "DNSKEY",
        50 => "NSEC3",
        51 => "NSEC3PARAM",
        52 => "TLSA",
        59 => "CDS",
        60 => "CDNSKEY",
        61 => "OPENPGPKEY",
        62 => "CSYNC",
        64 => "SVCB",
        65 => "HTTPS",
        250 => "TSIG",
        251 => "IXFR",
        252 => "AXFR",
        255 => "ANY",
        257 => "CAA",
        65305 => "ANAME",
    ];
}

impl RecordType {
    const UNKNOWN_PREFIX: &'static str = "TYPE";
}

impl_wire_enum! {
    pub struct Rcode(u16);
    rhai: "Rcode";
    known: KNOWN_RCODES;
    entries: [
        0 => "NOERROR",
        1 => "FORMERR",
        2 => "SERVFAIL",
        3 => "NXDOMAIN",
        4 => "NOTIMP",
        5 => "REFUSED",
        6 => "YXDOMAIN",
        7 => "YXRRSET",
        8 => "NXRRSET",
        9 => "NOTAUTH",
        10 => "NOTZONE",
        16 => "BADVERS",
        17 => "BADKEY",
        18 => "BADTIME",
        19 => "BADMODE",
        20 => "BADNAME",
        21 => "BADALG",
        22 => "BADTRUNC",
        23 => "BADCOOKIE",
    ];
}

impl Rcode {
    const UNKNOWN_PREFIX: &'static str = "RCODE";
}

impl_wire_enum! {
    pub struct QueryClass(u16);
    rhai: "QueryClass";
    known: KNOWN_QUERY_CLASSES;
    entries: [
        1 => "IN",
        3 => "CH",
        4 => "HS",
        254 => "NONE",
        255 => "ANY",
    ];
}

impl QueryClass {
    const UNKNOWN_PREFIX: &'static str = "CLASS";
}

impl_wire_enum! {
    pub struct DnsOpcode(u8);
    rhai: "DnsOpcode";
    known: KNOWN_DNS_OPCODES;
    entries: [
        0 => "QUERY",
        1 => "IQUERY",
        2 => "STATUS",
        4 => "NOTIFY",
        5 => "UPDATE",
    ];
}

impl DnsOpcode {
    const UNKNOWN_PREFIX: &'static str = "OPCODE";
}

impl_wire_enum! {
    pub struct EdnsOptionCode(u16);
    rhai: "EdnsOptionCode";
    known: KNOWN_EDNS_OPTION_CODES;
    entries: [
        0 => "ZERO",
        1 => "LLQ",
        2 => "UL",
        3 => "NSID",
        5 => "DAU",
        6 => "DHU",
        7 => "N3U",
        8 => "CLIENT_SUBNET",
        9 => "EXPIRE",
        10 => "COOKIE",
        11 => "TCP_KEEPALIVE",
        12 => "PADDING",
        13 => "CHAIN",
        14 => "KEY_TAG",
        15 => "EDE",
        16 => "CLIENT_TAG",
        17 => "SERVER_TAG",
        18 => "UMBRELLA",
        26946 => "DEVICEID",
    ];
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

pub fn register_record_type_api(engine: &mut Engine) {
    register_wire_enum_module::<RecordType>(
        engine,
        "RecordType",
        "TYPE",
        KNOWN_RECORD_TYPES,
        RecordType::from_number,
        &[],
        RecordType::from,
    );
}

pub fn register_rcode_api(engine: &mut Engine) {
    register_wire_enum_module::<Rcode>(
        engine,
        "Rcode",
        "RCODE",
        KNOWN_RCODES,
        Rcode::from_number,
        &[("BADSIG", 16)],
        Rcode::from,
    );
}

pub fn register_query_class_api(engine: &mut Engine) {
    register_wire_enum_module::<QueryClass>(
        engine,
        "QueryClass",
        "CLASS",
        KNOWN_QUERY_CLASSES,
        QueryClass::from_number,
        &[],
        QueryClass::from,
    );
}

pub fn register_dns_opcode_api(engine: &mut Engine) {
    register_wire_enum_module::<DnsOpcode>(
        engine,
        "DnsOpcode",
        "OPCODE",
        KNOWN_DNS_OPCODES,
        DnsOpcode::from_number,
        &[],
        |n| DnsOpcode(n as u8),
    );
}

pub fn register_edns_option_code_api(engine: &mut Engine) {
    register_wire_enum_module::<EdnsOptionCode>(
        engine,
        "EdnsOptionCode",
        "CODE",
        KNOWN_EDNS_OPTION_CODES,
        EdnsOptionCode::from_number,
        &[],
        EdnsOptionCode::from,
    );
}

pub fn register_dns_wire_api(engine: &mut Engine) {
    register_record_type_api(engine);
    register_rcode_api(engine);
    register_query_class_api(engine);
    register_dns_opcode_api(engine);
    register_edns_option_code_api(engine);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_enums_support_name_and_numeric_aliases() {
        let mut engine = Engine::new();
        register_dns_wire_api(&mut engine);
        assert!(engine
            .eval::<bool>("RecordType::A == RecordType::TYPE1")
            .unwrap());
        assert!(engine
            .eval::<bool>("Rcode::SERVFAIL == Rcode::RCODE2")
            .unwrap());
        assert!(engine
            .eval::<bool>("QueryClass::IN == QueryClass::CLASS1")
            .unwrap());
        assert!(engine
            .eval::<bool>("DnsOpcode::QUERY == DnsOpcode::OPCODE0")
            .unwrap());
        assert!(engine
            .eval::<bool>("EdnsOptionCode::COOKIE == EdnsOptionCode::CODE10")
            .unwrap());
        assert!(engine
            .eval::<bool>("Rcode::BADSIG == Rcode::RCODE16")
            .unwrap());
    }

    #[test]
    fn rcode_parse_name_covers_selectors() {
        assert_eq!(Rcode::parse_name("NXDOMAIN").map(Rcode::number), Some(3));
        assert_eq!(Rcode::parse_name("RCODE5").map(Rcode::number), Some(5));
        assert_eq!(Rcode::parse_name("UNKNOWN"), None);
    }
}
