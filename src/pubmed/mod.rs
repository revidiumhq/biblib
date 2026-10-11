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
        assert!(result[0].authors[0].affiliations.contains(
            &"Department of Science, Test University New York, NY 10021, USA".to_string()
        ));
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
        assert_eq!(
            result[0].doi.as_deref(),
            Some("10.1016/j.example.2023.01.001")
        );
    }

    // ── Phase 4: line-number accuracy tests ─────────────────────────────────

    /// Missing TI should not reject an otherwise valid PubMed record.
    /// — the first tag — appears).
    #[test]
    fn test_missing_title_is_allowed() {
        let input = "PMID- 12345678\nAU  - Smith, John\n\n";
        let citations = PubMedParser::new().parse(input).unwrap();
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].title, "");
        assert_eq!(citations[0].authors.len(), 1);
    }

    /// The same permissive behavior should hold for later citations too.
    #[test]
    fn test_missing_title_is_allowed_in_second_citation() {
        // Citation 1: lines 1-2, blank on 3, Citation 2: starts on line 4.
        let input = "PMID- 1\nTI  - First\n\nPMID- 2\nAU  - Doe, J\n\n";
        let citations = PubMedParser::new().parse(input).unwrap();
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].title, "First");
        assert_eq!(citations[1].title, "");
        assert_eq!(citations[1].authors.len(), 1);
    }

    /// A bad date value must also carry the right line number.
    #[test]
    fn test_bad_date_reports_line() {
        let input = "PMID- 1\nTI  - Title\nDP  - not-a-date\n\n";
        let err = PubMedParser::new().parse(input).unwrap_err();
        assert_eq!(
            err.line,
            Some(1),
            "error should point back to the citation start line"
        );
        assert!(matches!(
            err.error,
            crate::error::ValueError::BadValue { .. }
        ));
    }

    /// Multiple-citation files should also tolerate an untitled later citation.
    #[test]
    fn test_missing_title_is_allowed_in_third_citation() {
        let input = concat!(
            "PMID- 1\nTI  - One\n\n",    // chunk 1: lines 1-2
            "PMID- 2\nTI  - Two\n\n",    // chunk 2: lines 4-5
            "PMID- 3\nAU  - Doe, J\n\n", // chunk 3: starts line 7 (missing TI)
        );
        let citations = PubMedParser::new().parse(input).unwrap();
        assert_eq!(citations.len(), 3);
        assert_eq!(citations[2].title, "");
    }

    #[test]
    fn test_bom_prefixed_first_record_is_kept() {
        let citations = PubMedParser::new()
            .parse("\u{feff}PMID- 1\nTI  - First\n\nPMID- 2\nTI  - Second\n")
            .unwrap();
        let pmids = citations.iter().map(|c| c.pmid.as_deref()).collect_vec();
        assert_eq!(pmids, &[Some("1"), Some("2")]);
        assert_eq!(citations[0].title, "First");
    }

    #[test]
    fn test_bom_does_not_shift_error_span() {
        let body = "PMID- 1\nDP  - not-a-date\n";
        let plain = PubMedParser::new().parse(body).unwrap_err();
        let with_bom = PubMedParser::new()
            .parse(&format!("\u{feff}{body}"))
            .unwrap_err();
        assert_eq!(with_bom.line, plain.line);
        let (plain, with_bom) = (plain.span.unwrap(), with_bom.span.unwrap());
        assert_eq!(
            (with_bom.start, with_bom.end),
            (plain.start + 3, plain.end + 3)
        );
    }

    #[test]
    fn test_abbreviated_page_range_is_expanded() {
        let citations = PubMedParser::new()
            .parse("PMID- 1\nPG  - 940-5\n\nPMID- 2\nPG  - e082765\n")
            .unwrap();
        assert_eq!(citations[0].pages.as_deref(), Some("940-945"));
        assert_eq!(citations[1].pages.as_deref(), Some("e082765"));
    }

    #[test]
    fn test_free_text_date_falls_back_to_its_year() {
        let citations = PubMedParser::new()
            .parse("PMID- 1\nDP  - 1999-2000\n\nPMID- 2\nDP  - Summer 2001\n")
            .unwrap();
        let years = citations
            .iter()
            .map(|c| c.date.as_ref().map(|d| d.year))
            .collect_vec();
        assert_eq!(years, &[Some(1999), Some(2001)]);
    }

    #[test]
    fn test_maps_fields_like_other_formats() {
        // Real tag lines from PMID 32678530 and a GeneReviews chapter.
        let input = "PMID- 32678530
TI  - Dexamethasone in Hospitalized Patients with Covid-19.
FAU - Horby, Peter
AU  - Horby P
CN  - RECOVERY Collaborative Group
FAU - Lim, Wei Shen
AU  - Lim WS
OTO - NOTNLM
OT  - COVID-19
OT  - dexamethasone

PMID- 31536184
TI  - Glutaric Acidemia Type 1.
BTI - GeneReviews((R))
FED - Adam, Margaret P
ED  - Adam MP
FAU - Larson, Austin
AU  - Larson A
ISBN- 9780000000001
AID - NBK546575 [bookaccession]

PMID- 20301295
BTI - GeneReviews((R))
FED - Adam, Margaret P
ED  - Adam MP

PMID- 1215597
TI  - [Middle lobe syndrome].
TT  - Sindrome del lobulo medio.
";
        let citations = PubMedParser::new().parse(input).unwrap();

        let article = &citations[0];
        let names = article
            .authors
            .iter()
            .map(|a| a.name.as_str())
            .collect_vec();
        assert_eq!(names, &["Horby", "RECOVERY Collaborative Group", "Lim"]);
        assert_eq!(article.authors[1].given_name, None);
        assert!(!article.extra_fields.contains_key("CN"));
        assert_eq!(article.keywords, &["COVID-19", "dexamethasone"]);
        assert_eq!(article.extra_fields["OTO"], &["NOTNLM"]);

        // Editors are not authors when there are authors.
        let chapter = &citations[1];
        let names = chapter
            .authors
            .iter()
            .map(|a| a.name.as_str())
            .collect_vec();
        assert_eq!(names, &["Larson"]);
        assert_eq!(chapter.extra_fields["FED"], &["Adam, Margaret P"]);
        assert_eq!(chapter.issn, &["9780000000001"]);
        assert_eq!(chapter.accession_number.as_deref(), Some("NBK546575"));

        // A book without authors: its editors stand in.
        let book = &citations[2];
        assert_eq!(book.title, "GeneReviews((R))");
        assert_eq!(book.authors[0].name, "Adam");
        assert_eq!(book.authors[0].given_name.as_deref(), Some("Margaret"));
        assert!(!book.extra_fields.contains_key("FED"));
        assert!(!book.extra_fields.contains_key("ED"));

        assert_eq!(citations[3].title, "[Middle lobe syndrome].");
        let untitled = PubMedParser::new()
            .parse("PMID- 1\nTT  - Sindrome del lobulo medio.\n")
            .unwrap();
        assert_eq!(untitled[0].title, "Sindrome del lobulo medio.");
    }

    #[test]
    fn test_blank_or_tagless_chunks_are_not_citations() {
        for input in [
            "\nPMID- 1\nTI  - T\n",
            "\n\n\nPMID- 1\nTI  - T\n",
            "junk line\n\nPMID- 1\nTI  - T\n",
        ] {
            let citations = PubMedParser::new().parse(input).unwrap();
            assert_eq!(citations.len(), 1, "{input:?}");
            assert_eq!(citations[0].pmid.as_deref(), Some("1"));
        }
    }
}
