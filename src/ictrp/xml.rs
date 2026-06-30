//! ICTRP XML format parser implementation.
//!
//! WHO ICTRP XML exports contain one `<Trial>` element per registry record.
//! This parser keeps the normalized ICTRP citation mapping aligned with the
//! existing ICTRP CSV parser while preserving raw XML fields in `extra_fields`.

use crate::error::{ParseError, SourceSpan, ValueError, fields};
use crate::ictrp::{
    dedupe_urls, is_ictrp_url_field, parse_ictrp_compact_date, parse_ictrp_standard_date,
};
use crate::{Citation, CitationFormat, CitationParser};
use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::name::QName;
use std::collections::HashMap;
use std::io::BufRead;

/// Parser for ICTRP XML exports.
#[derive(Debug, Clone, Default)]
pub struct IctrpXmlParser;

impl IctrpXmlParser {
    /// Creates a new ICTRP XML parser instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CitationParser for IctrpXmlParser {
    fn parse(&self, input: &str) -> Result<Vec<Citation>, ParseError> {
        if input.trim().is_empty() {
            return Ok(Vec::new());
        }

        if !looks_like_ictrp_xml(input) {
            return Err(ParseError::without_position(
                CitationFormat::IctrpXml,
                ValueError::Syntax("Input does not appear to be an ICTRP XML export".to_string()),
            ));
        }

        parse_ictrp_xml(input)
    }
}

pub(crate) fn looks_like_ictrp_xml(content: &str) -> bool {
    let trimmed = content.trim_start_matches('\u{feff}').trim_start();

    (trimmed.starts_with("<?xml") || trimmed.starts_with("<Trials_downloaded_from_ICTRP"))
        && content.contains("<Trials_downloaded_from_ICTRP")
        && content.contains("<Trial")
}

