//! EndNote XML parsing implementation.
//!
//! This module provides the core parsing logic for EndNote XML format.

use crate::error::{ParseError, SourceSpan, ValueError};
use crate::utils::{buffer_position_to_line_number, utf8_bom_len, xml_reader_position};
use crate::{Author, Citation, CitationFormat};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::Event;
use quick_xml::name::QName;
use std::io::BufRead;

/// Enhanced extract_text function that tracks line numbers for better error reporting
fn extract_text_with_position<B: BufRead>(
    reader: &mut Reader<B>,
    buf: &mut Vec<u8>,
    closing_tag: &[u8],
    content: &str,
    start_pos: usize,
) -> Result<String, ParseError> {
    let mut text = String::new();
    let closing_tag_str = String::from_utf8_lossy(closing_tag);

    let text_error = |pos: usize, detail: String| {
        ParseError::at_line(
            buffer_position_to_line_number(content, pos),
            CitationFormat::EndNoteXml,
            ValueError::Syntax(format!("Invalid XML text content: {}", detail)),
        )
    };

    loop {
        let current_pos = xml_reader_position(reader, content);
        match reader.read_event_into(buf) {
            Ok(Event::Text(e)) => {
                text.push_str(
                    &e.xml_content(XmlVersion::Explicit1_1)
                        .map_err(|e| text_error(current_pos, e.to_string()))?,
                );
            }
            Ok(Event::CData(e)) => {
                text.push_str(
                    &e.decode()
                        .map_err(|e| text_error(current_pos, e.to_string()))?,
                );
            }
            // quick-xml reports `&amp;`, `&#x3B1;` etc. as separate events, between
            // the surrounding text events.
            Ok(Event::GeneralRef(e)) => {
                if let Some(ch) = e
                    .resolve_char_ref()
                    .map_err(|e| text_error(current_pos, e.to_string()))?
                {
                    text.push(ch);
                } else {
                    let name = e
                        .decode()
                        .map_err(|e| text_error(current_pos, e.to_string()))?;
                    match name.as_ref() {
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "amp" => text.push('&'),
                        "apos" => text.push('\''),
                        "quot" => text.push('"'),
                        other => {
                            return Err(text_error(
                                current_pos,
                                format!("unsupported entity reference &{};", other),
                            ));
                        }
                    }
                }
            }
            Ok(Event::End(e)) if e.name() == QName(closing_tag) => break,
            Ok(Event::Eof) => {
                let line_num = buffer_position_to_line_number(content, current_pos);
                let end_pos = xml_reader_position(reader, content);
                return Err(ParseError::at_line(
                    line_num,
                    CitationFormat::EndNoteXml,
                    ValueError::Syntax(format!(
                        "Unexpected EOF while looking for closing tag '{}'",
                        closing_tag_str
                    )),
                )
                .with_span(SourceSpan::new(start_pos, end_pos)));
            }
            Err(e) => {
                let line_num = buffer_position_to_line_number(content, current_pos);
                let end_pos = xml_reader_position(reader, content);
                return Err(ParseError::at_line(
                    line_num,
                    CitationFormat::EndNoteXml,
                    ValueError::Syntax(format!("XML parsing error: {}", e)),
                )
                .with_span(SourceSpan::new(start_pos, end_pos)));
            }
            _ => continue,
        }
        buf.clear();
    }

    Ok(text.trim().to_string())
}

