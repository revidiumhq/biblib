#![allow(clippy::result_large_err)]
#![allow(clippy::type_complexity)]
//! A comprehensive library for parsing, managing, and deduplicating academic citations.
//!
//! `biblib` parses citation exports from multiple sources into one normalized
//! [`Citation`] model, then optionally deduplicates the result set.
//!
//! It is designed for ingestion pipelines, review tooling, registry imports,
//! and any workflow that needs to reconcile heterogeneous citation files.
//!
//! # What You Get
//!
//! - Dedicated parsers for RIS, PubMed / MEDLINE, EndNote XML, EndNote Tagged
//!   (`.enw`), BibTeX / BibLaTeX (`.bib`), generic CSV, and ICTRP CSV exports
//! - A shared [`Citation`] output type with normalized identifiers such as DOI,
//!   PMID, PMCID, and `accession_number`
//! - Preservation of source-specific leftovers through `extra_fields`
//! - Optional duplicate detection via [`dedupe::Deduplicator`]
//! - Optional human-friendly parse diagnostics with the `diagnostics` feature
//!
//! # Quick Start
//!
//! ```rust
//! use biblib::{CitationParser, RisParser};
//!
//! let input = r#"TY  - JOUR
//! TI  - Example Article
//! AU  - Smith, John
//! DO  - 10.1000/example
//! ER  -"#;
//!
//! let citations = RisParser::new().parse(input).unwrap();
//!
//! assert_eq!(citations.len(), 1);
//! assert_eq!(citations[0].title, "Example Article");
//! assert_eq!(citations[0].doi.as_deref(), Some("10.1000/example"));
//! ```
//!
//! # Supported Parsers
//!
//! ```rust
//! use biblib::{
//!     BibParser, CitationParser, EndNoteXmlParser, EnwParser, IctrpCsvParser, PubMedParser,
//!     RisParser,
//! };
//! use biblib::csv::CsvParser;
//!
//! let _ris = RisParser::new();
//! let _pubmed = PubMedParser::new();
//! let _endnote = EndNoteXmlParser::new();
//! let _enw = EnwParser::new();
//! let _bib = BibParser::new();
//! let _csv = CsvParser::new();
//! let _ictrp = IctrpCsvParser::new();
//! ```
//!
//! # Auto-Detection
//!
//! [`detect_and_parse`] currently auto-detects RIS, PubMed, EndNote XML,
//! EndNote Tagged, BibTeX / BibLaTeX, and ICTRP CSV. Generic CSV remains
//! explicit because header mapping is application-specific.
//!
//! ```rust
//! use biblib::detect_and_parse;
//!
//! let input = "TY  - JOUR\nTI  - Example\nER  -";
//! let (citations, format) = detect_and_parse(input).unwrap();
//!
//! assert_eq!(format.as_str(), "RIS");
//! assert_eq!(citations[0].title, "Example");
//! ```
//!
//! # Feature Flags
//!
//! Disable default features when you only need a subset of parsers:
//!
//! ```toml
//! [dependencies]
//! biblib = { version = "0.6", default-features = false, features = ["ris", "csv"] }
//! ```
//!
//! Available public features:
//!
//! - `ris`
//! - `pubmed`
//! - `xml`
//! - `csv`
//! - `enw`
//! - `bib`
//! - `dedupe`
//! - `diagnostics`
//!
//! Since `v0.5`, `biblib` no longer uses the `regex` crate or exposes regex
//! backend feature flags. It uses `regex-lite` internally, and regex backend
//! choice is not part of the public feature surface.
//!
//! # Deduplication
//!
//! ```rust
//! use biblib::dedupe::{Deduplicator, DeduplicatorConfig};
//! use biblib::{Citation, Date};
//!
//! let citations = vec![
//!     Citation {
//!         title: "Example Title".to_string(),
//!         doi: Some("10.1000/example".to_string()),
//!         date: Some(Date { year: 2023, month: None, day: None }),
//!         journal: Some("Example Journal".to_string()),
//!         ..Default::default()
//!     },
//!     Citation {
//!         title: "Example Title".to_string(),
//!         doi: Some("10.1000/example".to_string()),
//!         date: Some(Date { year: 2023, month: None, day: None }),
//!         journal: Some("Example Journal".to_string()),
//!         ..Default::default()
//!     },
//! ];
//!
//! let config = DeduplicatorConfig {
//!     group_by_year: true,
//!     run_in_parallel: true,
//!     source_preferences: vec!["PubMed".to_string()],
//! };
//!
//! let groups = Deduplicator::new()
//!     .with_config(config)
//!     .find_duplicates(&citations)
//!     .unwrap();
//!
//! let duplicate_group = groups
//!     .iter()
//!     .find(|group| group.unique.doi.as_deref() == Some("10.1000/example"))
//!     .unwrap();
//!
//! assert_eq!(duplicate_group.duplicates.len(), 1);
//! ```
//!
//! # Errors and Diagnostics
//!
//! Parsers return [`ParseError`] with line numbers and, when available, source
//! spans.
//!
//! ```rust
//! use biblib::{CitationParser, RisParser, ValueError};
//!
//! let input = "TY  - JOUR\nAU  - Smith, John\nER  -\n";
//! let err = RisParser::new().parse(input).unwrap_err();
//!
//! assert_eq!(err.line, Some(1));
//! assert!(matches!(err.error, ValueError::MissingValue { key: "TI", .. }));
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "csv")]
extern crate csv as csv_crate;