fn parse_ictrp_xml(content: &str) -> Result<Vec<Citation>, ParseError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut citations = Vec::new();
    let mut buf = Vec::new();

    loop {
        let pos = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.name() == QName(b"Trial") => {
                citations.push(parse_trial(&mut reader, &mut buf, content, pos)?);
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(xml_error(content, pos, pos, e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(citations)
}

fn parse_trial<B: BufRead>(
    reader: &mut Reader<B>,
    buf: &mut Vec<u8>,
    content: &str,
    start_pos: usize,
) -> Result<Citation, ParseError> {
    let mut fields = HashMap::<String, Vec<String>>::new();
    let mut urls = Vec::new();

    loop {
        let event_pos = reader.buffer_position() as usize;
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let closing_tag = e.name().as_ref().to_vec();
                let value =
                    extract_text_with_position(reader, buf, &closing_tag, content, event_pos)?;
                store_field(&mut fields, &mut urls, &tag_name, value);
            }
            Ok(Event::Empty(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                store_field(&mut fields, &mut urls, &tag_name, String::new());
            }
            Ok(Event::End(ref e)) if e.name() == QName(b"Trial") => {
                let end_pos = reader.buffer_position() as usize;
                return build_trial_citation(fields, urls, content, start_pos, end_pos);
            }
            Ok(Event::Eof) => {
                return Err(xml_error(
                    content,
                    start_pos,
                    reader.buffer_position() as usize,
                    "Unexpected EOF while parsing <Trial>".to_string(),
                ));
            }
            Err(e) => {
                return Err(xml_error(
                    content,
                    event_pos,
                    reader.buffer_position() as usize,
                    e.to_string(),
                ));
            }
            _ => {}
        }
        buf.clear();
    }
}

fn store_field(
    fields: &mut HashMap<String, Vec<String>>,
    urls: &mut Vec<String>,
    key: &str,
    value: String,
) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    let value = trimmed.to_string();
    if is_ictrp_url_field(key) {
        urls.push(value.clone());
        fields.entry(key.to_string()).or_default().push(value);
        return;
    }

    let field_values = normalize_field_values(key, &value);
    if field_values.is_empty() {
        return;
    }

    let entry = fields.entry(key.to_string()).or_default();
    for value in field_values {
        if !entry.contains(&value) {
            entry.push(value);
        }
    }
}

fn normalize_field_values(key: &str, value: &str) -> Vec<String> {
    let normalized = normalize_embedded_markup(value);

    if is_contact_field(key) {
        return split_contact_field_values(&normalized);
    }

    if normalized.is_empty() {
        Vec::new()
    } else {
        vec![normalized]
    }
}

fn is_contact_field(key: &str) -> bool {
    matches!(
        key,
        "Contact_Firstname"
            | "Contact_Lastname"
            | "Contact_Email"
            | "Contact_Tel"
            | "Contact_Affiliation"
    )
}

fn split_contact_field_values(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter(|part| {
            !part
                .chars()
                .all(|ch| matches!(ch, ';' | ',' | '/' | '\\' | '-' | '+'))
        })
        .map(str::to_string)
        .collect()
}

fn build_trial_citation(
    mut fields: HashMap<String, Vec<String>>,
    urls: Vec<String>,
    content: &str,
    start_pos: usize,
    end_pos: usize,
) -> Result<Citation, ParseError> {
    let accession_number = take_first_value(&mut fields, &["TrialID"]).ok_or_else(|| {
        trial_error(
            content,
            start_pos,
            end_pos,
            ValueError::MissingValue {
                field: fields::ACCESSION_NUMBER,
                key: "TrialID",
            },
        )
    })?;

    let title = if let Some(value) = take_first_value(&mut fields, &["Scientific_title"]) {
        value
    } else if let Some(value) = take_first_value(&mut fields, &["Public_title"]) {
        value
    } else {
        return Err(trial_error(
            content,
            start_pos,
            end_pos,
            ValueError::MissingValue {
                field: fields::TITLE,
                key: "Scientific_title/Public_title",
            },
        ));
    };

    let compact_date = first_value(&fields, &["Date_registration3"]);
    let fallback_date = first_value(&fields, &["Date_registration"]);
    let date = compact_date
        .as_deref()
        .and_then(parse_ictrp_compact_date)
        .or_else(|| fallback_date.as_deref().and_then(parse_ictrp_standard_date));

    if compact_date
        .as_deref()
        .and_then(parse_ictrp_compact_date)
        .is_some()
    {
        fields.remove("Date_registration3");
    } else if fallback_date
        .as_deref()
        .and_then(parse_ictrp_standard_date)
        .is_some()
    {
        fields.remove("Date_registration");
    }

    let publisher = take_first_value(&mut fields, &["Primary_sponsor"]);
    let mut citation_type = vec!["Clinical Trial".to_string()];
    if let Some(study_type) = take_first_value(&mut fields, &["Study_type"])
        && !study_type.trim().is_empty()
        && study_type != "Clinical Trial"
    {
        citation_type.push(study_type);
    }

    fields.remove("web_address");
    fields.remove("results_url_link");
    fields.remove("results_url_protocol");

    Ok(Citation {
        citation_type,
        title,
        authors: Vec::new(),
        journal: None,
        journal_abbr: None,
        date,
        volume: None,
        issue: None,
        pages: None,
        issn: Vec::new(),
        doi: None,
        accession_number: Some(accession_number),
        pmid: None,
        pmc_id: None,
        abstract_text: None,
        keywords: Vec::new(),
        urls: dedupe_urls(urls),
        language: None,
        mesh_terms: Vec::new(),
        publisher,
        extra_fields: fields,
    })
}

fn first_value(fields: &HashMap<String, Vec<String>>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(values) = fields.get(*key)
            && let Some(value) = values.iter().find(|value| !value.trim().is_empty())
        {
            return Some(value.clone());
        }
    }
    None
}

fn take_first_value(fields: &mut HashMap<String, Vec<String>>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(values) = fields.get_mut(*key)
            && let Some(index) = values.iter().position(|value| !value.trim().is_empty())
        {
            let value = values.remove(index);
            if values.is_empty() {
                fields.remove(*key);
            }
            return Some(value);
        }
    }
    None
}

