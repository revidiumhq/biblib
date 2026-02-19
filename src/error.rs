//! Error types for citation parsing operations.
//!
//! This module defines a structured error hierarchy that provides detailed
//! information about parsing failures, including line/column positions and
//! format-specific context.

use crate::CitationFormat;
use thiserror::Error;

/// A byte-offset span into the original source text.
///
/// Both `start` and `end` are byte offsets (not character indices) from the
/// beginning of the source string.  `start` is inclusive, `end` is exclusive.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceSpan {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

impl SourceSpan {
    /// Create a new `SourceSpan`.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// Field name constants for consistent error reporting.
pub mod fields {
    pub const TITLE: &str = "title";
    pub const AUTHOR: &str = "author";
    pub const DATE: &str = "date";
    pub const JOURNAL: &str = "journal";
    pub const JOURNAL_ABBR: &str = "journal_abbr";
    pub const DOI: &str = "doi";
    pub const VOLUME: &str = "volume";
    pub const ISSUE: &str = "issue";
    pub const PAGES: &str = "pages";
    pub const ABSTRACT: &str = "abstract";
    pub const KEYWORDS: &str = "keywords";
    pub const YEAR: &str = "year";
    pub const PMID: &str = "pmid";
    pub const PMC_ID: &str = "pmc_id";
    pub const ISSN: &str = "issn";
    pub const LANGUAGE: &str = "language";
    pub const PUBLISHER: &str = "publisher";
    pub const URLS: &str = "urls";
    pub const MESH_TERMS: &str = "mesh_terms";
    pub const CITATION_TYPE: &str = "citation_type";
}

/// Top-level error type for citation operations.
#[derive(Error, Debug)]
pub enum CitationError {
    #[error("Unable to detect citation format from input")]
    UnknownFormat,

    #[error(transparent)]
    Parse(#[from] ParseError),
}

/// Parse error with detailed location and context information.
#[derive(Error, Debug)]
#[error("Error in {format} format{}: {error}", 
    match (line, column) {
        (Some(l), Some(c)) => format!(" at line {} column {}", l, c),
        (Some(l), None) => format!(" at line {}", l),
        (None, Some(c)) => format!(" at column {}", c),
        (None, None) => String::new(),
    }
)]
pub struct ParseError {
    /// Line number where the error occurred (1-based, None if not available)
    pub line: Option<usize>,
    /// Column number where the error occurred (1-based, None if not available)
    pub column: Option<usize>,
    /// Byte-offset span into the source text, for rich diagnostic rendering.
    pub span: Option<SourceSpan>,
    /// The citation format being parsed
    pub format: CitationFormat,
    /// The specific error that occurred
    pub error: ValueError,
}

impl ParseError {
    /// Create a new ParseError.
    pub fn new(
        line: Option<usize>,
        column: Option<usize>,
        format: CitationFormat,
        error: ValueError,
    ) -> Self {
        Self {
            line,
            column,
            span: None,
            format,
            error,
        }
    }

    /// Attach a byte-offset span to this error, returning `self` (builder style).
    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    /// Create a ParseError with just line information.
    pub fn at_line(line: usize, format: CitationFormat, error: ValueError) -> Self {
        Self::new(Some(line), None, format, error)
    }

    /// Create a ParseError with line and column information.
    pub fn at_position(
        line: usize,
        column: usize,
        format: CitationFormat,
        error: ValueError,
    ) -> Self {
        Self::new(Some(line), Some(column), format, error)
    }

    /// Create a ParseError without position information.
    pub fn without_position(format: CitationFormat, error: ValueError) -> Self {
        Self::new(None, None, format, error)
    }
}

/// Specific value-level errors that can occur during parsing.
#[derive(Error, Debug)]
pub enum ValueError {
    #[error("Bad syntax: {0}")]
    Syntax(String),

    #[error("Missing value for {key}")]
    MissingValue {
        field: &'static str,
        key: &'static str,
    },

    #[error("Bad value for {key}: \"{value}\" ({reason})")]
    BadValue {
        field: &'static str,
        key: &'static str,
        value: String,
        reason: String,
    },

