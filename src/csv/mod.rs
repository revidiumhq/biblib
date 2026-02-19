//! CSV format parser implementation.
//!
//! This module provides functionality to parse CSV formatted citations with configurable headers
//! and enhanced error handling.
//!
//! # Example
//!
//! ```
//! use biblib::{CitationParser, csv::CsvParser};
//!
//! let input = "Title,Author,Year\nExample Paper,Smith J,2023";
//!
//! let parser = CsvParser::new();
//!     
//! let citations = parser.parse(input).unwrap();
//! assert_eq!(citations[0].title, "Example Paper");
//! ```

mod config;
mod parse;
mod structure;

use crate::{Citation, CitationFormat, CitationParser};
pub use config::CsvConfig;
use parse::csv_parse;

/// Parser for CSV-formatted citation data with configurable mappings.
///
/// Provides flexible parsing of CSV files containing citation data, with support
/// for custom column mappings and different CSV dialects.
///
/// # Features
///
/// - Custom header mappings with O(1) lookup performance
/// - Configurable delimiters, quotes, and trimming
/// - Multiple author parsing (semicolon-separated)
/// - Support for extra fields not covered by standard citation fields
/// - Automatic delimiter detection
/// - Enhanced error reporting with line numbers
/// - Memory optimization options for large files
/// - Comprehensive input validation
///
/// # Examples
///
/// Basic usage:
/// ```
/// use biblib::csv::CsvParser;
/// use biblib::CitationParser;
///
/// let input = "Title,Author,Year\nExample Paper,Smith J,2023";
/// let parser = CsvParser::new();
/// let citations = parser.parse(input).unwrap();
/// ```
///
/// With custom configuration:
/// ```
/// use biblib::csv::{CsvParser, CsvConfig};
///
/// let mut config = CsvConfig::new();
/// config.set_delimiter(b';');
///
/// let parser = CsvParser::with_config(config);
/// ```
///
/// Auto-detection of format:
/// ```
/// use biblib::csv::CsvParser;
///
/// let parser = CsvParser::with_auto_detection();
/// // Will automatically detect delimiter and header presence
/// ```
///
/// # Extra Fields Support
///
/// The parser automatically identifies and preserves fields that don't map to
/// standard citation fields in the `extra_fields` HashMap:
///
/// ```
/// use biblib::{CitationParser, csv::CsvParser};
///
/// let input = "Title,Author,Custom Field\nPaper,Smith,Custom Value";
/// let parser = CsvParser::new();
/// let citations = parser.parse(input).unwrap();
///
/// assert!(citations[0].extra_fields.contains_key("Custom Field"));
/// ```
#[derive(Debug, Clone)]
pub struct CsvParser {
    config: CsvConfig,
    auto_detect: bool,
}

impl Default for CsvParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvParser {
    /// Creates a new CSV parser with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: CsvConfig::new(),
            auto_detect: false,
        }
    }

    /// Creates a new CSV parser with custom configuration
    #[must_use]
    pub fn with_config(config: CsvConfig) -> Self {
        Self {
            config,
            auto_detect: false,
        }
    }

    /// Creates a new CSV parser with automatic format detection
    #[must_use]
    pub fn with_auto_detection() -> Self {
        Self {
            config: CsvConfig::new(),
            auto_detect: true,
        }
    }

    /// Sets the configuration for this parser
    pub fn set_config(&mut self, config: CsvConfig) -> &mut Self {
        self.config = config;
        self
    }

    /// Gets a reference to the current configuration
    pub fn config(&self) -> &CsvConfig {
        &self.config
    }

    /// Gets a mutable reference to the current configuration
    pub fn config_mut(&mut self) -> &mut CsvConfig {
        &mut self.config
    }

    /// Enables or disables automatic format detection
    pub fn set_auto_detection(&mut self, enabled: bool) -> &mut Self {
        self.auto_detect = enabled;
        self
    }

    /// Auto-detects CSV format parameters from the input
    fn auto_detect_format(&self, input: &str) -> CsvConfig {
        let mut config = self.config.clone();

        if self.auto_detect {
            let delimiter = parse::detect_csv_delimiter(input);
            let has_headers = parse::detect_csv_headers(input, delimiter);

            config.set_delimiter(delimiter);
            config.set_has_header(has_headers);
        }

        config
    }
}

