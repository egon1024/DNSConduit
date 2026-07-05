//! Named steps in the per-query orchestrator graph (spec §3.1).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Receive,
    Parse,
    RequestRules,
    Lookup,
    ResponseRules,
    Send,
    /// Internal to the forward lookup provider (route → forward → wait); not a top-level graph phase.
    Route,
    /// Internal to the forward lookup provider; not a top-level graph phase.
    Forward,
    /// Internal resume point after async upstream I/O inside Lookup; not a top-level graph phase.
    WaitResponse,
}

/// Reserved attachment points for future processor chains and rate limits (pluggable-lookup-system).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelineAttachment {
    PostParse,
    PostRequestRules,
    PreForward,
    PostLookupPreForward,
    PostWait,
    PreSend,
}
