//! A comprehensive library for parsing, managing, and deduplicating academic citations.
//!
//! `biblib` provides robust functionality for working with academic citations in various formats.
//! It focuses on accurate parsing, format conversion, and intelligent deduplication of citations.
//!
//! # Features
//!
//! The library has several optional features that can be enabled in your Cargo.toml:
//!
//! - `csv` - Enable CSV format support (enabled by default)
//! - `pubmed` - Enable PubMed/MEDLINE format support (enabled by default)  
//! - `xml` - Enable EndNote XML support (enabled by default)
//! - `ris` - Enable RIS format support (enabled by default)
//! - `dedupe` - Enable citation deduplication (enabled by default)
//!
//! To use only specific features, disable default features and enable just what you need:
//!
//! ```toml
//! [dependencies]
//! biblib = { version = "0.3.0", default-features = false, features = ["csv", "ris"] }
//! ```
//!
//! # Key Characteristics
//!
//! - **Multiple Format Support**: Parse citations from:
//!   - RIS (Research Information Systems)
//!   - PubMed/MEDLINE
//!   - EndNote XML
//!   - CSV with configurable mappings
//!
//! - **Rich Metadata Support**:
//!   - Authors with affiliations
//!   - Journal details (name, abbreviation, ISSN)
//!   - DOIs and other identifiers
//!   - Complete citation metadata
//!
//! # Basic Usage
//!
//! ```rust
//! use biblib::{CitationParser, RisParser};
//!
//! // Parse RIS format
//! let input = r#"TY  - JOUR
//! TI  - Example Article
//! AU  - Smith, John
//! ER  -"#;
//!
//! let parser = RisParser::new();
//! let citations = parser.parse(input).unwrap();
//! println!("Title: {}", citations[0].title);
//! ```
//! # Citation Formats
//!
//! Each format has a dedicated parser with format-specific features:
//!
//! ```rust
//! use biblib::{RisParser, PubMedParser, EndNoteXmlParser, csv::CsvParser};
//!
//! // RIS format
//! let ris = RisParser::new();
//!
//! // PubMed format
//! let pubmed = PubMedParser::new();
//!
//! // EndNote XML format
//! let endnote = EndNoteXmlParser::new();
//!
//! // CSV format
//! let csv = CsvParser::new();
//! ```
//!
//! # Citation Deduplication
//!
//! ```rust
//! use biblib::{Citation, CitationParser, RisParser};
//!
//! let ris_input = r#"TY  - JOUR
//! TI  - Example Citation 1
//! AU  - Smith, John
//! ER  -
//!
//! TY  - JOUR
//! TI  - Example Citation 2
//! AU  - Smith, John
//! ER  -"#;
//!
//! let parser = RisParser::new();
//! let mut citations = parser.parse(ris_input).unwrap();
//!
//! // Configure deduplication
//! use biblib::dedupe::{Deduplicator, DeduplicatorConfig};
//!
//! // Configure deduplication
//! let config = DeduplicatorConfig {
//!     group_by_year: true,
//!     run_in_parallel: true,
//!     ..Default::default()
//! };
//!
//! let deduplicator = Deduplicator::new().with_config(config);
//! let duplicate_groups = deduplicator.find_duplicates(&citations).unwrap();
//!
//! for group in duplicate_groups {
//!     println!("Original: {}", group.unique.title);
//!     for duplicate in group.duplicates {
//!         println!("  Duplicate: {}", duplicate.title);
//!     }
//! }
//! ```
//!
//! # Error Handling
//!
//! The library uses a custom [`Result`] type that wraps [`CitationError`] for consistent
//! error handling across all operations:
//!
//! ```rust
//! use biblib::{CitationParser, RisParser, CitationError};
//!
//! let result = RisParser::new().parse("invalid input");
//! match result {
//!     Ok(citations) => println!("Parsed {} citations", citations.len()),
//!     Err(e) => eprintln!("Parse error: {}", e),
//! }
//! ```
//!
//! # Performance Considerations
//!
//! - Use year-based grouping for large datasets
//! - Enable parallel processing for better performance
//! - Consider using CSV format for very large datasets
//!
//! # Thread Safety
//!
//! All parser implementations are thread-safe and can be shared between threads.
//! The deduplicator supports parallel processing through the `run_in_parallel` option.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "csv")]
extern crate csv as csv_crate;