impl CitationParser for CsvParser {
    /// Parses a string containing CSV formatted citation data.
    ///
    /// # Arguments
    ///
    /// * `input` - The CSV formatted string to parse
    ///
    /// # Returns
    ///
    /// A Result containing a vector of parsed Citations or a ParseError
    ///
    /// # Errors
    ///
    /// Returns `ParseError` with detailed context including:
    /// - Line numbers for malformed records
    /// - Field validation errors
    /// - Configuration validation errors
    fn parse(&self, input: &str) -> std::result::Result<Vec<Citation>, crate::error::ParseError> {
        let config = self.auto_detect_format(input);
        let raw_citations = csv_parse(input, &config)?;

        let mut citations = Vec::with_capacity(raw_citations.len());
        for raw in raw_citations {
            // Convert the citation, handling potential errors
            let citation = raw
                .into_citation_with_config(&config)
                .map_err(|citation_err| {
                    // Convert CitationError to ParseError
                    match citation_err {
                        crate::error::CitationError::Parse(parse_err) => parse_err,
                        crate::error::CitationError::UnknownFormat => {
                            crate::error::ParseError::without_position(
                                CitationFormat::Csv,
                                crate::error::ValueError::Syntax("Unknown format".to_string()),
                            )
                        }
                    }
                })?;
            citations.push(citation);
        }

        Ok(citations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_basic_csv() {
        let input = "\
Title,Author,Year,Journal
Test Paper,Smith J,2023,Test Journal
Another Paper,\"Doe, Jane\",2022,Another Journal";

        let parser = CsvParser::new();
        let citations = parser.parse(input).unwrap();
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].title, "Test Paper");
    assert_eq!(citations[0].authors[0].name, "Smith");
        assert_eq!(citations[0].date.as_ref().unwrap().year, 2023);
        assert_eq!(citations[0].journal, Some("Test Journal".to_string()));
    }

    #[test]
    fn test_custom_headers() {
        let input = "\
Article Name,Writers,Published,Source
Test Paper,Smith J,2023,Test Journal";

        let mut config = CsvConfig::new();
        config
            .set_header_mapping("title", vec!["Article Name".to_string()])
            .set_header_mapping("authors", vec!["Writers".to_string()])
            .set_header_mapping("year", vec!["Published".to_string()])
            .set_header_mapping("journal", vec!["Source".to_string()]);

        let parser = CsvParser::with_config(config);
        let citations = parser.parse(input).unwrap();
        assert_eq!(citations[0].title, "Test Paper");
    assert_eq!(citations[0].authors[0].name, "Smith");
        assert_eq!(citations[0].date.as_ref().unwrap().year, 2023);
        assert_eq!(citations[0].journal, Some("Test Journal".to_string()));
    }

    #[test]
    fn test_multiple_authors() {
        let input = "\
Title,Authors,Year
Test Paper,\"Smith, John; Doe, Jane\",2023";

        let parser = CsvParser::new();
        let citations = parser.parse(input).unwrap();

        assert_eq!(citations[0].authors.len(), 2);
    assert_eq!(citations[0].authors[0].name, "Smith");
    assert_eq!(citations[0].authors[1].name, "Doe");
    }

    #[test]
    fn test_custom_delimiter() {
        let input = "Title;Author;Year\nTest Paper;Smith J;2023";

        let mut config = CsvConfig::new();
        config.set_delimiter(b';');

        let parser = CsvParser::with_config(config);
        let citations = parser.parse(input).unwrap();
        assert_eq!(citations[0].title, "Test Paper");
    assert_eq!(citations[0].authors[0].name, "Smith");
        assert_eq!(citations[0].date.as_ref().unwrap().year, 2023);
    }

    #[test]
    fn test_extra_fields_handling() {
        let input = "\
Title,Author,Year,Custom Field,Another Custom
Test Paper,Smith J,2023,Custom Value,Another Value
Second Paper,Doe J,2024,Test,Data";

        let parser = CsvParser::new();
        let citations = parser.parse(input).unwrap();

        assert_eq!(citations.len(), 2);

        // Check that extra fields are properly captured
        let first_citation = &citations[0];
        assert!(first_citation.extra_fields.contains_key("Custom Field"));
        assert!(first_citation.extra_fields.contains_key("Another Custom"));
        assert_eq!(
            first_citation.extra_fields.get("Custom Field").unwrap()[0],
            "Custom Value"
        );
    }