    #[error("Second value found for {key} but only one value is allowed")]
    MultipleValues {
        field: &'static str,
        key: &'static str,
        second_row: Option<usize>,
        second_col: Option<usize>,
    },
}

// Conversion implementations for external error types

#[cfg(feature = "csv")]
impl From<csv::Error> for ParseError {
    fn from(err: csv::Error) -> Self {
        let (line, column) = if let Some(position) = err.position() {
            (
                Some(position.line() as usize),
                Some(position.byte() as usize),
            )
        } else {
            (None, None)
        };

        ParseError::new(
            line,
            column,
            CitationFormat::Csv,
            ValueError::Syntax(err.to_string()),
        )
    }
}

#[cfg(feature = "xml")]
impl From<quick_xml::Error> for ParseError {
    fn from(err: quick_xml::Error) -> Self {
        ParseError::without_position(
            CitationFormat::EndNoteXml,
            ValueError::Syntax(err.to_string()),
        )
    }
}

#[cfg(feature = "xml")]
impl From<quick_xml::events::attributes::AttrError> for ParseError {
    fn from(err: quick_xml::events::attributes::AttrError) -> Self {
        ParseError::without_position(
            CitationFormat::EndNoteXml,
            ValueError::Syntax(err.to_string()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_display() {
        let error = ParseError::at_line(
            42,
            CitationFormat::Ris,
            ValueError::Syntax("Invalid tag format".to_string()),
        );

        let display = format!("{}", error);
        assert!(display.contains("line 42"));
        assert!(display.contains("RIS format"));
        assert!(display.contains("Invalid tag format"));
    }

    #[test]
    fn test_parse_error_with_position() {
        let error = ParseError::at_position(
            10,
            25,
            CitationFormat::Csv,
            ValueError::MissingValue {
                field: fields::TITLE,
                key: "Title",
            },
        );

        let display = format!("{}", error);
        assert!(display.contains("line 10 column 25"));
        assert!(display.contains("CSV format"));
    }

    #[test]
    fn test_parse_error_without_position() {
        let error = ParseError::without_position(
            CitationFormat::EndNoteXml,
            ValueError::BadValue {
                field: fields::YEAR,
                key: "year",
                value: "invalid".to_string(),
                reason: "not a valid year".to_string(),
            },
        );

        let display = format!("{}", error);
        assert!(display.contains("EndNote XML format"));
        assert!(!display.contains("line"));
        assert!(!display.contains("column"));
    }

    #[test]
    fn test_value_error_display() {
        let error = ValueError::MissingValue {
            field: fields::TITLE,
            key: "TI",
        };
        assert_eq!(format!("{}", error), "Missing value for TI");

        let error = ValueError::BadValue {
            field: fields::YEAR,
            key: "PY",
            value: "not-a-year".to_string(),
            reason: "invalid year format".to_string(),
        };
        assert_eq!(
            format!("{}", error),
            "Bad value for PY: \"not-a-year\" (invalid year format)"
        );
    }

    #[test]
    fn test_citation_format_display() {
        assert_eq!(format!("{}", CitationFormat::Ris), "RIS");
        assert_eq!(format!("{}", CitationFormat::PubMed), "PubMed");
        assert_eq!(format!("{}", CitationFormat::EndNoteXml), "EndNote XML");
        assert_eq!(format!("{}", CitationFormat::Csv), "CSV");
    }

    #[cfg(feature = "csv")]
    #[test]
    fn test_csv_error_conversion() {
        // Create a mock CSV error - this is a simplified test
        let csv_content = "invalid,csv\nwith,extra,field";
        let mut reader = csv::Reader::from_reader(csv_content.as_bytes());
        let result = reader.records().next();

        if let Some(Err(csv_err)) = result {
            let parse_err: ParseError = csv_err.into();
            assert_eq!(parse_err.format, CitationFormat::Csv);
            assert!(matches!(parse_err.error, ValueError::Syntax(_)));
        }
    }
}
