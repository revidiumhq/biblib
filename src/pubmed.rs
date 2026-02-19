//! PubMed format parser implementation.
//!
//! Provides functionality to parse PubMed formatted citations.
//!
//! # Example
//!
//! ```
//! use biblib::{CitationParser, PubMedParser};
//!
//! let input = r#"PMID- 12345678
//! TI  - Example Title
//! FAU - Smith, John
//!
//! "#;
//!
//! let parser = PubMedParser::new();
//!     
//! let citations = parser.parse(input).unwrap();
//! assert_eq!(citations[0].title, "Example Title");
//! ```

mod author;
mod parse;
mod split;
mod structure;
mod tags;
mod whole_lines;

use crate::error::ParseError;
use crate::pubmed::parse::pubmed_parse;
use crate::{Citation, CitationParser};
use itertools::Itertools;

/// Parser for PubMed format citations.
///
/// PubMed format is commonly used by PubMed and the National Library of Medicine
/// for bibliographic citations.
#[derive(Debug, Clone, Default)]
pub struct PubMedParser {}

impl PubMedParser {
    /// Creates a new PubMed parser instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use biblib::PubMedParser;
    /// let parser = PubMedParser::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl CitationParser for PubMedParser {
    /// Parses a string containing one or more citations in PubMed format.
    ///
    /// # Arguments
    ///
    /// * `input` - The PubMed formatted string to parse
    ///
    /// # Returns
    ///
    /// A Result containing a vector of parsed Citations or a ParseError
    ///
    /// # Errors
    ///
    /// Returns `ParseError` if the input is malformed
    fn parse(&self, input: &str) -> Result<Vec<Citation>, ParseError> {
        // Handle empty input by returning empty vector
        if input.trim().is_empty() {
            return Ok(Vec::new());
        }

        pubmed_parse(input)
            .into_iter()
            .map(|x| x.try_into())
            .try_collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_simple_citation() {
        let input = r#"PMID- 12345678
TI- Test Article Title
FAU- Smith, John
JT- Test Journal
DP- 2023 Jan 23
VI- 10
IP- 2
PG- 100-110
LID- 10.1000/test [doi]
AB- This is a test abstract.
MH- Keyword1
MH- Keyword2

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let citation = &result[0];
        assert_eq!(citation.pmid.as_deref(), Some("12345678"));
        assert_eq!(citation.title, "Test Article Title");
        assert_eq!(citation.authors.len(), 1);
    assert_eq!(citation.authors[0].name, "Smith");
        let date = citation.date.as_ref().unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, Some(1));
        assert_eq!(date.day, Some(23));
    }

    #[test]
    fn test_parse_three_citations() {
        let input = r#"PMID- 123
TI- One

PMID- 456
TI- Two

PMID- 789
TI- Three
"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        let titles = result.iter().map(|c| c.title.as_str()).collect_vec();
        assert_eq!(titles, &["One", "Two", "Three"]);
        let pmids = result.iter().map(|c| c.pmid.as_deref()).collect_vec();
        assert_eq!(pmids, &[Some("123"), Some("456"), Some("789")])
    }

    #[test]
    fn test_parse_citation_with_affiliation() {
        let input = r#"PMID- 12345678
TI  - Test Article Title
FAU - Smith, John
AD  - Department of Science, Test University
      New York, NY 10021, USA
JT  - Test Journal

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert!(result[0].authors[0]
            .affiliations
            .contains(&"Department of Science, Test University New York, NY 10021, USA".to_string()));
    }

    #[test]
    fn test_journal_names() {
        let input = r#"PMID- 12345678
TI  - Test Article
JT  - Journal of Testing
TA  - J Test

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();

        assert_eq!(result[0].journal.as_deref(), Some("Journal of Testing"));
        assert_eq!(result[0].journal_abbr.as_deref(), Some("J Test"));
    }

