//! Forward execution mode (sync blocking vs split_io submit).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardMode {
    /// Block on upstream recv in the Forward stage (default sync runtime).
    Sync,
    /// Submit upstream and suspend at WaitResponse (split_io / tokio).
    Submit,
}
