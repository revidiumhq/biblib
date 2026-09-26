//! Re-exports from the internal `regex_lite` backend.

#[cfg(feature = "dedupe")]
pub(crate) use regex_lite::Captures;
#[cfg(any(
    feature = "csv",
    feature = "xml",
    feature = "enw",
    feature = "bib",
    feature = "dedupe"
))]
pub(crate) use regex_lite::Regex;
