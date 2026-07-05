//! Terminal outcomes from a lookup provider attempt.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupOutcome {
    Answered,
    Miss,
    Pending,
    Bypass,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerSource {
    Cache,
    Forward,
}

impl AnswerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Forward => "forward",
        }
    }
}