    #[test]
    fn test_journal_fallback() {
        let input = r#"PMID- 12345678
TI  - Test Article
TA  - J Test

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result[0].journal.as_deref(), None);
        assert_eq!(result[0].journal_abbr.as_deref(), Some("J Test"));
    }

    // Add test for ISSN parsing
    #[test]
    fn test_parse_citation_with_issn() {
        let input = r#"PMID- 12345678
TI  - Test Article Title
IS  - 1234-5678
IS  - 8765-4321

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result[0].issn, vec!["1234-5678", "8765-4321"]);
    }

    #[test]
    fn test_parse_citation_with_au_tag() {
        let input = r#"PMID- 12345678
TI  - Test Article Title
AU  - Smith J
AU  - Jones B

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result[0].authors.len(), 2);
    assert_eq!(result[0].authors[0].name, "Smith");
    assert_eq!(result[0].authors[0].given_name.as_deref(), Some("J"));
    assert_eq!(result[0].authors[1].name, "Jones");
    assert_eq!(result[0].authors[1].given_name.as_deref(), Some("B"));
    }

    #[test]
    fn test_fau_precedence_over_au() {
        let input = r#"PMID- 12345678
TI  - Test Article Title
FAU - Li, Yun
AU  - Li Y
FAU - Zhang, Huajun
AU  - Zhang H

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result[0].authors.len(), 2);
    assert_eq!(result[0].authors[0].name, "Li");
    assert_eq!(result[0].authors[0].given_name.as_deref(), Some("Yun"));
    assert_eq!(result[0].authors[1].name, "Zhang");
    assert_eq!(result[0].authors[1].given_name.as_deref(), Some("Huajun"));
    }

    #[test]
    fn test_crlf_endings() {
        let input = "PMID- 123\r\nTI- Windows\r\nFAU- Gates, Bill\r\nFAU- Cutler, Dave";
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result[0].pmid.as_deref(), Some("123"));
        assert_eq!(result[0].title, "Windows");
    assert_eq!(result[0].authors[0].given_name.as_deref(), Some("Bill"));
    assert_eq!(result[0].authors[0].name, "Gates");
    assert_eq!(result[0].authors[1].given_name.as_deref(), Some("Dave"));
    assert_eq!(result[0].authors[1].name, "Cutler");
    }

    #[test]
    fn test_continued_line() {
        let input = r#"PMID- 31181385
DP  - 2019 Dec
TI  - Fantastic yeasts and where to find them: the hidden diversity of dimorphic fungal 
      pathogens.
AB  - This is a long abstract that spans
      multiple lines for testing purposes.
FAU - Van Dyke, Marley C Caballero
AU  - Van Dyke MCC
"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let citation = &result[0];
        assert_eq!(citation.pmid.as_deref(), Some("31181385"));
        assert_eq!(
            citation.title,
            "Fantastic yeasts and where to find them: the hidden diversity of dimorphic fungal pathogens."
        );
        assert_eq!(
            result[0].abstract_text.as_deref(),
            Some("This is a long abstract that spans multiple lines for testing purposes.")
        );
        assert_eq!(citation.authors.len(), 1);
    }

    #[test]
    fn test_empty_input() {
        let parser = PubMedParser::new();
        let result = parser.parse("").unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_whitespace_only_input() {
        let parser = PubMedParser::new();
        let result = parser.parse("   \n  \t  ").unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_doi_from_aid_field() {
        // Test that DOI can be extracted from AID field when LID doesn't have DOI
        let input = r#"PMID- 12345678
TI- Test Article Title
AID- 10.1234/aid.test [doi]

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result[0].doi.as_deref(), Some("10.1234/aid.test"));
    }

    #[test]
    fn test_doi_lid_takes_precedence_over_aid() {
        // Test that LID DOI is preferred over AID DOI
        let input = r#"PMID- 12345678
TI- Test Article Title
LID- 10.1000/lid.doi [doi]
AID- 10.1234/aid.doi [doi]

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result[0].doi.as_deref(), Some("10.1000/lid.doi"));
    }

    #[test]
    fn test_doi_from_aid_with_pii_in_lid() {
        // Test that DOI is extracted from AID when LID contains only PII
        let input = r#"PMID- 12345678
TI- Test Article Title
LID- S1234-5678(23)00001-X [pii]
AID- 10.1016/j.example.2023.01.001 [doi]

"#;
        let parser = PubMedParser::new();
        let result = parser.parse(input).unwrap();
        assert_eq!(result[0].doi.as_deref(), Some("10.1016/j.example.2023.01.001"));
    }

    // ── Phase 4: line-number accuracy tests ─────────────────────────────────

    /// A missing TI in a single-citation file must report line 1 (where PMID
    /// — the first tag — appears).
    #[test]
    fn test_missing_title_reports_line() {
        let input = "PMID- 12345678\nAU  - Smith, John\n\n";
        let err = PubMedParser::new().parse(input).unwrap_err();
        assert_eq!(err.line, Some(1), "error should point to line 1 (citation start)");
    }

    /// Second citation starts on line 4 (after blank-line separator).
    /// A missing TI there must report that line.
    #[test]
    fn test_missing_title_reports_second_citation_line() {
        // Citation 1: lines 1-2, blank on 3, Citation 2: starts on line 4.
        let input = "PMID- 1\nTI  - First\n\nPMID- 2\nAU  - Doe, J\n\n";
        let err = PubMedParser::new().parse(input).unwrap_err();
        assert_eq!(err.line, Some(4), "second citation starts on line 4");
    }

    /// The byte-offset span must cover the whole citation chunk, so its start
    /// byte for the first citation is 0.
    #[test]
    fn test_missing_title_error_has_span() {
        let input = "PMID- 12345678\nAU  - Smith, John\n\n";
        let err = PubMedParser::new().parse(input).unwrap_err();
        let span = err.span.expect("expected a byte-offset span");
        assert_eq!(span.start, 0, "first citation span should start at byte 0");
        assert!(span.end > span.start);
    }

    /// The span start for the second citation must be after the first citation's
    /// bytes (i.e. > 0).
    #[test]
    fn test_missing_title_second_citation_span_nonzero() {
        let first = "PMID- 1\nTI  - First\n\n";
        let second = "PMID- 2\nAU  - Doe, J\n\n";
        let input = format!("{}{}", first, second);
        let err = PubMedParser::new().parse(&input).unwrap_err();
        let span = err.span.expect("expected a byte-offset span");
        assert!(
            span.start >= first.len(),
            "second citation span ({}) should start at or after byte {} (end of first)",
            span.start, first.len()
        );
    }

    /// A bad date value must also carry the right line number.
    #[test]
    fn test_bad_date_reports_line() {
        let input = "PMID- 1\nTI  - Title\nDP  - not-a-date\n\n";
        let err = PubMedParser::new().parse(input).unwrap_err();
        assert_eq!(err.line, Some(1), "error should point back to the citation start line");
        assert!(matches!(err.error, crate::error::ValueError::BadValue { .. }));
    }

    /// Multiple-citation file: only the third citation is broken; the first two
    /// must parse OK and the error's line must point into the third chunk.
    #[test]
    fn test_line_number_in_third_citation() {
        let input = concat!(
            "PMID- 1\nTI  - One\n\n",    // chunk 1: lines 1-2
            "PMID- 2\nTI  - Two\n\n",    // chunk 2: lines 4-5
            "PMID- 3\nAU  - Doe, J\n\n", // chunk 3: starts line 7 (missing TI)
        );
        let err = PubMedParser::new().parse(input).unwrap_err();
        assert_eq!(err.line, Some(7), "third citation starts on line 7");
    }
}
