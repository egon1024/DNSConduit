//! Probe query construction and response validation (design §D4).
//!
//! A probe is a normal DNS query with a fresh random message id. A reply only
//! counts as a **success** when it is well-formed and its message id **and**
//! question section match the outstanding probe; by default any rcode is
//! "alive" (a recursor answering NXDOMAIN/SERVFAIL still proves it is up).
//! Operators may narrow the acceptable rcode set. Replies that do not match the
//! outstanding probe are **ignored** (neither success nor failure).

use hickory_proto::op::{Message, MessageType, OpCode, Query};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::BinDecodable;

/// What a received datagram means for the outstanding probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// Well-formed, matching, acceptable-rcode response — backend is alive.
    Success,
    /// Well-formed, matching response with an unacceptable rcode (only when the
    /// operator narrowed the acceptable set).
    Failure,
    /// Did not match the outstanding probe (wrong id/question) or could not be
    /// parsed; neither success nor failure (probe timeout decides liveness).
    Unmatched,
}

/// A compiled probe: the question to send and the rcode acceptance policy.
#[derive(Debug, Clone)]
pub struct ProbeSpec {
    name: Name,
    qtype: RecordType,
    /// `None` = accept any well-formed response; `Some` = narrowed rcode set.
    acceptable_rcodes: Option<Vec<u16>>,
}

impl ProbeSpec {
    /// Build a probe spec, parsing and lowercasing the query name.
    pub fn new(
        qname: &str,
        qtype: u16,
        acceptable_rcodes: Option<Vec<u16>>,
    ) -> Result<Self, String> {
        let mut name =
            Name::from_utf8(qname).map_err(|e| format!("invalid probe qname '{qname}': {e}"))?;
        name.set_fqdn(true);
        Ok(Self {
            name: name.to_lowercase(),
            qtype: RecordType::from(qtype),
            acceptable_rcodes,
        })
    }

    pub fn qtype(&self) -> RecordType {
        self.qtype
    }

    /// Encode a probe query for the given message id.
    pub fn build_query(&self, qid: u16) -> Result<Vec<u8>, String> {
        let mut msg = Message::new();
        msg.set_id(qid);
        msg.set_message_type(MessageType::Query);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        msg.add_query(Query::query(self.name.clone(), self.qtype));
        msg.to_vec()
            .map_err(|e| format!("probe encode failed: {e}"))
    }

    /// Classify a received datagram against this probe and message id.
    pub fn classify_response(&self, qid: u16, wire: &[u8]) -> ProbeOutcome {
        let msg = match Message::from_bytes(wire) {
            Ok(m) => m,
            // Unparseable datagram: cannot be attributed to this probe, so
            // ignore it and let the probe timeout decide liveness.
            Err(_) => return ProbeOutcome::Unmatched,
        };
        if msg.id() != qid {
            return ProbeOutcome::Unmatched;
        }
        let Some(query) = msg.queries().first() else {
            return ProbeOutcome::Unmatched;
        };
        if query.query_type() != self.qtype || query.name() != &self.name {
            return ProbeOutcome::Unmatched;
        }
        match &self.acceptable_rcodes {
            None => ProbeOutcome::Success,
            Some(allowed) => {
                let rcode = u16::from(msg.response_code());
                if allowed.contains(&rcode) {
                    ProbeOutcome::Success
                } else {
                    ProbeOutcome::Failure
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::{Message, MessageType, ResponseCode};
    use hickory_proto::rr::{Name, RecordType};

    fn spec_any() -> ProbeSpec {
        ProbeSpec::new("health.example.", 1, None).unwrap()
    }

    /// Build a synthetic response echoing a question, with the given id/rcode.
    fn response(id: u16, qname: &str, qtype: u16, rcode: ResponseCode) -> Vec<u8> {
        let mut msg = Message::new();
        msg.set_id(id);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(rcode);
        let mut name = Name::from_utf8(qname).unwrap();
        name.set_fqdn(true);
        msg.add_query(hickory_proto::op::Query::query(
            name,
            RecordType::from(qtype),
        ));
        msg.to_vec().unwrap()
    }

    #[test]
    fn build_query_uses_supplied_id_and_question() {
        let wire = spec_any().build_query(0x4321).unwrap();
        let msg = Message::from_bytes(&wire).unwrap();
        assert_eq!(msg.id(), 0x4321);
        assert_eq!(msg.queries().len(), 1);
        assert_eq!(msg.queries()[0].query_type(), RecordType::A);
    }

    #[test]
    fn any_rcode_is_success_by_default() {
        let spec = spec_any();
        let r = response(7, "health.example.", 1, ResponseCode::NXDomain);
        assert_eq!(spec.classify_response(7, &r), ProbeOutcome::Success);
        let r2 = response(7, "health.example.", 1, ResponseCode::ServFail);
        assert_eq!(spec.classify_response(7, &r2), ProbeOutcome::Success);
    }

    #[test]
    fn narrowed_rcode_rejects_servfail() {
        let spec = ProbeSpec::new("health.example.", 1, Some(vec![0])).unwrap(); // NOERROR only
        let ok = response(9, "health.example.", 1, ResponseCode::NoError);
        assert_eq!(spec.classify_response(9, &ok), ProbeOutcome::Success);
        let bad = response(9, "health.example.", 1, ResponseCode::ServFail);
        assert_eq!(spec.classify_response(9, &bad), ProbeOutcome::Failure);
    }

    #[test]
    fn mismatched_id_is_unmatched() {
        let spec = spec_any();
        let r = response(100, "health.example.", 1, ResponseCode::NoError);
        assert_eq!(spec.classify_response(101, &r), ProbeOutcome::Unmatched);
    }

    #[test]
    fn mismatched_question_is_unmatched() {
        let spec = spec_any();
        let wrong_name = response(5, "other.example.", 1, ResponseCode::NoError);
        assert_eq!(
            spec.classify_response(5, &wrong_name),
            ProbeOutcome::Unmatched
        );
        let wrong_type = response(5, "health.example.", 28, ResponseCode::NoError); // AAAA
        assert_eq!(
            spec.classify_response(5, &wrong_type),
            ProbeOutcome::Unmatched
        );
    }

    #[test]
    fn malformed_datagram_is_unmatched() {
        let spec = spec_any();
        assert_eq!(
            spec.classify_response(5, &[0u8, 1, 2]),
            ProbeOutcome::Unmatched
        );
    }

    #[test]
    fn question_match_is_case_insensitive() {
        let spec = spec_any();
        let r = response(5, "HEALTH.EXAMPLE.", 1, ResponseCode::NoError);
        assert_eq!(spec.classify_response(5, &r), ProbeOutcome::Success);
    }
}
