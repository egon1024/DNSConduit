//! DNS wire-number enums for Rule Rhai — canonical names, numeric aliases, and registration.
//!
//! Known constants are generated from vendored IANA CSV files; regenerate with
//! `scripts/generate_dns_wire_iana.py`.

#[path = "iana.rs"]
mod iana;

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
                parse_name_with_aliases($known, $parse_aliases, name, Self::UNKNOWN_PREFIX)
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

use iana::{
    DNS_OPCODE_PARSE_ALIASES, EDNS_OPTION_CODE_PARSE_ALIASES, KNOWN_DNS_OPCODES,
    KNOWN_EDNS_OPTION_CODES, KNOWN_QUERY_CLASSES, KNOWN_RCODES, KNOWN_RECORD_TYPES,
    QUERY_CLASS_PARSE_ALIASES, RCODE_PARSE_ALIASES, RECORD_TYPE_PARSE_ALIASES,
};

impl_wire_enum! {
    pub struct RecordType(u16);
    rhai: "RecordType";
    known: KNOWN_RECORD_TYPES;
    parse_aliases: RECORD_TYPE_PARSE_ALIASES;
}

impl RecordType {
    const UNKNOWN_PREFIX: &'static str = "TYPE";
}

impl_wire_enum! {
    pub struct Rcode(u16);
    rhai: "Rcode";
    known: KNOWN_RCODES;
    parse_aliases: RCODE_PARSE_ALIASES;
}

impl Rcode {
    const UNKNOWN_PREFIX: &'static str = "RCODE";
}

impl_wire_enum! {
    pub struct QueryClass(u16);
    rhai: "QueryClass";
    known: KNOWN_QUERY_CLASSES;
    parse_aliases: QUERY_CLASS_PARSE_ALIASES;
}

impl QueryClass {
    const UNKNOWN_PREFIX: &'static str = "CLASS";
}

impl_wire_enum! {
    pub struct DnsOpcode(u8);
    rhai: "DnsOpcode";
    known: KNOWN_DNS_OPCODES;
    parse_aliases: DNS_OPCODE_PARSE_ALIASES;
}

impl DnsOpcode {
    const UNKNOWN_PREFIX: &'static str = "OPCODE";
}

impl_wire_enum! {
    pub struct EdnsOptionCode(u16);
    rhai: "EdnsOptionCode";
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

pub fn register_record_type_api(engine: &mut Engine) {
    register_wire_enum_module::<RecordType>(
        engine,
        "RecordType",
        "TYPE",
        KNOWN_RECORD_TYPES,
        RecordType::from_number,
        RECORD_TYPE_PARSE_ALIASES,
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
        RCODE_PARSE_ALIASES,
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
        QUERY_CLASS_PARSE_ALIASES,
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
        DNS_OPCODE_PARSE_ALIASES,
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
        EDNS_OPTION_CODE_PARSE_ALIASES,
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
        assert!(engine
            .eval::<bool>("DnsOpcode::DSO == DnsOpcode::OPCODE6")
            .unwrap());
        assert!(engine
            .eval::<bool>("EdnsOptionCode::UMBRELLA == EdnsOptionCode::CODE20292")
            .unwrap());
        assert!(engine
            .eval::<bool>("EdnsOptionCode::UMBRELLA == EdnsOptionCode::UMBRELLA_IDENT")
            .unwrap());
        assert!(engine
            .eval::<bool>("EdnsOptionCode::REPORT_CHANNEL == EdnsOptionCode::CODE18")
            .unwrap());
        assert!(engine
            .eval::<bool>("EdnsOptionCode::CLIENT_SUBNET == EdnsOptionCode::CODE8")
            .unwrap());
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
        assert_eq!(
            EdnsOptionCode::parse_name("EDE").map(EdnsOptionCode::number),
            Some(15)
        );
        assert_eq!(
            EdnsOptionCode::parse_name("REPORT_CHANNEL").map(EdnsOptionCode::number),
            Some(18)
        );
    }

    #[test]
    fn record_type_covers_iana_assignments() {
        assert!(KNOWN_RECORD_TYPES.len() >= 99);
        assert_eq!(RecordType::name_for(65305), "ANAME");
    }
}
