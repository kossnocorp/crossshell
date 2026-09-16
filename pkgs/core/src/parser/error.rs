use crate::prelude::internal::*;

/// Parse error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("Parsing failed with {count} error(s)", count = .errors.len())]
pub struct CshParserError<'source_code> {
    pub errors: Vec<CshError<'source_code>>,
}
