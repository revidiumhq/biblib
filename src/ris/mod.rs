//! RIS format parser implementation.
//!
//! Provides functionality to parse RIS formatted citations with improved structure
//! and error handling.
//!
//! # Example
//!
//! ```
//! use biblib::{CitationParser, RisParser};
//!
//! let input = r#"TY  - JOUR
//! TI  - Example Title
//! AU  - Smith, John
//! ER  -"#;
//!
//! let parser = RisParser::new();
//!     
//! let citations = parser.parse(input).unwrap();
//! assert_eq!(citations[0].title, "Example Title");
//! ```

mod parse;
mod structure;
mod tags;

use crate::{Citation, CitationParser};
use parse::ris_parse;

/// Parser for RIS format citations.
///
/// RIS is a standardized format for bibliographic citations that uses two-letter
/// tags at the start of each line to denote different citation fields.
#[derive(Debug, Clone, Default)]
pub struct RisParser;

impl RisParser {
    /// Creates a new RIS parser instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use biblib::RisParser;
    /// let parser = RisParser::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CitationParser for RisParser {
    /// Parses a string containing one or more citations in RIS format.
    ///
    /// # Arguments
    ///
    /// * `input` - The RIS formatted string to parse
    ///
    /// # Returns
    ///
    /// A Result containing a vector of parsed Citations or a CitationError
    ///
    /// # Errors
    ///
    /// Returns `ParseError` if the input is malformed or contains no valid citations
    fn parse(&self, input: &str) -> std::result::Result<Vec<Citation>, crate::error::ParseError> {
        let raw_citations = ris_parse(input)?;

        let mut citations = Vec::with_capacity(raw_citations.len());
        for raw in raw_citations {
            let citation = raw.try_into()?;
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
    fn test_parse_simple_ris() {
        let input = r#"TY  - JOUR
TI  - Test Article Title
AU  - Smith, John
JO  - Test Journal
PY  - 2023/12/25/Christmas edition
VL  - 10
IS  - 2
SP  - 100
EP  - 110
DO  - 10.1000/test
AB  - This is a test abstract.
KW  - Keyword1
KW  - Keyword2
ER  -

"#;
        let parser = RisParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let citation = &result[0];
        assert_eq!(citation.citation_type[0], "Journal Article");
        assert_eq!(citation.title, "Test Article Title");
        assert_eq!(citation.authors.len(), 1);
        assert_eq!(citation.authors[0].name, "Smith");
        let date = citation.date.as_ref().unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, Some(12));
        assert_eq!(date.day, Some(25));
        assert_eq!(citation.pages, Some("100-110".to_string()));
        assert_eq!(citation.keywords.len(), 2);
    }

    #[test]
    fn test_parse_gs_format() {
        let input = r#"TY  - JOUR
T1  - Albendazole therapy in children with focal seizures and single small enhancing computerized tomographic lesions: a randomized, placebo-controlled, double blind trial
A1  - Baranwal, Arun K
A1  - Singhi, Pratibha D
A1  - Khandelwal, N
A1  - Singhi, Sunit C
JO  - The Pediatric infectious disease journal
VL  - 17
IS  - 8
SP  - 696
EP  - 700
SN  - 0891-3668
Y1  - 1998///
PB  - LWW
ER  - 


TY  - JOUR
T1  - High-dose praziquantel with cimetidine for refractory neurocysticercosis: a case report with clinical and MRI follow-up.
A1  - Yee, Thomas
A1  - Barakos, Jerome A
A1  - Knight, Robert T
JO  - Western journal of medicine
VL  - 170
IS  - 2
SP  - 112
Y1  - 1999
PB  - BMJ Publishing Group
ER  - 

"#;
        let parser = RisParser::new();
        let citations = parser.parse(&input).unwrap();
        assert_eq!(
            citations.len(),
            2,
            "Expected 2 citations in Google Scholar format"
        );
        assert_eq!(citations[0].date.as_ref().unwrap().year, 1998);
        assert_eq!(citations[1].date.as_ref().unwrap().year, 1999);
    }

    #[test]
    fn test_parse_url_with_doi_extraction() {
        let input = r#"TY  - JOUR
TI  - Test Article
UR  - https://doi.org/10.1000/test
L1  - https://example.com/pdf
ER  -"#;

        let parser = RisParser::new();
        let result = parser.parse(input).unwrap();

        assert_eq!(result[0].urls.len(), 2);
        assert!(
            result[0]
                .urls
                .contains(&"https://doi.org/10.1000/test".to_string())
        );
        assert!(
            result[0]
                .urls
                .contains(&"https://example.com/pdf".to_string())
        );
        assert_eq!(result[0].doi, Some("10.1000/test".to_string()));
    }

    // ── Phase 4: line-number accuracy tests ─────────────────────────────────

    /// Missing TI in the very first citation (TY on line 1) must report line 1.
    #[test]
    fn test_missing_title_reports_first_citation_line() {
        let input = "TY  - JOUR\nAU  - Smith, John\nER  -\n";
        let err = RisParser::new().parse(input).unwrap_err();
        assert_eq!(err.line, Some(1), "expected line 1 (TY tag line)");
    }

    /// Missing TI in the *second* citation — TY starts on line 6 — must
    /// report that line, not line 1.
    #[test]
    fn test_missing_title_reports_second_citation_line() {
        let input = concat!(
            "TY  - JOUR\n",   // line 1
            "TI  - First\n",  // line 2
            "ER  -\n",        // line 3
            "\n",             // line 4
            "TY  - JOUR\n",   // line 5
            "AU  - Doe, J\n", // line 6
            "ER  -\n",        // line 7
        );
        let err = RisParser::new().parse(input).unwrap_err();
        assert_eq!(err.line, Some(5), "expected line 5 (second TY tag)");
    }

    /// The byte-offset span on a missing-title error must start at the byte
    /// offset of the TY line (i.e. byte 0 for the first citation).
    #[test]
    fn test_missing_title_error_has_span() {
        let input = "TY  - JOUR\nAU  - Smith, John\nER  -\n";
        let err = RisParser::new().parse(input).unwrap_err();
        let span = err.span.expect("expected a byte-offset span");
        assert_eq!(
            span.start, 0,
            "span should start at byte 0 (TY on first line)"
        );
        assert!(span.end > span.start, "span end must be after start");
    }

    /// `record_span` for the second citation must begin *after* the first
    /// citation ends — i.e. its start byte must be > 0.
    #[test]
    fn test_missing_title_second_citation_span_nonzero() {
        let first = "TY  - JOUR\nTI  - First\nER  -\n\n";
        let second = "TY  - JOUR\nAU  - Doe, J\nER  -\n";
        let input = format!("{}{}", first, second);

        let err = RisParser::new().parse(&input).unwrap_err();
        let span = err.span.expect("expected a byte-offset span");
        assert!(
            span.start >= first.len(),
            "second citation span ({}) should start at or after byte {} (end of first citation)",
            span.start,
            first.len()
        );
    }

    /// Line numbers for syntax errors (bad tag characters) must be accurate.
    #[test]
    fn test_syntax_error_line_accuracy() {
        // !! on line 3 is invalid; the line is captured in ignored_lines.
        // We verify through the raw parser (crate-internal path).
        use super::parse::ris_parse;
        let input = "TY  - JOUR\nTI  - Title\n!!  - bad\nER  -\n";
        let raw = ris_parse(input).unwrap();
        assert_eq!(raw[0].ignored_lines.len(), 1);
        assert_eq!(
            raw[0].ignored_lines[0].0, 3,
            "bad line should be tagged as line 3"
        );
    }
}
