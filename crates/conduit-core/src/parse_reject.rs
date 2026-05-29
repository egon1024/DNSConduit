//! Parse-stage rejection reasons for built-in metrics.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseRejectReason {
    Empty,
    WireError,
    NotQuery,
    NoQuestion,
    MultiQuestion,
}

impl ParseRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::WireError => "wire_error",
            Self::NotQuery => "not_query",
            Self::NoQuestion => "no_question",
            Self::MultiQuestion => "multi_question",
        }
    }
}
