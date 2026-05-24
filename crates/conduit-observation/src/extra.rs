//! JSON `Dnstap.extra` builder from configured field lists.

use crate::compile::{CompiledSinkInstance, ExtraField, TagExportMode};
use crate::view::TxnExtraSource;
use std::fmt::Write;

pub fn build_extra_json(
    instance: &CompiledSinkInstance,
    source: &TxnExtraSource,
) -> Option<Vec<u8>> {
    if instance.extra_fields.is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push('{');
    let mut first = true;
    for field in &instance.extra_fields {
        match field {
            ExtraField::Pool => {
                if let Some(pool) = &source.pool {
                    append_field(&mut out, &mut first, "pool", |o| {
                        write!(o, "\"{}\"", json_escape(pool)).ok()
                    });
                }
            }
            ExtraField::Backend => {
                if let Some(backend) = &source.backend {
                    append_field(&mut out, &mut first, "backend", |o| {
                        write!(o, "\"{}\"", json_escape(backend)).ok()
                    });
                }
            }
            ExtraField::AttemptCount => {
                append_field(&mut out, &mut first, "attempt_count", |o| {
                    write!(o, "{}", source.attempt_count).ok()
                });
            }
            ExtraField::TxnId => {
                append_field(&mut out, &mut first, "txn_id", |o| {
                    write!(o, "{}", source.txn_id).ok()
                });
            }
            ExtraField::Qname => {
                if let Some(qname) = &source.qname {
                    append_field(&mut out, &mut first, "qname", |o| {
                        write!(o, "\"{}\"", json_escape(qname)).ok()
                    });
                }
            }
            ExtraField::Rcode => {
                if let Some(rcode) = &source.rcode_label {
                    append_field(&mut out, &mut first, "rcode", |o| {
                        write!(o, "\"{}\"", json_escape(rcode)).ok()
                    });
                }
            }
            ExtraField::Client => {
                append_field(&mut out, &mut first, "client", |o| {
                    write!(o, "\"{}\"", json_escape(&source.client)).ok()
                });
            }
            ExtraField::Tags => {
                if let Some(tags_json) = build_tags_json(source, &instance.tag_export) {
                    append_field(&mut out, &mut first, "tags", |o| {
                        o.push_str(&tags_json);
                        Some(())
                    });
                }
            }
            ExtraField::SinkName => {
                append_field(&mut out, &mut first, "sink_name", |o| {
                    write!(o, "\"{}\"", json_escape(&instance.name)).ok()
                });
            }
        }
    }
    out.push('}');
    Some(out.into_bytes())
}

fn build_tags_json(source: &TxnExtraSource, mode: &TagExportMode) -> Option<String> {
    let mut out = String::new();
    out.push('{');
    let mut first = true;
    match mode {
        TagExportMode::All => {
            for (key, value) in &source.tag_bools {
                if *value {
                    append_field(&mut out, &mut first, key, |o| {
                        o.push_str("true");
                        Some(())
                    });
                }
            }
            for (key, value) in &source.tag_strings {
                append_field(&mut out, &mut first, key, |o| {
                    write!(o, "\"{}\"", json_escape(value)).ok()
                });
            }
        }
        TagExportMode::Keys(keys) => {
            for key in keys {
                if let Some(value) = source.tag_bools.iter().find(|(k, _)| k == key) {
                    if value.1 {
                        append_field(&mut out, &mut first, key, |o| {
                            o.push_str("true");
                            Some(())
                        });
                    }
                } else if let Some(value) = source.tag_strings.iter().find(|(k, _)| k == key) {
                    append_field(&mut out, &mut first, key, |o| {
                        write!(o, "\"{}\"", json_escape(&value.1)).ok()
                    });
                }
            }
        }
    }
    if first {
        return None;
    }
    out.push('}');
    Some(out)
}

fn append_field(
    out: &mut String,
    first: &mut bool,
    key: &str,
    mut write_val: impl FnMut(&mut String) -> Option<()>,
) {
    if !*first {
        out.push(',');
    }
    *first = false;
    write!(out, "\"{}\":", json_escape(key)).ok();
    write_val(out);
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                write!(out, "\\u{:04x}", c as u32).ok();
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_one_sink;
    use conduit_proto::config::ObservationSink;

    fn source_with_tags() -> TxnExtraSource {
        TxnExtraSource {
            pool: Some("default".into()),
            backend: Some("127.0.0.1:5300".into()),
            attempt_count: 2,
            txn_id: 42,
            qname: Some("example.com.".into()),
            rcode_label: Some("NOERROR".into()),
            client: "127.0.0.1:1234".into(),
            tag_bools: vec![("vip".into(), true), ("debug".into(), false)],
            tag_strings: vec![("tenant".into(), "acme".into())],
        }
    }

    #[test]
    fn builds_pool_and_attempt_count() {
        let instance = compile_one_sink(&ObservationSink {
            r#type: "dnstap".into(),
            export_id: "x".into(),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec!["response".into()],
            filters: None,
            extra_fields: vec!["pool".into(), "attempt_count".into()],
            extra_tags: vec![],
            name: None,
            connect_retry: None,
        })
        .unwrap();
        let json =
            String::from_utf8(build_extra_json(&instance, &source_with_tags()).unwrap()).unwrap();
        assert!(json.contains("\"pool\":\"default\""));
        assert!(json.contains("\"attempt_count\":2"));
        assert!(!json.contains("tags"));
    }

    #[test]
    fn all_tags_when_star() {
        let instance = compile_one_sink(&ObservationSink {
            r#type: "dnstap".into(),
            export_id: "x".into(),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec![],
            filters: None,
            extra_fields: vec!["tags".into()],
            extra_tags: vec!["*".into()],
            name: None,
            connect_retry: None,
        })
        .unwrap();
        let json =
            String::from_utf8(build_extra_json(&instance, &source_with_tags()).unwrap()).unwrap();
        assert!(json.contains("\"vip\":true"));
        assert!(json.contains("\"tenant\":\"acme\""));
        assert!(!json.contains("debug"));
    }

    #[test]
    fn filtered_tags() {
        let instance = compile_one_sink(&ObservationSink {
            r#type: "dnstap".into(),
            export_id: "x".into(),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec![],
            filters: None,
            extra_fields: vec!["tags".into()],
            extra_tags: vec!["tenant".into()],
            name: None,
            connect_retry: None,
        })
        .unwrap();
        let json =
            String::from_utf8(build_extra_json(&instance, &source_with_tags()).unwrap()).unwrap();
        assert!(json.contains("\"tenant\":\"acme\""));
        assert!(!json.contains("vip"));
    }

    #[test]
    fn sink_name_extra_field() {
        let instance = compile_one_sink(&ObservationSink {
            r#type: "dnstap".into(),
            name: Some("prod-tap".into()),
            export_id: "wire-id".into(),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec![],
            filters: None,
            extra_fields: vec!["sink_name".into()],
            extra_tags: vec![],
            connect_retry: None,
        })
        .unwrap();
        let json =
            String::from_utf8(build_extra_json(&instance, &source_with_tags()).unwrap()).unwrap();
        assert!(json.contains("\"sink_name\":\"prod-tap\""));
    }

    #[test]
    fn empty_extra_fields_omits_bytes() {
        let instance = compile_one_sink(&ObservationSink {
            r#type: "dnstap".into(),
            export_id: "x".into(),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec![],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            name: None,
            connect_retry: None,
        })
        .unwrap();
        assert!(build_extra_json(&instance, &source_with_tags()).is_none());
    }
}