fn normalize_embedded_markup(value: &str) -> String {
    let mut normalized = value.replace("\r\n", "\n");

    normalized = normalized
        .replace("\r<br />", "<br />")
        .replace("\r<br/>", "<br/>")
        .replace("\r<br>", "<br>");

    normalized = normalized
        .replace("&lt;br /&gt;", "\n")
        .replace("&lt;br/&gt;", "\n")
        .replace("&lt;br&gt;", "\n")
        .replace("<br />", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n");

    normalized = normalized.replace('\r', "\n");
    normalized = normalized
        .replace("&lt;=", "<=")
        .replace("&gt;=", ">=")
        .replace("&lt;", "<")
        .replace("&gt;", ">");

    let normalized = normalized
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    collapse_soft_wrapped_lines(&normalized)
}

fn collapse_soft_wrapped_lines(value: &str) -> String {
    let mut output = String::new();

    for line in value.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if !output.ends_with("\n\n") && !output.is_empty() {
                output.push('\n');
                output.push('\n');
            }
            continue;
        }

        if output.is_empty() {
            output.push_str(trimmed);
            continue;
        }

        if output.ends_with("\n\n") || starts_new_block(trimmed) {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(trimmed);
        } else {
            output.push(' ');
            output.push_str(trimmed);
        }
    }

    output
}

fn starts_new_block(line: &str) -> bool {
    if line.starts_with("- ") {
        return true;
    }

    let mut chars = line.chars().peekable();
    let mut saw_digit = false;
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            saw_digit = true;
            chars.next();
            continue;
        }
        break;
    }

    if !saw_digit || !matches!(chars.next(), Some('.')) {
        return false;
    }

    matches!(chars.peek(), Some(ch) if ch.is_whitespace())
}

fn trial_error(content: &str, start_pos: usize, end_pos: usize, error: ValueError) -> ParseError {
    ParseError::at_line(
        buffer_position_to_line_number(content, start_pos),
        CitationFormat::IctrpXml,
        error,
    )
    .with_span(SourceSpan::new(start_pos, end_pos))
}

fn xml_error(content: &str, start_pos: usize, end_pos: usize, detail: String) -> ParseError {
    ParseError::at_line(
        buffer_position_to_line_number(content, start_pos),
        CitationFormat::IctrpXml,
        ValueError::Syntax(format!("XML parsing error: {}", detail)),
    )
    .with_span(SourceSpan::new(start_pos, end_pos))
}

fn buffer_position_to_line_number(content: &str, pos: usize) -> usize {
    if pos >= content.len() {
        return content.lines().count();
    }

    content[..pos].lines().count()
}

