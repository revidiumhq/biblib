//! BibTeX / BibLaTeX (`.bib`) parser implementation.
//!
//! This parser is designed for tolerant ingestion into `biblib`'s normalized
//! [`crate::Citation`] model while preserving unmapped fields in
//! `Citation::extra_fields`.
//!
//! # Example
//!
//! ```
//! use biblib::{BibParser, CitationParser};
//!
//! let input = r#"@article{smith2024,
//!   title = {Example Article},
//!   author = {Smith, John and Doe, Jane},
//!   date = {2024-05-02},
//!   doi = {10.1000/example}
//! }"#;
//!
//! let citations = BibParser::new().parse(input).unwrap();
//! assert_eq!(citations.len(), 1);
//! assert_eq!(citations[0].title, "Example Article");
//! assert_eq!(citations[0].doi.as_deref(), Some("10.1000/example"));
//! ```

mod parse;

use crate::error::ParseError;
use crate::{Citation, CitationParser};
pub(crate) use parse::looks_like_bib;
use parse::parse_bib;

/// Parser for BibTeX / BibLaTeX (`.bib`) files.
#[derive(Debug, Clone, Default)]
pub struct BibParser;

impl BibParser {
    /// Creates a new `.bib` parser instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CitationParser for BibParser {
    fn parse(&self, input: &str) -> Result<Vec<Citation>, ParseError> {
        if input.trim().is_empty() {
            return Ok(Vec::new());
        }

        parse_bib(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CitationFormat;
    #[cfg(feature = "diagnostics")]
    use crate::parse_with_diagnostics;

    #[test]
    fn test_parse_simple_article() {
        let input = r#"@article{smith2024,
  title = {Example Article},
  author = {Smith, John and Doe, Jane},
  date = {2024-05-02},
  doi = {10.1000/example},
  url = {https://doi.org/10.1000/example}
}"#;

        let citations = BibParser::new().parse(input).unwrap();
        assert_eq!(citations.len(), 1);
        let citation = &citations[0];
        assert_eq!(citation.citation_type, vec!["article"]);
        assert_eq!(citation.title, "Example Article");
        assert_eq!(citation.authors.len(), 2);
        assert_eq!(citation.authors[0].name, "Smith");
        assert_eq!(citation.authors[0].given_name.as_deref(), Some("John"));
        assert_eq!(citation.authors[1].name, "Doe");
        assert_eq!(citation.authors[1].given_name.as_deref(), Some("Jane"));
        assert_eq!(citation.doi.as_deref(), Some("10.1000/example"));
        assert_eq!(citation.urls.len(), 1);
        assert_eq!(citation.date.as_ref().map(|d| d.year), Some(2024));
    }

    #[test]
    fn test_parse_three_authors() {
        let input = r#"@article{smith2024,
  title = {Example Article},
  author = {Smith, John and Doe, Jane and Brown, Alex}
}"#;

        let citation = BibParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.authors.len(), 3);
        assert_eq!(citation.authors[0].name, "Smith");
        assert_eq!(citation.authors[0].given_name.as_deref(), Some("John"));
        assert_eq!(citation.authors[1].name, "Doe");
        assert_eq!(citation.authors[1].given_name.as_deref(), Some("Jane"));
        assert_eq!(citation.authors[2].name, "Brown");
        assert_eq!(citation.authors[2].given_name.as_deref(), Some("Alex"));
    }

    #[test]
    fn test_parse_title_and_subtitle() {
        let input = r#"@book{titlecase,
  title = {Main Title},
  subtitle = {Practical Guide},
  editor = {Doe, Jane}
}"#;