#[cfg(feature = "csv")]
pub mod csv;
#[cfg(feature = "dedupe")]
pub mod dedupe;
#[cfg(feature = "diagnostics")]
pub mod diagnostics;
#[cfg(feature = "xml")]
pub mod endnote_xml;
pub mod error;
#[cfg(feature = "pubmed")]
pub mod pubmed;
#[cfg(feature = "ris")]
pub mod ris;

// Reexports
#[cfg(feature = "csv")]
pub use csv::CsvParser;
#[cfg(feature = "xml")]
pub use endnote_xml::EndNoteXmlParser;
pub use error::{CitationError, ParseError, SourceSpan, ValueError};
#[cfg(feature = "diagnostics")]
pub use diagnostics::parse_with_diagnostics;
#[cfg(feature = "pubmed")]
pub use pubmed::PubMedParser;
#[cfg(feature = "ris")]
pub use ris::RisParser;

mod regex;
mod utils;

/// Citation format types supported by the library.
#[derive(Debug, Clone, PartialEq)]
pub enum CitationFormat {
    Ris,
    PubMed,
    EndNoteXml,
    Csv,
    Unknown,
}

impl CitationFormat {
    /// Convert the format to a string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            CitationFormat::Ris => "RIS",
            CitationFormat::PubMed => "PubMed",
            CitationFormat::EndNoteXml => "EndNote XML",
            CitationFormat::Csv => "CSV",
            CitationFormat::Unknown => "Unknown",
        }
    }
}

impl std::fmt::Display for CitationFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Represents a publication date with required year and optional month/day components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Date {
    /// Publication year (required)
    pub year: i32,
    /// Publication month (1-12)
    pub month: Option<u8>,
    /// Publication day (1-31)
    pub day: Option<u8>,
}

/// Represents an author of a citation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    /// The primary name of the person. This can be the family name or full name for mononyms.
    pub name: String,

    /// Optional given name (first name).
    pub given_name: Option<String>,

    /// Optional middle name(s), when available.
    pub middle_name: Option<String>,

    /// List of affiliation strings associated with the author.
    pub affiliations: Vec<String>,
}

/// Represents a single citation with its metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Citation {
    /// Type of the citation
    pub citation_type: Vec<String>,
    /// Title of the work
    pub title: String,
    /// List of authors
    pub authors: Vec<Author>,
    /// Journal name
    pub journal: Option<String>,
    /// Journal abbreviation
    pub journal_abbr: Option<String>,
    /// Publication date with year, month, and day
    pub date: Option<Date>,
    /// Volume number
    pub volume: Option<String>,
    /// Issue number
    pub issue: Option<String>,
    /// Page range
    pub pages: Option<String>,
    /// ISSN of the journal
    pub issn: Vec<String>,
    /// Digital Object Identifier
    pub doi: Option<String>,
    /// PubMed ID
    pub pmid: Option<String>,
    /// PMC ID
    pub pmc_id: Option<String>,
    /// Abstract text
    pub abstract_text: Option<String>,
    /// Keywords
    pub keywords: Vec<String>,
    /// URLs
    pub urls: Vec<String>,
    /// Language
    pub language: Option<String>,
    /// MeSH Terms
    pub mesh_terms: Vec<String>,
    /// Publisher
    pub publisher: Option<String>,
    /// Additional fields not covered by standard fields
    pub extra_fields: HashMap<String, Vec<String>>,
}

impl Citation {
    /// Create a new empty Citation.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Represents a group of duplicate citations with one unique citation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    /// The unique (original) citation
    pub unique: Citation,
    /// The duplicate citations
    pub duplicates: Vec<Citation>,
}

/// Trait for implementing citation parsers.
pub trait CitationParser {
    /// Parse a string containing one or more citations.
    ///
    /// # Arguments
    ///
    /// * `input` - The string containing citation data
    ///
    /// # Returns
    ///
    /// A Result containing a vector of parsed Citations or a ParseError
    ///
    /// # Errors
    ///
    /// Returns `ParseError` if the input is malformed
    fn parse(&self, input: &str) -> std::result::Result<Vec<Citation>, crate::error::ParseError>;
}

