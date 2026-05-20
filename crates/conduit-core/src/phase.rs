//! Named steps in the per-query orchestrator graph (spec §3.1).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Receive,
    Parse,
    RequestRules,
    Route,
    Forward,
    WaitResponse,
    ResponseRules,
    Send,
}