fn extract_text_with_position<B: BufRead>(
    reader: &mut Reader<B>,
    buf: &mut Vec<u8>,
    closing_tag: &[u8],
    content: &str,
    start_pos: usize,
) -> Result<String, ParseError> {
    let mut text = String::new();
    let closing_tag_str = String::from_utf8_lossy(closing_tag);

    loop {
        let current_pos = reader.buffer_position() as usize;
        match reader.read_event_into(buf) {
            Ok(Event::Text(e)) => {
                let decoded = e.decode().map_err(|e| {
                    xml_error(
                        content,
                        current_pos,
                        reader.buffer_position() as usize,
                        e.to_string(),
                    )
                })?;
                let unescaped = unescape(&decoded).map_err(|e| {
                    xml_error(
                        content,
                        current_pos,
                        reader.buffer_position() as usize,
                        e.to_string(),
                    )
                })?;
                text.push_str(&unescaped);
            }
            Ok(Event::CData(e)) => {
                text.push_str(&e.decode().map_err(|e| {
                    xml_error(
                        content,
                        current_pos,
                        reader.buffer_position() as usize,
                        e.to_string(),
                    )
                })?);
            }
            Ok(Event::GeneralRef(e)) => {
                if let Some(ch) = e.resolve_char_ref().map_err(|e| {
                    xml_error(
                        content,
                        current_pos,
                        reader.buffer_position() as usize,
                        e.to_string(),
                    )
                })? {
                    text.push(ch);
                } else {
                    let decoded = e.decode().map_err(|e| {
                        xml_error(
                            content,
                            current_pos,
                            reader.buffer_position() as usize,
                            e.to_string(),
                        )
                    })?;

                    match decoded.as_ref() {
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "amp" => text.push('&'),
                        "apos" => text.push('\''),
                        "quot" => text.push('"'),
                        other => {
                            return Err(xml_error(
                                content,
                                current_pos,
                                reader.buffer_position() as usize,
                                format!("Unsupported entity reference: &{};", other),
                            ));
                        }
                    }
                }
            }
            Ok(Event::End(e)) if e.name() == QName(closing_tag) => break,
            Ok(Event::Eof) => {
                return Err(ParseError::at_line(
                    buffer_position_to_line_number(content, current_pos),
                    CitationFormat::IctrpXml,
                    ValueError::Syntax(format!(
                        "Unexpected EOF while looking for closing tag '{}'",
                        closing_tag_str
                    )),
                )
                .with_span(SourceSpan::new(
                    start_pos,
                    reader.buffer_position() as usize,
                )));
            }
            Err(e) => {
                return Err(xml_error(
                    content,
                    current_pos,
                    reader.buffer_position() as usize,
                    e.to_string(),
                ));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_looks_like_ictrp_xml() {
        let xml = "<?xml version='1.0' encoding='UTF-8'?><Trials_downloaded_from_ICTRP><Trial></Trial></Trials_downloaded_from_ICTRP>";
        assert!(looks_like_ictrp_xml(xml));
    }

    #[test]
    fn test_does_not_confuse_endnote_xml_for_ictrp_xml() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><xml><records><record><titles><title>Test</title></titles></record></records></xml>";
        assert!(!looks_like_ictrp_xml(xml));
    }

    #[test]
    fn test_parse_ictrp_xml() {
        let input = r#"<?xml version='1.0' encoding='UTF-8' ?>
<Trials_downloaded_from_ICTRP>
  <Trial>
    <TrialID>NCT00000001</TrialID>
    <Public_title>Public title</Public_title>
    <Scientific_title>Scientific title</Scientific_title>
    <Primary_sponsor>Sponsor</Primary_sponsor>
    <Date_registration3>20260501</Date_registration3>
    <Date_registration>01/05/2026</Date_registration>
    <Study_type>Interventional</Study_type>
    <web_address>https://example.test/study</web_address>
    <results_url_link>https://example.test/results</results_url_link>
    <Source_Register>ClinicalTrials.gov</Source_Register>
  </Trial>
</Trials_downloaded_from_ICTRP>"#;

        let citation = IctrpXmlParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.accession_number.as_deref(), Some("NCT00000001"));
        assert_eq!(citation.title, "Scientific title");
        assert_eq!(citation.publisher.as_deref(), Some("Sponsor"));
        assert_eq!(
            citation.citation_type,
            vec!["Clinical Trial", "Interventional"]
        );
        assert_eq!(citation.urls.len(), 2);
        assert!(!citation.extra_fields.contains_key("TrialID"));
        assert!(!citation.extra_fields.contains_key("Scientific_title"));
        assert!(!citation.extra_fields.contains_key("Primary_sponsor"));
        assert!(!citation.extra_fields.contains_key("Study_type"));
        assert!(!citation.extra_fields.contains_key("Date_registration3"));
        assert!(!citation.extra_fields.contains_key("web_address"));
        assert!(!citation.extra_fields.contains_key("results_url_link"));
        assert_eq!(
            citation.extra_fields.get("Public_title").unwrap(),
            &vec!["Public title".to_string()]
        );
    }

    #[test]
    fn test_parse_ictrp_xml_public_title_fallback() {
        let input = r#"<?xml version='1.0' encoding='UTF-8' ?>
<Trials_downloaded_from_ICTRP>
  <Trial>
    <TrialID>NCT00000002</TrialID>
    <Public_title>Public title</Public_title>
    <Scientific_title/>
    <Date_registration>01/05/2026</Date_registration>
  </Trial>
</Trials_downloaded_from_ICTRP>"#;

        let citation = IctrpXmlParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.title, "Public title");
        assert!(!citation.extra_fields.contains_key("Public_title"));
    }

    #[test]
    fn test_parse_ictrp_xml_missing_trial_id_errors() {
        let input = r#"<?xml version='1.0' encoding='UTF-8' ?>
<Trials_downloaded_from_ICTRP>
  <Trial>
    <Scientific_title>Scientific title</Scientific_title>
  </Trial>
</Trials_downloaded_from_ICTRP>"#;

        let err = IctrpXmlParser::new().parse(input).unwrap_err();
        assert_eq!(err.format, CitationFormat::IctrpXml);
        assert!(matches!(
            err.error,
            ValueError::MissingValue { key: "TrialID", .. }
        ));
    }

    #[test]
    fn test_parse_ictrp_xml_supports_hyphen_dates() {
        let input = r#"<?xml version='1.0' encoding='UTF-8' ?>
<Trials_downloaded_from_ICTRP>
  <Trial>
    <TrialID>NCT00000003</TrialID>
    <Scientific_title>Scientific title</Scientific_title>
    <Date_registration>2026-04-20</Date_registration>
  </Trial>
</Trials_downloaded_from_ICTRP>"#;

        let citation = IctrpXmlParser::new().parse(input).unwrap().remove(0);
        assert_eq!(
            citation.date,
            Some(crate::Date {
                year: 2026,
                month: Some(4),
                day: Some(20),
            })
        );
        assert!(!citation.extra_fields.contains_key("Date_registration"));
    }

    #[test]
    fn test_parse_ictrp_xml_ignores_empty_fields() {
        let input = r#"<?xml version='1.0' encoding='UTF-8' ?>
<Trials_downloaded_from_ICTRP>
  <Trial>
    <TrialID>NCT00000004</TrialID>
    <Scientific_title>Scientific title</Scientific_title>
    <results_url_link/>
    <results_url_protocol/>
  </Trial>
</Trials_downloaded_from_ICTRP>"#;

        let citation = IctrpXmlParser::new().parse(input).unwrap().remove(0);
        assert!(!citation.extra_fields.contains_key("results_url_link"));
        assert!(citation.urls.is_empty());
    }

    #[test]
    fn test_parse_ictrp_xml_dedupes_urls() {
        let input = r#"<?xml version='1.0' encoding='UTF-8' ?>
<Trials_downloaded_from_ICTRP>
  <Trial>
    <TrialID>NCT00000005</TrialID>
    <Scientific_title>Scientific title</Scientific_title>
    <web_address>https://example.test/study</web_address>
    <results_url_protocol>https://example.test/study</results_url_protocol>
  </Trial>
</Trials_downloaded_from_ICTRP>"#;

        let citation = IctrpXmlParser::new().parse(input).unwrap().remove(0);
        assert_eq!(
            citation.urls,
            vec!["https://example.test/study".to_string()]
        );
        assert!(!citation.extra_fields.contains_key("web_address"));
        assert!(!citation.extra_fields.contains_key("results_url_protocol"));
    }

    #[test]
    fn test_parse_ictrp_xml_preserves_unused_fallback_date_field() {
        let input = r#"<?xml version='1.0' encoding='UTF-8' ?>
<Trials_downloaded_from_ICTRP>
  <Trial>
    <TrialID>NCT00000006</TrialID>
    <Scientific_title>Scientific title</Scientific_title>
    <Date_registration3>20260501</Date_registration3>
    <Date_registration>01/05/2026</Date_registration>
  </Trial>
</Trials_downloaded_from_ICTRP>"#;

        let citation = IctrpXmlParser::new().parse(input).unwrap().remove(0);
        assert!(!citation.extra_fields.contains_key("Date_registration3"));
        assert_eq!(
            citation.extra_fields.get("Date_registration").unwrap(),
            &vec!["01/05/2026".to_string()]
        );
    }

    #[test]
    fn test_normalize_embedded_markup() {
        let input = "Inclusion Criteria:\r<br>\r<br>1. Age &gt;= 18 years.<br>2. Tumor margin &lt;= 12 cm and survival &gt; 50 days.";
        let normalized = normalize_embedded_markup(input);

        assert_eq!(
            normalized,
            "Inclusion Criteria:\n\n1. Age >= 18 years.\n2. Tumor margin <= 12 cm and survival > 50 days."
        );
    }

    #[test]
    fn test_parse_ictrp_xml_splits_and_dedupes_contact_fields() {
        let input = r#"<?xml version='1.0' encoding='UTF-8' ?>
<Trials_downloaded_from_ICTRP>
  <Trial>
    <TrialID>NCT00000007</TrialID>
    <Scientific_title>Scientific title</Scientific_title>
    <Contact_Firstname>; ;</Contact_Firstname>
    <Contact_Lastname>Jessica Sharpe, MD, PhD;Jennifer Whisenant, PhD</Contact_Lastname>
    <Contact_Email>jessica.m.sharpe@vumc.org;j.whisenant@vumc.org</Contact_Email>
    <Contact_Tel>615-936-8422;615-936-8422</Contact_Tel>
  </Trial>
</Trials_downloaded_from_ICTRP>"#;

        let citation = IctrpXmlParser::new().parse(input).unwrap().remove(0);
        assert!(!citation.extra_fields.contains_key("Contact_Firstname"));
        assert_eq!(
            citation.extra_fields.get("Contact_Lastname").unwrap(),
            &vec![
                "Jessica Sharpe, MD, PhD".to_string(),
                "Jennifer Whisenant, PhD".to_string()
            ]
        );
        assert_eq!(
            citation.extra_fields.get("Contact_Email").unwrap(),
            &vec![
                "jessica.m.sharpe@vumc.org".to_string(),
                "j.whisenant@vumc.org".to_string()
            ]
        );
        assert_eq!(
            citation.extra_fields.get("Contact_Tel").unwrap(),
            &vec!["615-936-8422".to_string()]
        );
    }

    #[test]
    fn test_normalize_embedded_markup_collapses_real_output_soft_wraps() {
        let input = "Inclusion Criteria:\r<br>\r<br>  2. Histopathologically confirmed rectal adenocarcinoma via colonoscopy; pMMR or MSS\r<br>     phenotype;\r<br>\r<br>  3. Rectal MRI stage II/III (excluding T4b); distal tumor margin &lt;= 12 cm from the anal\r<br>     verge;\r<br>\r<br>  9. Baseline laboratory evaluations completed as required, with results obtained within\r<br>     14 days before randomization, and laboratory values meeting the following criteria\r<br>     (per CTCAE 5.0):\r<br>\r<br>       -  Serum creatinine &lt;= 1.5Ã—upper limit of normal (ULN) or creatinine clearance &gt; 50\r<br>          mL/min (female: creatinine clearance = [140 - age (years)] Ã— body weight (kg) Ã—\r<br>          0.85 / (72 Ã— serum creatinine (mg/dL)); male: creatinine clearance = [140 - age\r<br>          (years)] Ã— body weight (kg) Ã— 1.00 / (72 Ã— serum creatinine (mg/dL)));\r<br>\r<br>  8. Treatment with immunosuppressants or corticosteroids within 1 month before\r<br>     enrollment;";
        let normalized = normalize_embedded_markup(input);

        assert!(
            normalized.contains("pMMR or MSS phenotype;"),
            "{normalized:?}"
        );
        assert!(
            normalized.contains("from the anal verge;"),
            "{normalized:?}"
        );
        assert!(
            normalized.contains("within 14 days before randomization"),
            "{normalized:?}"
        );
        assert!(
            normalized.contains("1 month before enrollment;"),
            "{normalized:?}"
        );
        assert!(
            normalized.contains("clearance > 50 mL/min"),
            "{normalized:?}"
        );
        assert!(normalized.contains("0.85 / (72"), "{normalized:?}");
        assert!(!normalized.contains("MSS\nphenotype"), "{normalized:?}");
        assert!(!normalized.contains("anal\nverge"), "{normalized:?}");
        assert!(!normalized.contains("within\n14 days"), "{normalized:?}");
        assert!(!normalized.contains("before\nenrollment"), "{normalized:?}");
        assert!(!normalized.contains("\n0.85 /"), "{normalized:?}");
        assert!(
            normalized.contains("\n\n-  Serum creatinine"),
            "{normalized:?}"
        );
    }

    #[test]
    fn test_normalize_embedded_markup_preserves_charref_line_breaks() {
        let input = "not part of\r\n     routine and all other\r\n     histological types";
        let normalized = normalize_embedded_markup(input);

        assert_eq!(
            normalized,
            "not part of routine and all other histological types"
        );
    }

    #[test]
    fn test_extract_text_preserves_escaped_markup_for_normalization() {
        let input = r#"<Trial><Inclusion_Criteria>Inclusion Criteria:&#x0D;&lt;br&gt;&#x0D;&lt;br&gt;1. Age &lt;= 2 and &gt;= 1</Inclusion_Criteria></Trial>"#;
        let mut reader = Reader::from_str(input);
        reader.config_mut().trim_text(false);
        let mut buf = Vec::new();

        loop {
            let pos = reader.buffer_position() as usize;
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) if e.name() == QName(b"Inclusion_Criteria") => {
                    let text = extract_text_with_position(
                        &mut reader,
                        &mut buf,
                        b"Inclusion_Criteria",
                        input,
                        pos,
                    )
                    .unwrap();
                    assert!(text.contains("<br>"), "{:?}", text);
                    assert!(text.contains("<= 2"), "{:?}", text);
                    assert!(text.contains(">= 1"), "{:?}", text);
                    return;
                }
                Ok(Event::Eof) => panic!("did not find Inclusion_Criteria"),
                Ok(_) => {}
                Err(e) => panic!("{}", e),
            }
            buf.clear();
        }
    }

    #[test]
    fn test_parse_ictrp_xml_sample_file() {
        let input = include_str!("../../tests/fixtures/ictrp/who-export-sample.xml");
        let citations = IctrpXmlParser::new().parse(input).unwrap();
        assert!(!citations.is_empty());

        let first_trial = citations
            .iter()
            .find(|citation| citation.accession_number.as_deref() == Some("NCT07596290"))
            .unwrap();
        let inclusion = &first_trial.extra_fields["Inclusion_Criteria"][0];
        assert!(
            inclusion.contains('\n'),
            "actual inclusion text: {:?}",
            inclusion
        );
        assert!(!inclusion.contains("<br>"));
        assert!(!inclusion.contains("brbr"));
        assert!(
            inclusion.contains("pMMR or MSS phenotype;"),
            "{inclusion:?}"
        );
        assert!(inclusion.contains("from the anal verge;"), "{inclusion:?}");
        assert!(
            inclusion.contains("within 14 days before randomization"),
            "{inclusion:?}"
        );
        assert!(
            inclusion.contains("1 month before enrollment;"),
            "{inclusion:?}"
        );
        assert!(inclusion.contains("<= 12 cm"));
        assert!(inclusion.contains(">= 2000/"));
        assert!(inclusion.contains("\n\n2. "), "{inclusion:?}");
        assert!(
            inclusion.contains("\n\n-  White blood cell count"),
            "{inclusion:?}"
        );
        assert!(
            inclusion.contains("\n\nExclusion Criteria:\n\n1. "),
            "{inclusion:?}"
        );
        assert!(!inclusion.contains("MSS\nphenotype"), "{inclusion:?}");
        assert!(!inclusion.contains("anal\nverge"), "{inclusion:?}");
        assert!(!inclusion.contains("within\n14 days"), "{inclusion:?}");
        assert!(!inclusion.contains("before\nenrollment"), "{inclusion:?}");
        assert!(!inclusion.contains("\n0.85 /"), "{inclusion:?}");

        let dense_record = citations
            .iter()
            .find(|citation| citation.accession_number.as_deref() == Some("NCT07549399"))
            .unwrap();
        let dense_inclusion = &dense_record.extra_fields["Inclusion_Criteria"][0];
        assert!(
            dense_inclusion.contains("part of routine care"),
            "{dense_inclusion:?}"
        );
        assert!(
            dense_inclusion.contains("other histological types"),
            "{dense_inclusion:?}"
        );

        let bullet_record = citations
            .iter()
            .find(|citation| citation.accession_number.as_deref() == Some("NCT07394192"))
            .unwrap();
        let bullet_inclusion = &bullet_record.extra_fields["Inclusion_Criteria"][0];
        assert!(
            bullet_inclusion.contains("follow-up visits"),
            "{bullet_inclusion:?}"
        );
    }
}
