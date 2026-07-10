//! RIS format parsing implementation.
//!
//! This module handles the low-level parsing of RIS formatted text.

use crate::ris::structure::RawRisData;
use crate::ris::tags::RisTag;
use crate::utils::parse_author_name;
use crate::{
    Author, CitationFormat,
    error::{ParseError, SourceSpan, ValueError},
};

/// Parse the content of a RIS formatted file, returning structured data.
pub(crate) fn ris_parse<S: AsRef<str>>(ris_text: S) -> Result<Vec<RawRisData>, ParseError> {
    let text = ris_text.as_ref();

    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut citations = Vec::new();
    let mut current_citation = RawRisData::new();
    let mut line_number = 0;
    let text_ptr = text.as_ptr() as usize;
    // Track the last successfully parsed non-author tag for continuation line appending.
    let mut last_tag: Option<RisTag> = None;

    for raw_line in text.lines() {
        line_number += 1;
        let line_byte_start = raw_line.as_ptr() as usize - text_ptr;
        let line = strip_leading_bom(raw_line, line_number);

        if line.trim().is_empty() {
            continue;
        }

        let line_byte_end = line_byte_start + raw_line.len();
        let trimmed_line = line.trim();

        if is_metadata_line(trimmed_line) {
            continue;
        }

        match classify_ris_line(line) {
            RisLineKind::Continuation => {
                if append_continuation_line(
                    &mut current_citation,
                    last_tag.as_ref(),
                    line_byte_end,
                    trimmed_line,
                ) {
                    continue;
                }

                if let Some(ref mut span) = current_citation.record_span {
                    span.end = line_byte_end;
                }
                current_citation.add_ignored_line(line_number, trimmed_line.to_string());
                continue;
            }
            RisLineKind::Invalid => {
                if let Some(ref mut span) = current_citation.record_span {
                    span.end = line_byte_end;
                }
                last_tag = None;
                current_citation.add_ignored_line(line_number, trimmed_line.to_string());
                continue;
            }
            RisLineKind::Tag => {}
        }

        match parse_ris_line(trimmed_line, line_number) {
            Ok((tag, content)) => match tag {
                RisTag::Type => {
                    if current_citation.has_content() {
                        citations.push(current_citation);
                        current_citation = RawRisData::new();
                    }
                    last_tag = None;
                    current_citation.start_line = Some(line_number);
                    current_citation.record_span =
                        Some(SourceSpan::new(line_byte_start, line_byte_end));
                    current_citation.add_data(tag, content);
                }
                RisTag::EndOfReference => {
                    if let Some(ref mut span) = current_citation.record_span {
                        span.end = line_byte_end;
                    }
                    last_tag = None;
                    if current_citation.has_content() {
                        citations.push(current_citation);
                        current_citation = RawRisData::new();
                    }
                }
                tag if tag.is_author_tag() => {
                    if let Some(ref mut span) = current_citation.record_span {
                        span.end = line_byte_end;
                    }
                    last_tag = None;
                    let authors = split_and_parse_authors(&content);
                    for author in authors {
                        current_citation.add_author(author);
                    }
                }
                _ => {
                    if let Some(ref mut span) = current_citation.record_span {
                        span.end = line_byte_end;
                    }
                    last_tag = Some(tag.clone());
                    current_citation.add_data(tag, content);
                }
            },
            Err(_) => {
                if let Some(ref mut span) = current_citation.record_span {
                    span.end = line_byte_end;
                }
                last_tag = None;
                current_citation.add_ignored_line(line_number, trimmed_line.to_string());
            }
        }
    }

    if current_citation.has_content() {
        citations.push(current_citation);
    }

    if citations.is_empty() {
        return Ok(Vec::new());
    }

    Ok(citations)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RisLineKind {
    Tag,
    Continuation,
    Invalid,
}

fn strip_leading_bom(line: &str, line_number: usize) -> &str {
    if line_number == 1 {
        line.strip_prefix('\u{feff}').unwrap_or(line)
    } else {
        line
    }
}

fn append_continuation_line(
    current_citation: &mut RawRisData,
    last_tag: Option<&RisTag>,
    line_byte_end: usize,
    continuation: &str,
) -> bool {
    let Some(tag) = last_tag else {
        return false;
    };

    let Some(values) = current_citation.data.get_mut(tag) else {
        return false;
    };

    let Some(last_val) = values.last_mut() else {
        return false;
    };

    if let Some(ref mut span) = current_citation.record_span {
        span.end = line_byte_end;
    }

    if !last_val.is_empty() {
        last_val.push(' ');
    }
    last_val.push_str(continuation);
    true
}

fn classify_ris_line(line: &str) -> RisLineKind {
    let bytes = line.as_bytes();

    if bytes.len() < 2 {
        return RisLineKind::Continuation;
    }

    if has_valid_tag_prefix(bytes) {
        return RisLineKind::Tag;
    }

    if looks_like_invalid_tag_line(bytes) {
        return RisLineKind::Invalid;
    }

    RisLineKind::Continuation
}

fn has_valid_tag_prefix(bytes: &[u8]) -> bool {
    if bytes.len() < 2 {
        return false;
    }

    bytes[0].is_ascii_alphanumeric()
        && bytes[1].is_ascii_alphanumeric()
        && separator_kind(bytes).is_some()
}

fn looks_like_invalid_tag_line(bytes: &[u8]) -> bool {
    separator_kind(bytes).is_some()
}

fn separator_kind(bytes: &[u8]) -> Option<usize> {
    if bytes.len() >= 6 && &bytes[2..6] == b"  - " {
        return Some(6);
    }

    if bytes.len() >= 5 && &bytes[2..5] == b"  -" {
        return Some(5);
    }

    if bytes.len() >= 4 && &bytes[2..4] == b" -" {
        return Some(4);
    }

    if bytes.len() >= 4 && &bytes[2..4] == b"- " {
        return Some(4);
    }

    if bytes.len() >= 3 && bytes[2] == b'-' {
        return Some(3);
    }

    None
}

/// Parse a single RIS line into a tag and content.
fn parse_ris_line(line: &str, line_number: usize) -> Result<(RisTag, String), ParseError> {
    let bytes = line.as_bytes();

    if bytes.len() < 2 {
        return Err(ParseError::at_line(
            line_number,
            CitationFormat::Ris,
            ValueError::Syntax(format!(
                "Line too short for RIS format (minimum 2 chars): '{}'",
                line
            )),
        ));
    }

    if !bytes[0].is_ascii_alphanumeric() || !bytes[1].is_ascii_alphanumeric() {
        return Err(ParseError::at_line(
            line_number,
            CitationFormat::Ris,
            ValueError::Syntax(format!(
                "Invalid RIS tag format: '{}'",
                String::from_utf8_lossy(&bytes[..bytes.len().min(2)])
            )),
        ));
    }

    let tag_str = std::str::from_utf8(&bytes[..2]).expect("validated ASCII RIS tag");
    let tag = RisTag::from_tag(tag_str);
    let content = extract_ris_content(line, bytes, line_number)?;

    Ok((tag, content))
}

/// Extract content from a RIS line, handling various format patterns.
fn extract_ris_content(line: &str, bytes: &[u8], line_number: usize) -> Result<String, ParseError> {
    if let Some(content_start) = separator_kind(bytes) {
        return Ok(line[content_start..].trim().to_string());
    }

    if bytes.len() > 2 {
        let third_char = bytes[2] as char;
        if third_char == ' ' || third_char == '-' {
            return Ok(line[2..].trim().to_string());
        }
    }

    Err(ParseError::at_line(
        line_number,
        CitationFormat::Ris,
        ValueError::Syntax(format!(
            "RIS line missing proper separator (space or dash) after tag: '{}'",
            line
        )),
    ))
}

/// Split an author string into multiple authors and parse each one.
///
/// Handles non-standard RIS files where multiple authors are on a single AU line.
/// Splits on:
/// - Semicolons (`;`) - primary separator
/// - ` & ` and ` and ` - secondary separators (with surrounding spaces)
///
/// Does NOT split on bare commas since "Last, First" format uses commas.
fn split_and_parse_authors(author_str: &str) -> Vec<Author> {
    let trimmed = author_str.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let segments: Vec<&str> = trimmed.split(';').collect();
    let mut authors = Vec::new();

    for segment in segments {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        let sub_segments: Vec<&str> = segment
            .split(" & ")
            .flat_map(|s| s.split(" and "))
            .collect();

        for sub in sub_segments {
            let sub = sub.trim();
            if !sub.is_empty() {
                authors.push(parse_author(sub));
            }
        }
    }

    if authors.is_empty() {
        authors.push(parse_author(trimmed));
    }

    authors
}

/// Parse an author string into an Author struct.
fn parse_author(author_str: &str) -> Author {
    let (family, given) = parse_author_name(author_str);
    let (given_opt, middle_opt) = if given.is_empty() {
        (None, None)
    } else {
        crate::utils::split_given_and_middle(&given)
    };
    Author {
        name: family,
        given_name: given_opt,
        middle_name: middle_opt,
        affiliations: Vec::new(),
    }
}

/// Check if a line is RIS metadata that should be ignored.
fn is_metadata_line(line: &str) -> bool {
    line.starts_with("Record #")
        || line.starts_with("Provider:")
        || line.starts_with("Content:")
        || line.starts_with("Database:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[rstest]
    #[case("TY  - JOUR", RisTag::Type, "JOUR")]
    #[case("TI  - Test Title", RisTag::Title, "Test Title")]
    #[case("AU  - Smith, John", RisTag::Author, "Smith, John")]
    #[case("ER  -", RisTag::EndOfReference, "")]
    #[case("DO  - 10.1000/test", RisTag::Doi, "10.1000/test")]
    #[case("TY Content", RisTag::Type, "Content")]
    #[case("TY-Content", RisTag::Type, "Content")]
    fn test_parse_ris_line_valid(
        #[case] line: &str,
        #[case] expected_tag: RisTag,
        #[case] expected_content: &str,
    ) {
        let result = parse_ris_line(line, 1).unwrap();
        assert_eq!(result.0, expected_tag);
        assert_eq!(result.1, expected_content);
    }

    #[rstest]
    #[case("")]
    #[case("A")]
    #[case("!!  - Invalid tag")]
    #[case("TYNoSeparator")]
    #[case("TYBAD")]
    fn test_parse_ris_line_invalid(#[case] line: &str) {
        let result = parse_ris_line(line, 1);
        assert!(result.is_err());
    }

    #[rstest]
    #[case("Record #1 of 10", true)]
    #[case("Provider: Some Provider", true)]
    #[case("Content: text/plain", true)]
    #[case("Database: PubMed", true)]
    #[case("TY  - JOUR", false)]
    fn test_is_metadata_line(#[case] line: &str, #[case] expected: bool) {
        assert_eq!(is_metadata_line(line), expected);
    }

    #[test]
    fn test_parse_simple_citation() {
        let input = r#"TY  - JOUR
TI  - Test Article
AU  - Smith, John
ER  -"#;

        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);

        let raw = &result[0];
        assert_eq!(raw.get_first(&RisTag::Type), Some(&"JOUR".to_string()));
        assert_eq!(
            raw.get_first(&RisTag::Title),
            Some(&"Test Article".to_string())
        );
        assert_eq!(raw.authors.len(), 1);
        assert_eq!(raw.authors[0].name, "Smith");
    }

    #[test]
    fn test_parse_multiple_citations() {
        let input = r#"TY  - JOUR
TI  - First Article
AU  - Smith, John
ER  -

TY  - BOOK
TI  - Second Article
AU  - Doe, Jane
ER  -"#;

        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 2);

        assert_eq!(
            result[0].get_first(&RisTag::Type),
            Some(&"JOUR".to_string())
        );
        assert_eq!(
            result[1].get_first(&RisTag::Type),
            Some(&"BOOK".to_string())
        );
    }

    #[test]
    fn test_parse_with_metadata() {
        let input = r#"Record #1 of 2
Provider: Test Provider
Database: Test DB

TY  - JOUR
TI  - Test Article
AU  - Smith, John
ER  -"#;

        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].get_first(&RisTag::Title),
            Some(&"Test Article".to_string())
        );
    }

    #[test]
    fn test_parse_empty_input() {
        let result = ris_parse("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_no_valid_citations() {
        let input = r#"Record #1 of 0
Provider: Test Provider"#;

        let result = ris_parse(input).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_with_invalid_lines() {
        let input = r#"TY  - JOUR
TI  - Test Article
!! - This is truly invalid
AU  - Smith, John
ER  -"#;

        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].ignored_lines.len(), 1);
        assert!(result[0].ignored_lines[0].1.contains("!!"));
    }

    #[test]
    fn test_parse_author() {
        let author = parse_author("Smith, John");
        assert_eq!(author.name, "Smith");
        assert_eq!(author.given_name.as_deref(), Some("John"));
        assert!(author.affiliations.is_empty());
    }

    #[test]
    fn test_split_authors_single() {
        let authors = split_and_parse_authors("Smith, John");
        assert_eq!(authors.len(), 1);
        assert_eq!(authors[0].name, "Smith");
    }

    #[test]
    fn test_split_authors_semicolon() {
        let authors = split_and_parse_authors("Smith, J.; Doe, A.; Brown, B.");
        assert_eq!(authors.len(), 3);
        assert_eq!(authors[0].name, "Smith");
        assert_eq!(authors[1].name, "Doe");
        assert_eq!(authors[2].name, "Brown");
    }

    #[test]
    fn test_split_authors_ampersand() {
        let authors = split_and_parse_authors("Smith, J. & Doe, A.");
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].name, "Smith");
        assert_eq!(authors[1].name, "Doe");
    }

    #[test]
    fn test_split_authors_and() {
        let authors = split_and_parse_authors("Smith, J. and Doe, A.");
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].name, "Smith");
        assert_eq!(authors[1].name, "Doe");
    }

    #[test]
    fn test_split_authors_mixed() {
        let authors = split_and_parse_authors("Smith, J.; Doe, A. & Brown, B.");
        assert_eq!(authors.len(), 3);
        assert_eq!(authors[0].name, "Smith");
        assert_eq!(authors[1].name, "Doe");
        assert_eq!(authors[2].name, "Brown");
    }

    #[test]
    fn test_split_authors_reported_issue() {
        let authors = split_and_parse_authors("Abebe, T., Alemu, B., & Teshome, M");
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].name, "Abebe");
        assert_eq!(authors[1].name, "Teshome");
    }

    #[test]
    fn test_split_authors_empty() {
        let authors = split_and_parse_authors("");
        assert!(authors.is_empty());
    }

    #[test]
    fn test_bom_prefixed_input_parses_without_panic() {
        let input = "\u{feff}TY  - JOUR\nTI  - Test\nER  -\n";
        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].get_first(&RisTag::Type),
            Some(&"JOUR".to_string())
        );
    }

    #[test]
    fn test_n2_continuation_line_without_leading_space() {
        let input = "TY  - JOUR\nTI  - Test\nN2  - Brief Summary\nAt present, there are no relevant studies.\nER  -\n";
        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let abstract_val = result[0].get_first(&RisTag::AbstractAlternative).unwrap();
        assert_eq!(
            abstract_val,
            "Brief Summary At present, there are no relevant studies."
        );
    }

    #[test]
    fn test_ab_continuation_line_without_leading_space() {
        let input = "TY  - JOUR\nTI  - Test\nAB  - First sentence.\nSecond sentence continues here.\nER  -\n";
        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let abstract_val = result[0].get_first(&RisTag::Abstract).unwrap();
        assert_eq!(
            abstract_val,
            "First sentence. Second sentence continues here."
        );
    }

    #[test]
    fn test_kw_continuation_line_without_leading_space() {
        let input =
            "TY  - JOUR\nTI  - Test\nKW  - analysis of variance\nchild\ncontrolled study\nER  -\n";
        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let keyword = result[0].get_first(&RisTag::Keywords).unwrap();
        assert_eq!(keyword, "analysis of variance child controlled study");
    }

    #[test]
    fn test_title_continuation_line_without_leading_space() {
        let input = "TY  - JOUR\nTI  - Main title\ncontinued subtitle\nER  -\n";
        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let title = result[0].get_first(&RisTag::Title).unwrap();
        assert_eq!(title, "Main title continued subtitle");
    }

    #[test]
    fn test_unicode_leading_continuation_line_without_panic() {
        let input = "TY  - JOUR\nTI  - Test\nAB  - First sentence.\nE\u{2010}learning continues here.\nER  -\n";
        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let abstract_val = result[0].get_first(&RisTag::Abstract).unwrap();
        assert_eq!(
            abstract_val,
            "First sentence. E\u{2010}learning continues here."
        );
    }

    #[test]
    fn test_invalid_fake_tag_line_is_ignored_not_appended() {
        let input = "TY  - JOUR\nTI  - Test\nKW  - analysis of variance\n!!  - invalid\nER  -\n";
        let result = ris_parse(input).unwrap();
        assert_eq!(result.len(), 1);
        let keyword = result[0].get_first(&RisTag::Keywords).unwrap();
        assert_eq!(keyword, "analysis of variance");
        assert_eq!(result[0].ignored_lines.len(), 1);
        assert_eq!(result[0].ignored_lines[0].1, "!!  - invalid");
    }
}
