use crate::prelude::internal::*;

mod report;
pub use report::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshError<'source_code> {
    Unexpected {
        /// The unexpected UTF-8 character, or an empty slice at end of input.
        found: &'source_code str,
        span: Range<usize>,
        expected: &'static str,
    },

    UnclosedQuote {
        quote: char,
        opening_span: Range<usize>,
        end_span: Range<usize>,
    },

    IncompleteEscape {
        span: Range<usize>,
    },
}
