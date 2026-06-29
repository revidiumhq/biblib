//! EndNote Tagged (`.enw`) parser implementation.
//!
//! EndNote Tagged, also called EndNote Web format, is a line-oriented tagged
//! export where each line begins with a percent-prefixed one-character tag.
//!
//! # Example
//!
//! ```
//! use biblib::{CitationParser, EnwParser};
//!
//! let input = r#"%0 Journal Article
//! %T Example Title
//! %A Smith, John
//! %D 2024
//! %R 10.1000/example
//! "#;
//!
//! let citations = EnwParser::new().parse(input).unwrap();
//! assert_eq!(citations.len(), 1);
//! assert_eq!(citations[0].title, "Example Title");
//! assert_eq!(citations[0].doi.as_deref(), Some("10.1000/example"));
//! ```

mod parse;

use crate::error::ParseError;
use crate::{Citation, CitationParser};
pub(crate) use parse::looks_like_enw;
use parse::parse_enw;

/// Parser for EndNote Tagged (`.enw`) citations.
#[derive(Debug, Clone, Default)]
pub struct EnwParser;

impl EnwParser {
    /// Creates a new EndNote Tagged parser instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CitationParser for EnwParser {
    fn parse(&self, input: &str) -> Result<Vec<Citation>, ParseError> {
        if input.trim().is_empty() {
            return Ok(Vec::new());
        }

        parse_enw(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ValueError;

    #[test]
    fn test_parse_sample_enw_record() {
        let input = r#"%0 Edited Book
%@ 978-3-8309-1689-5
%E Handke, Jürgen
%E Franke, Peter
%T The virtual linguistics campus
%D 2006
%C Münster
%I Waxmann
%J Strategies and concepts for successful e-learning
%P 324 S.
%K E-Learning
"#;

        let citations = EnwParser::new().parse(input).unwrap();
        assert_eq!(citations.len(), 1);

        let citation = &citations[0];
        assert_eq!(citation.citation_type, vec!["Edited Book"]);
        assert_eq!(citation.title, "The virtual linguistics campus");
        assert_eq!(
            citation.journal.as_deref(),
            Some("Strategies and concepts for successful e-learning")
        );
        assert_eq!(citation.publisher.as_deref(), Some("Waxmann"));
        assert_eq!(citation.pages.as_deref(), Some("324 S."));
        assert_eq!(citation.keywords, vec!["E-Learning"]);
        assert_eq!(citation.date.as_ref().map(|d| d.year), Some(2006));
        assert_eq!(citation.issn, vec!["978-3-8309-1689-5"]);
        assert_eq!(citation.authors.len(), 2);
        assert_eq!(
            citation
                .extra_fields
                .get("%E")
                .cloned()
                .unwrap_or_default()
                .len(),
            2
        );
        assert_eq!(
            citation.extra_fields.get("%C"),
            Some(&vec!["Münster".to_string()])
        );
    }

    #[test]
    fn test_parse_multiple_records() {
        let input = r#"%0 Journal Article
%T First
%A Smith, John

%0 Report
%T Second
%A Doe, Jane
"#;

        let citations = EnwParser::new().parse(input).unwrap();
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].title, "First");
        assert_eq!(citations[1].title, "Second");
        assert_eq!(citations[0].citation_type, vec!["Journal Article"]);
        assert_eq!(citations[1].citation_type, vec!["Report"]);
    }

    #[test]
    fn test_percent0_and_percent9_are_preserved_raw() {
        let input = r#"%0 Journal Article
%9 Randomized Controlled Trial
%9 Randomized Controlled Trial
%T Example
"#;

        let citations = EnwParser::new().parse(input).unwrap();
        assert_eq!(
            citations[0].citation_type,
            vec!["Journal Article", "Randomized Controlled Trial"]
        );
    }

    #[test]
    fn test_contributor_roles_are_flattened_and_preserved() {
        let input = r#"%0 Book
%T Example
%A Smith, John
%E Doe, Jane
%Y Brown, Alex
%? Helper, Sam
%H Translator, Terry
"#;

        let citations = EnwParser::new().parse(input).unwrap();
        let citation = &citations[0];
        assert_eq!(citation.authors.len(), 5);
        assert!(citation.extra_fields.get("%A").is_none());
        assert_eq!(
            citation.extra_fields.get("%E"),
            Some(&vec!["Doe, Jane".to_string()])
        );
        assert_eq!(
            citation.extra_fields.get("%Y"),
            Some(&vec!["Brown, Alex".to_string()])
        );
        assert_eq!(
            citation.extra_fields.get("%?"),
            Some(&vec!["Helper, Sam".to_string()])
        );
        assert_eq!(
            citation.extra_fields.get("%H"),
            Some(&vec!["Translator, Terry".to_string()])
        );
    }

