//! DNS wire-number enums for Rule Rhai — newtypes over `conduit-dns-wire` with Rhai registration.

use conduit_dns_wire::WireEnumEntry;
use rhai::{CustomType, Dynamic, Engine, EvalAltResult, Module, TypeBuilder};

pub use conduit_dns_wire::{qclass_canonical_name, qtype_canonical_name, rcode_canonical_name};

use conduit_dns_wire::iana::{
    DNS_OPCODE_PARSE_ALIASES, EDNS_OPTION_CODE_PARSE_ALIASES, KNOWN_DNS_OPCODES,
    KNOWN_EDNS_OPTION_CODES, KNOWN_QUERY_CLASSES, KNOWN_RCODES, KNOWN_RECORD_TYPES,
    QUERY_CLASS_PARSE_ALIASES, RCODE_PARSE_ALIASES, RECORD_TYPE_PARSE_ALIASES,
};

macro_rules! impl_wire_newtype {
    ($Name:ident, $Inner:ty, $rhai:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $Name(pub $Inner);

        impl $Name {
            pub fn number(self) -> u16 {
                self.0.number() as u16
            }

            pub fn canonical_name(self) -> String {
                self.0.canonical_name()
            }

            pub fn parse_name(name: &str) -> Option<Self> {
                <$Inner>::parse_name(name).map(Self)
            }

            fn from_number_rhai(number: i64) -> Result<Self, Box<EvalAltResult>> {
                if !(0..=65535).contains(&number) {
                    return Err(format!("{rhai} number out of range: {number}", rhai = $rhai).into());
                }
                Ok(Self(<$Inner>::from(number as u16)))
            }
        }

        impl From<u16> for $Name {
            fn from(number: u16) -> Self {
                Self(<$Inner>::from(number))
            }
        }

        impl CustomType for $Name {
            fn build(mut builder: TypeBuilder<Self>) {
                builder
                    .with_name($rhai)
                    .on_print(|v| v.canonical_name())
                    .on_debug(|v| format!("{}({}: {})", $rhai, v.number(), v.canonical_name()))
                    .with_fn("number", |v: $Name| v.number() as i64)
                    .with_fn("name", |v: $Name| v.canonical_name())
                    .with_fn("==", |a: $Name, b: $Name| a == b)
                    .with_fn("!=", |a: $Name, b: $Name| a != b);
            }
        }
    };
}

impl_wire_newtype!(RecordType, conduit_dns_wire::RecordType, "RecordType");
impl_wire_newtype!(Rcode, conduit_dns_wire::Rcode, "Rcode");
impl_wire_newtype!(QueryClass, conduit_dns_wire::QueryClass, "QueryClass");
impl_wire_newtype!(EdnsOptionCode, conduit_dns_wire::EdnsOptionCode, "EdnsOptionCode");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DnsOpcode(pub conduit_dns_wire::DnsOpcode);

impl DnsOpcode {
    pub fn number(self) -> u8 {
        self.0.number()
    }

    pub fn canonical_name(self) -> String {
        self.0.canonical_name()
    }

    pub fn parse_name(name: &str) -> Option<Self> {
        conduit_dns_wire::DnsOpcode::parse_name(name).map(Self)
    }

    fn from_number_rhai(number: i64) -> Result<Self, Box<EvalAltResult>> {
        if !(0..=15).contains(&number) {
            return Err(format!("DnsOpcode number out of range: {number}").into());
        }
        Ok(Self(conduit_dns_wire::DnsOpcode(number as u8)))
    }
}

impl CustomType for DnsOpcode {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("DnsOpcode")
            .on_print(|v| v.canonical_name())
            .on_debug(|v| format!("DnsOpcode({}: {})", v.number(), v.canonical_name()))
            .with_fn("number", |v: DnsOpcode| v.number() as i64)
            .with_fn("name", |v: DnsOpcode| v.canonical_name())
            .with_fn("==", |a: DnsOpcode, b: DnsOpcode| a == b)
            .with_fn("!=", |a: DnsOpcode, b: DnsOpcode| a != b);
    }
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

pub fn register_record_type_api(engine: &mut Engine) {
    register_wire_enum_module::<RecordType>(
        engine,
        "RecordType",
        "TYPE",
        KNOWN_RECORD_TYPES,
        RecordType::from_number_rhai,
        RECORD_TYPE_PARSE_ALIASES,
        |n| RecordType(conduit_dns_wire::RecordType::from(n)),
    );
}

pub fn register_rcode_api(engine: &mut Engine) {
    register_wire_enum_module::<Rcode>(
        engine,
        "Rcode",
        "RCODE",
        KNOWN_RCODES,
        Rcode::from_number_rhai,
        RCODE_PARSE_ALIASES,
        |n| Rcode(conduit_dns_wire::Rcode::from(n)),
    );
}

pub fn register_query_class_api(engine: &mut Engine) {
    register_wire_enum_module::<QueryClass>(
        engine,
        "QueryClass",
        "CLASS",
        KNOWN_QUERY_CLASSES,
        QueryClass::from_number_rhai,
        QUERY_CLASS_PARSE_ALIASES,
        |n| QueryClass(conduit_dns_wire::QueryClass::from(n)),
    );
}

pub fn register_dns_opcode_api(engine: &mut Engine) {
    register_wire_enum_module::<DnsOpcode>(
        engine,
        "DnsOpcode",
        "OPCODE",
        KNOWN_DNS_OPCODES,
        DnsOpcode::from_number_rhai,
        DNS_OPCODE_PARSE_ALIASES,
        |n| DnsOpcode(conduit_dns_wire::DnsOpcode(n as u8)),
    );
}

pub fn register_edns_option_code_api(engine: &mut Engine) {
    register_wire_enum_module::<EdnsOptionCode>(
        engine,
        "EdnsOptionCode",
        "CODE",
        KNOWN_EDNS_OPTION_CODES,
        EdnsOptionCode::from_number_rhai,
        EDNS_OPTION_CODE_PARSE_ALIASES,
        |n| EdnsOptionCode(conduit_dns_wire::EdnsOptionCode::from(n)),
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
    use rhai::Engine;

    #[test]
    fn wire_enums_support_name_and_numeric_aliases_in_rhai() {
        let mut engine = Engine::new();
        register_dns_wire_api(&mut engine);
        assert!(engine
            .eval::<bool>("RecordType::A == RecordType::TYPE1")
            .unwrap());
        assert!(engine
            .eval::<bool>("Rcode::SERVFAIL == Rcode::RCODE2")
            .unwrap());
    }
}