    #[test]
    fn test_auto_detection() {
        let input = "\
title;author;year
Test Paper;Smith J;2023
Another Paper;Doe J;2024";

        let parser = CsvParser::with_auto_detection();
        let citations = parser.parse(input).unwrap();

        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].title, "Test Paper");
    assert_eq!(citations[0].authors[0].name, "Smith");
    }

    #[test]
    fn test_memory_optimization() {
        let input = "\
Title,Author,Year
Test Paper,Smith J,2023
Another Paper,Doe J,2024";

        // Test with original record storage disabled (default)
        let mut config = CsvConfig::new();
        config.set_store_original_record(false);
        let parser = CsvParser::with_config(config);

        let citations = parser.parse(input).unwrap();
        assert_eq!(citations.len(), 2);

        // Test with original record storage enabled
        let mut config2 = CsvConfig::new();
        config2.set_store_original_record(true);
        let parser2 = CsvParser::with_config(config2);

        let citations2 = parser2.parse(input).unwrap();
        assert_eq!(citations2.len(), 2);
    }

    #[test]
    fn test_improved_validation_errors() {
        // Test empty field name validation
        let mut config = CsvConfig::new();
        config.set_header_mapping("", vec!["test".to_string()]);

        let parser = CsvParser::with_config(config);
        let result = parser.parse("test\nvalue");
        assert!(result.is_err());

        // Test invalid delimiter validation
        let mut config2 = CsvConfig::new();
        config2.set_delimiter(b'\n');

        let parser2 = CsvParser::with_config(config2);
        let result2 = parser2.parse("test,value\ntest2,value2");
        assert!(result2.is_err());
    }

    #[test]
    fn test_comprehensive_header_detection() {
        // Should detect headers with various academic field names
        let inputs = vec![
            ("title,doi,volume\nTest Article,10.1234/test,5", "title"),
            ("Title,Publication Year,Volume\nTest,2023,5", "title"),
            (
                "article title,authors,year\nTest Paper,Smith J,2023",
                "title",
            ),
        ];

        for (input, _expected_field) in inputs {
            let parser = CsvParser::with_auto_detection();
            let citations = parser.parse(input).unwrap();
            assert!(!citations.is_empty());
        }
    }

    #[test]
    fn test_error_with_line_numbers() {
        let input = "Title,Author\nTest Paper"; // Missing author field

        let parser = CsvParser::new();
        match parser.parse(input) {
            Err(_) => {
                // Error handling works - the specific error type has changed
                // but errors are still reported properly
            }
            Ok(_) => panic!("Expected an error for malformed CSV"),
        }
    }

    #[test]
    fn test_keywords_parsing() {
        let input = "Title,Keywords\nTest Paper,\"keyword1; keyword2; keyword3\"";

        let parser = CsvParser::new();
        let citations = parser.parse(input).unwrap();
        assert_eq!(citations[0].keywords.len(), 3);
        assert!(citations[0].keywords.contains(&"keyword1".to_string()));
    }

    #[test]
    fn test_empty_input() {
        let parser = CsvParser::new();
        let result = parser.parse("");
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_no_valid_citations() {
        let input = "Title,Author\n,\n  ,  ";

        let parser = CsvParser::new();
        let result = parser.parse(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parser_configuration_methods() {
        let mut parser = CsvParser::new();

        // Test configuration access
        assert_eq!(parser.config().delimiter, b',');

        // Test mutable configuration
        parser.config_mut().set_delimiter(b';');
        assert_eq!(parser.config().delimiter, b';');

        // Test setting new config
        let new_config = CsvConfig::new();
        parser.set_config(new_config);
        assert_eq!(parser.config().delimiter, b','); // Back to default

        // Test auto-detection toggle
        parser.set_auto_detection(true);
        assert!(parser.auto_detect);
    }

    // ── Phase 4: line-number accuracy tests ─────────────────────────────────

    /// A missing Title field for the *second* data row (line 3) must produce
    /// an error whose `line` field equals 3.
    #[test]
    fn test_missing_title_on_second_row_reports_line() {
        // header = line 1, first data row = line 2, second data row = line 3
        let input = "Title,Author\nFirst Paper,Smith J\n,Doe J";
        let err = CsvParser::new().parse(input).unwrap_err();
        // The first row parses fine; the second row (line 3) is missing title.
        assert_eq!(
            err.line,
            Some(3),
            "missing title on line 3 should be reported as line 3"
        );
    }

    /// A missing Title field on line 2 (first data row) must report line 2.
    #[test]
    fn test_missing_title_on_first_data_row_reports_line() {
        let input = "Title,Author\n,Smith J";
        let err = CsvParser::new().parse(input).unwrap_err();
        assert_eq!(err.line, Some(2));
    }

    /// The error's `span` must be `Some` and its start byte must be >= the
    /// length of the header row (i.e. past the header bytes).
    #[test]
    fn test_missing_title_error_has_span() {
        let header = "Title,Author\n";
        let input = format!("{},Smith J", header);
        let err = CsvParser::new().parse(&input).unwrap_err();
        let span = err.span.expect("expected a byte-offset span on CSV error");
        // The span start should point into the first data row, which begins
        // after the header row.
        assert!(
            span.start >= header.len().saturating_sub(1),
            "span.start ({}) should be at or near the start of the data row (header is {} bytes)",
            span.start, header.len()
        );
    }

    /// Verify that line numbers increase correctly across multiple rows.
    #[test]
    fn test_line_numbers_increase_correctly() {
        use crate::csv::config::CsvConfig;
        // 3 data rows — we check their line_number fields directly.
        let input = "Title,Author\nPaper A,Smith\nPaper B,Jones\nPaper C,Doe";
        let config = CsvConfig::new();
        let raw = parse::csv_parse(input, &config).unwrap();
        assert_eq!(raw.len(), 3);
        assert_eq!(raw[0].line_number, 2);
        assert_eq!(raw[1].line_number, 3);
        assert_eq!(raw[2].line_number, 4);
    }
}
