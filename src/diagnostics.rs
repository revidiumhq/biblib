//! Pretty diagnostic reporting using [ariadne].
//!
//! This module provides rich, human-readable error output for [`ParseError`]
//! values, rendered with source-code context, underlines, and labels.  It
//! is only compiled when the `diagnostics` Cargo feature is enabled:
//!
//! ```toml
//! [dependencies]
//! biblib = { version = "0.3", features = ["diagnostics"] }
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use biblib::{CitationParser, RisParser};
//!
//! let source = "TY  - JOUR\nAU  - Smith, John\nER  -";
//! match RisParser::new().parse(source) {
//!     Ok(citations) => println!("Parsed {} citations", citations.len()),
//!     Err(e) => eprintln!("{}", e.to_diagnostic("input.ris", source)),
//! }
//! ```

use crate::error::ParseError;

#[cfg(feature = "diagnostics")]
use ariadne::{Color, Label, Report, ReportKind, Source};

#[cfg(feature = "diagnostics")]
impl ParseError {
    /// Render this error as a pretty Ariadne diagnostic.
    ///
    /// The returned `String` contains ANSI colour codes when the terminal
    /// supports them.  Redirect to a file or pipe through `strip-ansi` if
    /// you need plain text.
    ///
    /// # Arguments
    ///
    /// * `filename` – Label shown in the report header (e.g. `"citations.ris"`).
    /// * `source`   – The original source text that was parsed.
    pub fn to_diagnostic(&self, filename: &str, source: &str) -> String {
        let mut buf = Vec::new();

        // Ariadne 0.6+: Report::build takes a Span directly.
        // We use (filename, range) as our span type, where range is the
        // portion of the source that triggered the error.
        let primary_range = self.primary_byte_range(source);
        let header_span = (filename, primary_range.clone());

        let mut report = Report::build(ReportKind::Error, header_span)
            .with_message(format!("{}", self));

        // Attach a label pointing at the exact span / line.
        report = report.with_label(
            Label::new((filename, primary_range))
                .with_message(format!("{}", self.error))
                .with_color(Color::Red),
        );

        report
            .finish()
            .write((filename, Source::from(source)), &mut buf)
            .unwrap();

        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Compute a byte-range into `source` that best represents the error
    /// location, used for Ariadne label placement.
    ///
    /// Priority: explicit `span` > line-derived range > whole-file fallback.
    #[cfg(feature = "diagnostics")]
    fn primary_byte_range(&self, source: &str) -> std::ops::Range<usize> {
        if let Some(ref span) = self.span {
            return span.start..span.end;
        }
        if let Some(line) = self.line {
            let line_start: usize = source
                .lines()
                .take(line.saturating_sub(1))
                .map(|l| l.len() + 1) // +1 for '\n'
                .sum();
            let line_len = source
                .lines()
                .nth(line.saturating_sub(1))
                .map(|l| l.len())
                .unwrap_or(0);
            return line_start..line_start + line_len;
        }
        // No position info — point at offset 0 (shows the first line).
        0..0
    }
}

/// Parse a citation string and, on failure, return a pretty Ariadne diagnostic
/// instead of a raw [`ParseError`].
///
/// This is a convenience wrapper around calling `.parse()` and then
/// `.to_diagnostic()` on the resulting error.
///
/// # Arguments
///
/// * `parser`   – Any type implementing [`crate::CitationParser`].
/// * `input`    – The source text to parse.
/// * `filename` – A display label for the source (e.g. a file path).
///
/// # Returns
///
/// `Ok(citations)` on success, or `Err(diagnostic_string)` on failure.
#[cfg(feature = "diagnostics")]
pub fn parse_with_diagnostics(
    parser: &dyn crate::CitationParser,
    input: &str,
    filename: &str,
) -> Result<Vec<crate::Citation>, String> {
    parser
        .parse(input)
        .map_err(|e| e.to_diagnostic(filename, input))
}

#[cfg(all(test, feature = "diagnostics"))]
mod tests {
    use crate::{
        error::{ParseError, SourceSpan, ValueError},
        CitationFormat,
    };

    #[test]
    fn test_to_diagnostic_with_span() {
        let source = "TY  - JOUR\nTI  - Hello\nER  -\n";
        let err = ParseError::at_line(1, CitationFormat::Ris, ValueError::Syntax("oops".into()))
            .with_span(SourceSpan::new(0, 10));

        let diag = err.to_diagnostic("test.ris", source);
        assert!(diag.contains("test.ris"));
    }

    #[test]
    fn test_to_diagnostic_line_only() {
        let source = "TY  - JOUR\nTI  - Hello\nER  -\n";
        let err = ParseError::at_line(
            2,
            CitationFormat::Ris,
            ValueError::MissingValue {
                field: "title",
                key: "TI",
            },
        );

        let diag = err.to_diagnostic("test.ris", source);
        assert!(diag.contains("test.ris"));
    }

    #[test]
    fn test_to_diagnostic_no_position() {
        let source = "some content\n";
        let err = ParseError::without_position(
            CitationFormat::Ris,
            ValueError::Syntax("bad input".into()),
        );

        // Should not panic even without position info
        let diag = err.to_diagnostic("test.ris", source);
        assert!(diag.contains("test.ris"));
    }
}