    #[test]
    fn test_container_priority_prefers_j_over_b_over_s() {
        let input = r#"%0 Journal Article
%T Example
%S Tertiary Title
%B Conference Name
%J Journal Name
"#;

        let citation = EnwParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.journal.as_deref(), Some("Journal Name"));
        assert_eq!(
            citation.extra_fields.get("%B"),
            Some(&vec!["Conference Name".to_string()])
        );
        assert_eq!(
            citation.extra_fields.get("%S"),
            Some(&vec!["Tertiary Title".to_string()])
        );
        assert!(citation.extra_fields.get("%J").is_none());
    }

    #[test]
    fn test_percent8_date_is_preferred_over_percent_d() {
        let input = r#"%0 Journal Article
%T Example
%D 2006
%8 2007-05-02
"#;

        let citation = EnwParser::new().parse(input).unwrap().remove(0);
        let date = citation.date.expect("date should be parsed");
        assert_eq!(date.year, 2007);
        assert_eq!(date.month, Some(5));
        assert_eq!(date.day, Some(2));
        assert_eq!(
            citation.extra_fields.get("%D"),
            Some(&vec!["2006".to_string()])
        );
    }

    #[test]
    fn test_percent_d_year_only_fallback() {
        let input = r#"%0 Journal Article
%T Example
%D 2006
%8 not-a-date
"#;

        let citation = EnwParser::new().parse(input).unwrap().remove(0);
        let date = citation.date.expect("date should be parsed");
        assert_eq!(date.year, 2006);
        assert_eq!(date.month, None);
        assert_eq!(date.day, None);
        assert_eq!(
            citation.extra_fields.get("%8"),
            Some(&vec!["not-a-date".to_string()])
        );
    }

    #[test]
    fn test_doi_extraction_from_percent_r_and_url_fallback() {
        let input = r#"%0 Journal Article
%T Example
%R 10.1000/example
%R PMID-12345
%U https://doi.org/10.1000/url-fallback
%> https://example.com/full.pdf
"#;

        let citation = EnwParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.doi.as_deref(), Some("10.1000/example"));
        assert_eq!(
            citation.urls,
            vec![
                "https://doi.org/10.1000/url-fallback".to_string(),
                "https://example.com/full.pdf".to_string()
            ]
        );
        assert_eq!(
            citation.extra_fields.get("%R"),
            Some(&vec!["PMID-12345".to_string()])
        );
    }

    #[test]
    fn test_percent_at_preserves_raw_identifier_when_not_issn_like() {
        let input = r#"%0 Book
%T Example
%@ 978-3-8309-1689-5
"#;

        let citation = EnwParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.issn, vec!["978-3-8309-1689-5"]);
    }

    #[test]
    fn test_continuation_lines_append_to_previous_value() {
        let input = r#"%0 Journal Article
%T Example
%X First line
Second line continues here.
"#;

        let citation = EnwParser::new().parse(input).unwrap().remove(0);
        assert_eq!(
            citation.abstract_text.as_deref(),
            Some("First line\nSecond line continues here.")
        );
    }

    #[test]
    fn test_author_only_record_is_valid() {
        let input = r#"%0 Personal Communication
%A Smith, John
"#;

        let citation = EnwParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.title, "");
        assert_eq!(citation.authors.len(), 1);
    }

    #[test]
    fn test_missing_content_reports_line_and_span() {
        let input = "%0 Generic\n%K keyword\n";
        let err = EnwParser::new().parse(input).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert_eq!(err.format, crate::CitationFormat::Enw);
        assert!(matches!(
            err.error,
            ValueError::MissingValue {
                field: "title or author",
                key: "title/author"
            }
        ));
        let span = err.span.expect("expected span");
        assert_eq!(span.start, 0);
        assert!(span.end > span.start);
    }

    #[test]
    fn test_malformed_tag_reports_line_and_span() {
        let input = "%0 Journal Article\n%AB bad\n%T Example\n";
        let err = EnwParser::new().parse(input).unwrap_err();

        assert_eq!(err.line, Some(2));
        assert_eq!(err.format, crate::CitationFormat::Enw);
        assert!(matches!(err.error, ValueError::Syntax(_)));
        let span = err.span.expect("expected span");
        assert!(span.end > span.start);
    }
}