        let citation = BibParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.title, "Main Title: Practical Guide");
        assert_eq!(citation.authors.len(), 1);
        assert_eq!(citation.authors[0].name, "Doe");
        assert_eq!(
            citation.extra_fields.get("editor"),
            Some(&vec!["Doe, Jane".to_string()])
        );
    }

    #[test]
    fn test_parse_journal_priority() {
        let input = r#"@article{journalpriority,
  title = {Example},
  author = {Smith, John},
  journaltitle = {Journal Title},
  journal = {Fallback Journal},
  booktitle = {Proceedings Title}
}"#;

        let citation = BibParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.journal.as_deref(), Some("Journal Title"));
        assert_eq!(
            citation.extra_fields.get("journal"),
            Some(&vec!["Fallback Journal".to_string()])
        );
        assert_eq!(
            citation.extra_fields.get("booktitle"),
            Some(&vec!["Proceedings Title".to_string()])
        );
    }

    #[test]
    fn test_parse_string_macros_and_concat() {
        let input = r#"@string{jmlr = {Journal of Machine Learning Research}}
@article{macrocase,
  title = {Example},
  author = {Smith, John},
  journaltitle = jmlr # { Archive},
  year = {2024},
  month = jan
}"#;

        let citation = BibParser::new().parse(input).unwrap().remove(0);
        assert_eq!(
            citation.journal.as_deref(),
            Some("Journal of Machine Learning Research Archive")
        );
        let date = citation.date.expect("date");
        assert_eq!(date.year, 2024);
        assert_eq!(date.month, Some(1));
    }

    #[test]
    fn test_parse_crossref_and_xdata_inheritance() {
        let input = r#"@xdata{xcommon,
  publisher = {Shared Publisher},
  langid = {english}
}

@proceedings{conf2024,
  title = {Conference Proceedings},
  year = {2024},
  booktitle = {Conference Proceedings},
  xdata = {xcommon}
}

@inproceedings{child2024,
  title = {Child Paper},
  author = {Doe, Jane},
  crossref = {conf2024}
}"#;

        let citations = BibParser::new().parse(input).unwrap();
        let child = citations
            .iter()
            .find(|citation| citation.title == "Child Paper")
            .unwrap();
        assert_eq!(child.publisher.as_deref(), Some("Shared Publisher"));
        assert_eq!(child.language.as_deref(), Some("english"));
        assert_eq!(child.journal.as_deref(), Some("Conference Proceedings"));
        assert_eq!(
            child.extra_fields.get("crossref"),
            Some(&vec!["conf2024".to_string()])
        );
    }

    #[test]
    fn test_missing_parent_is_soft_failure() {
        let input = r#"@article{missingparent,
  title = {Example},
  author = {Smith, John},
  crossref = {unknown-parent}
}"#;

        let citation = BibParser::new().parse(input).unwrap().remove(0);
        assert_eq!(
            citation.extra_fields.get("crossref"),
            Some(&vec!["unknown-parent".to_string()])
        );
    }

    #[test]
    fn test_unresolved_macro_preserves_raw_extra_field() {
        let input = r#"@article{unresolved,
  title = {Example},
  author = {Smith, John},
  note = unknownmacro # { appendix}
}"#;

        let citation = BibParser::new().parse(input).unwrap().remove(0);
        assert_eq!(
            citation.extra_fields.get("note"),
            Some(&vec!["unknownmacro # { appendix}".to_string()])
        );
    }

    #[test]
    fn test_detect_looks_like_bib() {
        assert!(looks_like_bib("@article{a, title={Example}}"));
        assert!(looks_like_bib(" \n\t@string{name = {Value}}"));
        assert!(!looks_like_bib("article{a, title={Example}}"));
        assert!(!looks_like_bib("@ not really bib"));
    }

    #[test]
    fn test_unterminated_brace_reports_span() {
        let input = "@article{broken,\n  title = {Example,\n  author = {Smith, John}\n}";
        let err = BibParser::new().parse(input).unwrap_err();
        assert_eq!(err.format, CitationFormat::Bib);
        assert!(matches!(err.error, crate::ValueError::Syntax(_)));
        assert!(err.line.is_some());
        assert!(err.span.is_some());
    }

    #[test]
    fn test_identity_less_entry_errors() {
        let input = r#"@misc{empty,
  note = {Only a note}
}"#;

        let err = BibParser::new().parse(input).unwrap_err();
        assert_eq!(err.format, CitationFormat::Bib);
        assert!(err.line.is_some());
        assert!(err.span.is_some());
    }

    #[cfg(feature = "diagnostics")]
    #[test]
    fn test_bib_diagnostics() {
        let input = r#"@misc{empty,
  note = {Only a note}
}"#;
        let diag = parse_with_diagnostics(&BibParser::new(), input, "refs.bib").unwrap_err();
        assert!(diag.contains("refs.bib"));
        assert!(diag.contains("Bib"));
    }
}