/// Format detection and automatic parsing of citation files
///
/// # Arguments
///
/// * `content` - The content of the file to parse
///
/// # Returns
///
/// A Result containing a vector of parsed Citations and the detected format,
/// or a CitationError if parsing fails
///
/// # Examples
///
/// ```
/// use biblib::detect_and_parse;
///
/// let content = r#"TY  - JOUR
/// TI  - Example Title
/// ER  -"#;
///
/// let (citations, format) = detect_and_parse(content).unwrap();
/// assert_eq!(format.as_str(), "RIS");
/// assert_eq!(citations[0].title, "Example Title");
/// ```
pub fn detect_and_parse(
    content: &str,
) -> std::result::Result<(Vec<Citation>, CitationFormat), CitationError> {
    let trimmed = content.trim();

    if trimmed.is_empty() {
        return Ok((Vec::new(), CitationFormat::Unknown));
    }

    // Try to detect format based on content patterns
    if trimmed.starts_with("<?xml") || trimmed.starts_with("<xml>") {
        // EndNote XML format
        #[cfg(feature = "xml")]
        {
            let parser = EndNoteXmlParser::new();
            let citations = parser.parse(content).map_err(CitationError::Parse)?;
            return Ok((citations, CitationFormat::EndNoteXml));
        }
        #[cfg(not(feature = "xml"))]
        return Err(CitationError::UnknownFormat);
    }

    // Check for RIS format (starts with TY or has TY  - pattern)
    if trimmed.starts_with("TY  -") || trimmed.contains("\nTY  -") {
        #[cfg(feature = "ris")]
        {
            let parser = RisParser::new();
            return parser
                .parse(content)
                .map(|citations| (citations, CitationFormat::Ris))
                .map_err(CitationError::Parse);
        }
        #[cfg(not(feature = "ris"))]
        return Err(CitationError::UnknownFormat);
    }

    // Check for PubMed format (starts with PMID- or has PMID- pattern)
    if trimmed.starts_with("PMID-") || trimmed.contains("\nPMID-") {
        #[cfg(feature = "pubmed")]
        {
            let parser = PubMedParser::new();
            return parser
                .parse(content)
                .map(|citations| (citations, CitationFormat::PubMed))
                .map_err(CitationError::Parse);
        }
        #[cfg(not(feature = "pubmed"))]
        return Err(CitationError::UnknownFormat);
    }

    Err(CitationError::UnknownFormat)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_author_equality() {
        let author1 = Author {
            name: "Smith".to_string(),
            given_name: Some("John".to_string()),
            middle_name: None,
            affiliations: Vec::new(),
        };
        let author2 = Author {
            name: "Smith".to_string(),
            given_name: Some("John".to_string()),
            middle_name: None,
            affiliations: Vec::new(),
        };
        assert_eq!(author1, author2);
    }

    #[test]
    fn test_detect_and_parse_ris() {
        let content = r#"TY  - JOUR
TI  - Test Title
AU  - Smith, John
ER  -"#;

        let (citations, format) = detect_and_parse(content).unwrap();
        assert_eq!(format, CitationFormat::Ris);
        assert_eq!(citations[0].title, "Test Title");
    }

    #[test]
    fn test_detect_and_parse_pubmed() {
        let content = r#"PMID- 12345678
TI  - Test Title
FAU - Smith, John"#;

        let (citations, format) = detect_and_parse(content).unwrap();
        assert_eq!(format, CitationFormat::PubMed);
        assert_eq!(citations[0].title, "Test Title");
    }

    #[test]
    fn test_detect_and_parse_endnote() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<xml><records><record>
<titles><title>Test Title</title></titles>
</record></records></xml>"#;

        let (citations, format) = detect_and_parse(content).unwrap();
        assert_eq!(format, CitationFormat::EndNoteXml);
        assert_eq!(citations[0].title, "Test Title");
    }

    #[test]
    fn test_detect_and_parse_empty() {
        let result = detect_and_parse("");
        assert!(
            matches!(result, Ok((citations, format)) if citations.is_empty() && format == CitationFormat::Unknown)
        );
    }

    #[test]
    fn test_detect_and_parse_unknown() {
        let content = "Some random content\nthat doesn't match\nany known format";
        let result = detect_and_parse(content);
        assert!(matches!(result, Err(CitationError::UnknownFormat)));
    }
}