#[cfg(feature = "bib")]
pub mod bib;
#[cfg(feature = "csv")]
pub mod csv;
#[cfg(feature = "dedupe")]
pub mod dedupe;
#[cfg(feature = "diagnostics")]
pub mod diagnostics;
#[cfg(feature = "enw")]
pub mod enw;
#[cfg(feature = "xml")]
pub mod endnote_xml;
pub mod error;
#[cfg(feature = "pubmed")]
pub mod pubmed;
#[cfg(feature = "ris")]
pub mod ris;

// Reexports
#[cfg(feature = "csv")]
pub use csv::{CsvParser, IctrpCsvParser};
#[cfg(feature = "bib")]
pub use bib::BibParser;
#[cfg(feature = "diagnostics")]
pub use diagnostics::parse_with_diagnostics;
#[cfg(feature = "enw")]
pub use enw::EnwParser;
#[cfg(feature = "xml")]
pub use endnote_xml::EndNoteXmlParser;
pub use error::{CitationError, ParseError, SourceSpan, ValueError};
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
    Enw,
    Bib,
    Csv,
    IctrpCsv,
    Unknown,
}

impl CitationFormat {
    /// Convert the format to a string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            CitationFormat::Ris => "RIS",
            CitationFormat::PubMed => "PubMed",
            CitationFormat::EndNoteXml => "EndNote XML",
            CitationFormat::Enw => "EndNote Tagged",
            CitationFormat::Bib => "BibTeX / BibLaTeX",
            CitationFormat::Csv => "CSV",
            CitationFormat::IctrpCsv => "ICTRP CSV",
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
    /// Accession number or registry identifier
    pub accession_number: Option<String>,
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
/// A Result containing a vector of parsed Citations and the detected format, or
/// a CitationError if parsing fails
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

    // Check for EndNote Tagged / ENW format (records start with %0)
    #[cfg(feature = "enw")]
    if enw::looks_like_enw(content) {
        let parser = EnwParser::new();
        return parser
            .parse(content)
            .map(|citations| (citations, CitationFormat::Enw))
            .map_err(CitationError::Parse);
    }

    #[cfg(feature = "bib")]
    if bib::looks_like_bib(content) {
        let parser = BibParser::new();
        return parser
            .parse(content)
            .map(|citations| (citations, CitationFormat::Bib))
            .map_err(CitationError::Parse);
    }

    #[cfg(feature = "csv")]
    if csv::looks_like_ictrp_csv(content) {
        let parser = IctrpCsvParser::new();
        return parser
            .parse(content)
            .map(|citations| (citations, CitationFormat::IctrpCsv))
            .map_err(CitationError::Parse);
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

    #[cfg(feature = "enw")]
    #[test]
    fn test_detect_and_parse_enw() {
        let content = "%0 Journal Article\n%T Test Title\n%A Smith, John\n";

        let (citations, format) = detect_and_parse(content).unwrap();
        assert_eq!(format, CitationFormat::Enw);
        assert_eq!(citations[0].title, "Test Title");
        assert_eq!(citations[0].citation_type, vec!["Journal Article"]);
    }

    #[cfg(feature = "bib")]
    #[test]
    fn test_detect_and_parse_bib() {
        let content = r#"@article{smith2024,
  title = {Test Title},
  author = {Smith, John}
}"#;

        let (citations, format) = detect_and_parse(content).unwrap();
        assert_eq!(format, CitationFormat::Bib);
        assert_eq!(citations[0].title, "Test Title");
        assert_eq!(citations[0].citation_type, vec!["article"]);
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

    #[cfg(feature = "csv")]
    #[test]
    fn test_detect_and_parse_ictrp_csv() {
        let content = concat!(
            "TrialID,Public title,Scientific title,Date registration,Date registration3,",
            "Source Register\n",
            "NCT00000001,Public,Scientific,01/05/2026,20260501,ClinicalTrials.gov\n"
        );

        let (citations, format) = detect_and_parse(content).unwrap();
        assert_eq!(format, CitationFormat::IctrpCsv);
        assert_eq!(
            citations[0].accession_number.as_deref(),
            Some("NCT00000001")
        );
    }
}