/// Parse EndNote XML content into citations.
///
/// This function parses EndNote XML format and returns a vector of citations.
/// It uses quick_xml for robust XML parsing and converts to our Citation structure.
///
/// # Arguments
///
/// * `content` - The EndNote XML content as a string
///
/// # Returns
///
/// A Result containing either a vector of citations or a parsing error.
///
/// ```
pub(crate) fn parse_endnote_xml(content: &str) -> Result<Vec<Citation>, ParseError> {
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut reader = Reader::from_str(&content[utf8_bom_len(content)..]);
    reader.config_mut().trim_text(false);
    // Not `trim_text(true)`: it trims each text event separately, which drops the
    // spaces between inline `<style>` runs and around entity references. Field
    // text is trimmed as a whole in `extract_text_with_position` instead.

    let mut citations = Vec::new();
    let mut buf = Vec::new();

    loop {
        let pos = xml_reader_position(&reader, content);
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.name() == QName(b"record") => {
                citations.push(parse_record(&mut reader, &mut buf, content, pos)?);
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                let pos = xml_reader_position(&reader, content);
                let line = buffer_position_to_line_number(content, pos);
                return Err(ParseError::at_line(
                    line,
                    CitationFormat::EndNoteXml,
                    ValueError::Syntax(format!("XML parsing error: {}", e)),
                ));
            }
            _ => (),
        }
        buf.clear();
    }

    // Return empty vector instead of error for empty but valid XML
    Ok(citations)
}

/// Extracts date components (year, month, day) from a year element
fn extract_date_from_year_element<B: BufRead>(
    reader: &mut Reader<B>,
    e: &quick_xml::events::BytesStart,
    content: &str,
) -> Result<(Option<i32>, Option<u8>, Option<u8>), ParseError> {
    let mut year_val = None;
    let mut month_val = None;
    let mut day_val = None;

    // First, extract attributes if present
    let attr_pos = xml_reader_position(reader, content);
    let attr_line = buffer_position_to_line_number(content, attr_pos);
    for attr in e.attributes() {
        let attr = attr.map_err(|e| {
            ParseError::at_line(
                attr_line,
                CitationFormat::EndNoteXml,
                ValueError::Syntax(format!("Invalid attribute: {}", e)),
            )
        })?;
        let key = attr.key.as_ref();
        if !matches!(key, b"year" | b"month" | b"day") {
            continue;
        }
        // Decode like any other attribute, so `year="&#50;020"` reads as 2020.
        let value = attr
            .decoded_and_normalized_value(XmlVersion::Explicit1_1, reader.decoder())
            .map_err(|e| {
                ParseError::at_line(
                    attr_line,
                    CitationFormat::EndNoteXml,
                    ValueError::Syntax(format!("Invalid attribute value: {}", e)),
                )
            })?;
        match key {
            b"year" => year_val = value.parse::<i32>().ok(),
            b"month" => {
                month_val = value.parse::<u8>().ok().filter(|&m| (1..=12).contains(&m));
            }
            _ => {
                day_val = value.parse::<u8>().ok().filter(|&d| (1..=31).contains(&d));
            }
        }
    }

    // If no year attribute, try to get year from text content
    if year_val.is_none() {
        let mut local_buf = Vec::new();
        let start_pos = xml_reader_position(reader, content);
        if let Ok(year) =
            extract_text_with_position(reader, &mut local_buf, b"year", content, start_pos)?
                .parse::<i32>()
        {
            year_val = Some(year);
        }
    } else {
        // Still need to consume the text content
        let mut local_buf = Vec::new();
        let start_pos = xml_reader_position(reader, content);
        let _ = extract_text_with_position(reader, &mut local_buf, b"year", content, start_pos)?;
    }

    Ok((year_val, month_val, day_val))
}

/// Error for input that ends before `</closing_tag>` inside a record.
fn unexpected_eof<B: BufRead>(
    content: &str,
    start_pos: usize,
    reader: &Reader<B>,
    closing_tag: &str,
) -> ParseError {
    ParseError::at_line(
        buffer_position_to_line_number(content, start_pos),
        CitationFormat::EndNoteXml,
        ValueError::Syntax(format!(
            "Unexpected EOF while looking for closing tag '{}'",
            closing_tag
        )),
    )
    .with_span(SourceSpan::new(
        start_pos,
        xml_reader_position(reader, content),
    ))
}

