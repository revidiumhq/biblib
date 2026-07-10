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
        let citations = parser.parse(input).unwrap();
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

    #[test]
    fn test_parse_accession_number_from_an() {
        let input = r#"TY  - JOUR
TI  - Test Article
AN  - ACC-123
ER  -"#;

        let citation = RisParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.accession_number.as_deref(), Some("ACC-123"));
    }

    #[test]
    fn test_id_remains_in_extra_fields_when_an_present() {
        let input = r#"TY  - JOUR
TI  - Test Article
AN  - ACC-123
ID  - REF-456
ER  -"#;

        let citation = RisParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.accession_number.as_deref(), Some("ACC-123"));
        assert_eq!(citation.pmid, None);
        assert_eq!(
            citation.extra_fields.get("ID"),
            Some(&vec!["REF-456".to_string()])
        );
    }

    #[test]
    fn test_missing_title_is_allowed_in_first_citation() {
        let input = "TY  - JOUR\nAU  - Smith, John\nER  -\n";
        let citations = RisParser::new().parse(input).unwrap();
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].title, "");
        assert_eq!(citations[0].authors.len(), 1);
    }

    #[test]
    fn test_missing_title_is_allowed_in_second_citation() {
        let input = concat!(
            "TY  - JOUR\n",   // line 1
            "TI  - First\n",  // line 2
            "ER  -\n",        // line 3
            "\n",             // line 4
            "TY  - JOUR\n",   // line 5
            "AU  - Doe, J\n", // line 6
            "ER  -\n",        // line 7
        );
        let citations = RisParser::new().parse(input).unwrap();
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].title, "First");
        assert_eq!(citations[1].title, "");
        assert_eq!(citations[1].authors[0].name, "Doe");
    }

    #[test]
    fn test_m3_included_in_citation_type() {
        let input = "TY  - JOUR\nTI  - Test\nM3  - Randomized Controlled Trial\nER  -\n";
        let citations = RisParser::new().parse(input).unwrap();
        assert!(
            citations[0]
                .citation_type
                .contains(&"Randomized Controlled Trial".to_string()),
            "M3 value should appear in citation_type; got {:?}",
            citations[0].citation_type
        );
        assert!(
            citations[0]
                .citation_type
                .contains(&"Journal Article".to_string()),
            "TY value should still be present"
        );
    }

    #[test]
    fn test_ab_absent_falls_back_to_n2() {
        let input = "TY  - JOUR\nTI  - Test\nN2  - Abstract from N2 field.\nER  -\n";
        let citations = RisParser::new().parse(input).unwrap();
        assert_eq!(
            citations[0].abstract_text.as_deref(),
            Some("Abstract from N2 field."),
            "should fall back to N2 when AB is absent"
        );
    }

    #[test]
    fn test_ab_takes_priority_over_n2() {
        let input =
            "TY  - JOUR\nTI  - Test\nAB  - Primary abstract.\nN2  - Fallback abstract.\nER  -\n";
        let citations = RisParser::new().parse(input).unwrap();
        assert_eq!(
            citations[0].abstract_text.as_deref(),
            Some("Primary abstract."),
            "AB should take priority over N2"
        );
    }

    #[test]
    fn test_multiple_ab_tags_are_joined() {
        let input = concat!(
            "TY  - JOUR\n",
            "TI  - Test\n",
            "AB  - First paragraph.\n",
            "AB  - Second paragraph.\n",
            "AB  - Third paragraph.\n",
            "ER  -\n",
        );
        let citations = RisParser::new().parse(input).unwrap();
        assert_eq!(
            citations[0].abstract_text.as_deref(),
            Some("First paragraph.\n\nSecond paragraph.\n\nThird paragraph."),
            "repeated AB tags should be merged into one abstract"
        );
    }

    #[test]
    fn test_multiple_n2_tags_are_joined_when_ab_is_absent() {
        let input = concat!(
            "TY  - JOUR\n",
            "TI  - Test\n",
            "N2  - First fallback paragraph.\n",
            "N2  - Second fallback paragraph.\n",
            "ER  -\n",
        );
        let citations = RisParser::new().parse(input).unwrap();
        assert_eq!(
            citations[0].abstract_text.as_deref(),
            Some("First fallback paragraph.\n\nSecond fallback paragraph."),
            "repeated N2 tags should be merged when no AB is present"
        );
    }

    #[test]
    fn test_n2_multiline_no_indent() {
        let input = concat!(
            "TY  - JOUR\n",
            "TI  - Test\n",
            "N2  - Brief Summary\n",
            "At present, there are no relevant studies.\n",
            "ER  -\n",
        );
        let citations = RisParser::new().parse(input).unwrap();
        assert_eq!(
            citations[0].abstract_text.as_deref(),
            Some("Brief Summary At present, there are no relevant studies."),
            "continuation line should be joined into N2"
        );
    }

    #[test]
    fn test_syntax_error_line_accuracy() {
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
