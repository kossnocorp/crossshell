use crate::prelude::internal::*;

mod report;
pub use report::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshError<'source_code> {
    Unexpected(Rich<'source_code, char>),

    UnclosedQuote {
        quote: char,
        opening_span: Range<usize>,
        end_span: Range<usize>,
    },

    UnsupportedEscape {
        span: Range<usize>,
    },
}

impl<'source_code> ChumskyError<'source_code, &'source_code str> for CshError<'source_code> {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unexpected(a), Self::Unexpected(b)) => {
                Self::Unexpected(<Rich<'source_code, char> as ChumskyError<
                    'source_code,
                    &'source_code str,
                >>::merge(a, b))
            }

            // A targeted diagnosis takes precedence over generic expectations
            // from alternative branches at the same input position.
            (Self::Unexpected(_), diagnostic) => diagnostic,
            (diagnostic, _) => diagnostic,
        }
    }
}

impl<'source_code, L> LabelError<'source_code, &'source_code str, L> for CshError<'source_code>
where
    Rich<'source_code, char>: LabelError<'source_code, &'source_code str, L>,
{
    fn expected_found<E: IntoIterator<Item = L>>(
        expected: E,
        found: Option<MaybeRef<'source_code, char>>,
        span: SimpleSpan,
    ) -> Self {
        Self::Unexpected(Rich::expected_found(expected, found, span))
    }

    fn label_with(&mut self, label: L) {
        if let Self::Unexpected(error) = self {
            error.label_with(label);
        }
    }

    fn in_context(&mut self, label: L, span: SimpleSpan) {
        if let Self::Unexpected(error) = self {
            error.in_context(label, span);
        }
    }
}
