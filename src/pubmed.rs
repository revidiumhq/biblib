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
}
