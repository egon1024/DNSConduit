//! Pipeline stage trait and outcomes (spec §3.1, §10).

use crate::phase::Phase;
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub enum StageOutcome {
    Continue(Phase),
    Drop,
}

pub trait PipelineStage: Send + Sync {
    fn name(&self) -> &'static str;
    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::RuntimeSnapshot;
    use crate::transaction::ClientProtocol;
    use conduit_config::load_yaml;
    use std::net::SocketAddr;

    struct SetTagStage;

    impl PipelineStage for SetTagStage {
        fn name(&self) -> &'static str {
            "set_tag"
        }

        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            txn.tags.set_bool("seen", true);
            StageOutcome::Continue(Phase::Parse)
        }
    }

    #[test]
    fn fake_stage_sets_tag() {
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:15353".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        let stage = SetTagStage;
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).expect("parse");
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));

        let outcome = stage.handle(&mut txn, &snap);
        assert!(txn.tags.has("seen"));
        assert_eq!(outcome, StageOutcome::Continue(Phase::Parse));
    }
}