/// Parse a single record element into a Citation
fn parse_record<B: BufRead>(
    reader: &mut Reader<B>,
    buf: &mut Vec<u8>,
    content: &str,
    start_pos: usize,
) -> Result<Citation, ParseError> {
    let mut citation = Citation::new();

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"ref-type" => {
                    let attr_pos = xml_reader_position(reader, content);
                    let attr_line = buffer_position_to_line_number(content, attr_pos);
                    for attr in e.attributes() {
                        let attr = attr.map_err(|e| {
                            ParseError::at_line(
                                attr_line,
                                CitationFormat::EndNoteXml,
                                ValueError::Syntax(format!("Invalid attribute: {}", e)),
                            )
                        })?;
                        if attr.key.as_ref() == b"name" {
                            citation.citation_type.push(
                                attr.decoded_and_normalized_value(
                                    XmlVersion::Explicit1_1,
                                    reader.decoder(),
                                )
                                .map_err(|e| {
                                    ParseError::at_line(
                                        attr_line,
                                        CitationFormat::EndNoteXml,
                                        ValueError::Syntax(format!(
                                            "Invalid attribute value: {}",
                                            e
                                        )),
                                    )
                                })?
                                .into_owned(),
                            );
                        }
                    }
                }
                b"title" => {
                    let pos = xml_reader_position(reader, content);
                    citation.title =
                        extract_text_with_position(reader, buf, b"title", content, pos)?;
                }
                b"author" => {
                    let pos = xml_reader_position(reader, content);
                    let author_str =
                        extract_text_with_position(reader, buf, b"author", content, pos)?;
                    let (family, given) = crate::utils::parse_author_name(&author_str);
                    let (given_opt, middle_opt) = if given.is_empty() {
                        (None, None)
                    } else {
                        crate::utils::split_given_and_middle(&given)
                    };
                    citation.authors.push(Author {
                        name: family,
                        given_name: given_opt,
                        middle_name: middle_opt,
                        affiliations: Vec::new(),
                    });
                }
                b"secondary-title" => {
                    let pos = xml_reader_position(reader, content);
                    let sec_title =
                        extract_text_with_position(reader, buf, b"secondary-title", content, pos)?;
                    // If no primary title, use secondary-title as title
                    if citation.title.is_empty() {
                        citation.title = sec_title;
                    } else {
                        citation.journal = Some(sec_title);
                    }
                }
                b"alt-title" => {
                    let pos = xml_reader_position(reader, content);
                    let alt_title =
                        extract_text_with_position(reader, buf, b"alt-title", content, pos)?;
                    // If no primary title or journal is set, use alt-title as title
                    if citation.title.is_empty() && citation.journal.is_none() {
                        citation.title = alt_title;
                    } else if citation.journal.is_none() {
                        citation.journal = Some(alt_title);
                    } else {
                        citation.journal_abbr = Some(alt_title);
                    }
                }
                b"custom2" => {
                    let pos = xml_reader_position(reader, content);
                    let text = extract_text_with_position(reader, buf, b"custom2", content, pos)?;
                    // Check for PMC ID patterns
                    if text.to_lowercase().contains("pmc") || text.starts_with("PMC") {
                        citation.pmc_id = Some(text);
                    }
                }
                b"accession-num" => {
                    let pos = xml_reader_position(reader, content);
                    citation.accession_number = Some(extract_text_with_position(
                        reader,
                        buf,
                        b"accession-num",
                        content,
                        pos,
                    )?);
                }
                b"volume" => {
                    let pos = xml_reader_position(reader, content);
                    citation.volume = Some(extract_text_with_position(
                        reader, buf, b"volume", content, pos,
                    )?);
                }
                b"number" => {
                    let pos = xml_reader_position(reader, content);
                    citation.issue = Some(extract_text_with_position(
                        reader, buf, b"number", content, pos,
                    )?);
                }
                b"pages" => {
                    let pos = xml_reader_position(reader, content);
                    let pages = extract_text_with_position(reader, buf, b"pages", content, pos)?;
                    citation.pages = Some(crate::utils::format_page_numbers(&pages));
                }
                b"electronic-resource-num" => {
                    let pos = xml_reader_position(reader, content);
                    let doi = extract_text_with_position(
                        reader,
                        buf,
                        b"electronic-resource-num",
                        content,
                        pos,
                    )?;
                    citation.doi = crate::utils::format_doi(&doi);
                }
                b"url" => {
                    let pos = xml_reader_position(reader, content);
                    let url = extract_text_with_position(reader, buf, b"url", content, pos)?;
                    if citation.doi.is_none() && url.contains("doi.org") {
                        citation.doi = crate::utils::format_doi(&url);
                    }
                    citation.urls.push(url);
                }
                b"year" => {
                    let (year_val, month_val, day_val) =
                        extract_date_from_year_element(reader, e, content)?;
                    citation.date = crate::utils::parse_endnote_date(year_val, month_val, day_val);
                }
                b"dates" => {
                    // Handle the dates element - we'll look for year sub-element
                    // This is a more complex structure but we'll process it
                    loop {
                        match reader.read_event_into(buf) {
                            Ok(Event::Start(ref inner_e)) if inner_e.name() == QName(b"year") => {
                                // Parse year element within dates
                                let (year_val, month_val, day_val) =
                                    extract_date_from_year_element(reader, inner_e, content)?;
                                citation.date =
                                    crate::utils::parse_endnote_date(year_val, month_val, day_val);
                            }
                            Ok(Event::End(ref inner_e)) if inner_e.name() == QName(b"dates") => {
                                break;
                            }
                            Ok(Event::Eof) => {
                                return Err(unexpected_eof(content, start_pos, reader, "dates"));
                            }
                            Err(e) => {
                                let pos = xml_reader_position(reader, content);
                                let line = buffer_position_to_line_number(content, pos);
                                return Err(ParseError::at_line(
                                    line,
                                    CitationFormat::EndNoteXml,
                                    ValueError::Syntax(format!("XML parsing error: {}", e)),
                                ));
                            }
                            _ => continue,
                        }
                        buf.clear();
                    }
                }
                b"abstract" => {
                    let pos = xml_reader_position(reader, content);
                    citation.abstract_text = Some(extract_text_with_position(
                        reader,
                        buf,
                        b"abstract",
                        content,
                        pos,
                    )?);
                }
                b"keyword" => {
                    let pos = xml_reader_position(reader, content);
                    citation.keywords.push(extract_text_with_position(
                        reader, buf, b"keyword", content, pos,
                    )?);
                }
                b"language" => {
                    let pos = xml_reader_position(reader, content);
                    citation.language = Some(extract_text_with_position(
                        reader,
                        buf,
                        b"language",
                        content,
                        pos,
                    )?);
                }
                b"publisher" => {
                    let pos = xml_reader_position(reader, content);
                    citation.publisher = Some(extract_text_with_position(
                        reader,
                        buf,
                        b"publisher",
                        content,
                        pos,
                    )?);
                }
                b"isbn" => {
                    let pos = xml_reader_position(reader, content);
                    let issns = extract_text_with_position(reader, buf, b"isbn", content, pos)?;
                    citation.issn.extend(crate::utils::split_issns(&issns));
                }
                _ => (),
            },
            Ok(Event::End(ref e)) if e.name() == QName(b"record") => break,
            Ok(Event::Eof) => return Err(unexpected_eof(content, start_pos, reader, "record")),
            Err(e) => {
                let pos = xml_reader_position(reader, content);
                let line = buffer_position_to_line_number(content, pos);
                return Err(ParseError::at_line(
                    line,
                    CitationFormat::EndNoteXml,
                    ValueError::Syntax(format!("XML parsing error: {}", e)),
                ));
            }
            _ => (),
        }
        buf.clear();
    }

    // Validate that we have at least a title or author
    if citation.title.is_empty() && citation.authors.is_empty() {
        let end_pos = xml_reader_position(reader, content);
        let line_num = buffer_position_to_line_number(content, start_pos);
        return Err(ParseError::at_line(
            line_num,
            CitationFormat::EndNoteXml,
            ValueError::MissingValue {
                field: "title or author",
                key: "title/author",
            },
        )
        .with_span(SourceSpan::new(start_pos, end_pos)));
    }

    Ok(citation)
}
