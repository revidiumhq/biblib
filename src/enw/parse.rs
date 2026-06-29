use crate::error::{ParseError, SourceSpan, ValueError};
use crate::{Author, Citation, CitationFormat};
use std::collections::HashMap;

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
enum EnwTag {
    ReferenceType,
    Author,
    SecondaryTitle,
    PlacePublished,
    Year,
    Editor,
    Label,
    Language,
    TranslatedAuthor,
    Publisher,
    Journal,
    Keywords,
    CallNumber,
    AccessionNumber,
    Issue,
    Pages,
    TranslatedTitle,
    ElectronicResourceNumber,
    TertiaryTitle,
    Title,
    Url,
    Volume,
    Abstract,
    TertiaryAuthor,
    Notes,
    WorkType,
    NumberOfVolumes,
    Edition,
    Date,
    SubsidiaryAuthor,
    IsbnIssn,
    OriginalPublication,
    PdfLink,
    AccessDate,
    Unknown(char),
}

impl EnwTag {
    fn from_code(code: char) -> Self {
        match code {
            '0' => Self::ReferenceType,
            'A' => Self::Author,
            'B' => Self::SecondaryTitle,
            'C' => Self::PlacePublished,
            'D' => Self::Year,
            'E' => Self::Editor,
            'F' => Self::Label,
            'G' => Self::Language,
            'H' => Self::TranslatedAuthor,
            'I' => Self::Publisher,
            'J' => Self::Journal,
            'K' => Self::Keywords,
            'L' => Self::CallNumber,
            'M' => Self::AccessionNumber,
            'N' => Self::Issue,
            'P' => Self::Pages,
            'Q' => Self::TranslatedTitle,
            'R' => Self::ElectronicResourceNumber,
            'S' => Self::TertiaryTitle,
            'T' => Self::Title,
            'U' => Self::Url,
            'V' => Self::Volume,
            'X' => Self::Abstract,
            'Y' => Self::TertiaryAuthor,
            'Z' => Self::Notes,
            '6' => Self::NumberOfVolumes,
            '7' => Self::Edition,
            '8' => Self::Date,
            '9' => Self::WorkType,
            '?' => Self::SubsidiaryAuthor,
            '@' => Self::IsbnIssn,
            '(' => Self::OriginalPublication,
            '>' => Self::PdfLink,
            '[' => Self::AccessDate,
            other => Self::Unknown(other),
        }
    }

    fn as_key(&self) -> String {
        match self {
            Self::ReferenceType => "%0".to_string(),
            Self::Author => "%A".to_string(),
            Self::SecondaryTitle => "%B".to_string(),
            Self::PlacePublished => "%C".to_string(),
            Self::Year => "%D".to_string(),
            Self::Editor => "%E".to_string(),
            Self::Label => "%F".to_string(),
            Self::Language => "%G".to_string(),
            Self::TranslatedAuthor => "%H".to_string(),
            Self::Publisher => "%I".to_string(),
            Self::Journal => "%J".to_string(),
            Self::Keywords => "%K".to_string(),
            Self::CallNumber => "%L".to_string(),
            Self::AccessionNumber => "%M".to_string(),
            Self::Issue => "%N".to_string(),
            Self::Pages => "%P".to_string(),
            Self::TranslatedTitle => "%Q".to_string(),
            Self::ElectronicResourceNumber => "%R".to_string(),
            Self::TertiaryTitle => "%S".to_string(),
            Self::Title => "%T".to_string(),
            Self::Url => "%U".to_string(),
            Self::Volume => "%V".to_string(),
            Self::Abstract => "%X".to_string(),
            Self::TertiaryAuthor => "%Y".to_string(),
            Self::Notes => "%Z".to_string(),
            Self::WorkType => "%9".to_string(),
            Self::NumberOfVolumes => "%6".to_string(),
            Self::Edition => "%7".to_string(),
            Self::Date => "%8".to_string(),
            Self::SubsidiaryAuthor => "%?".to_string(),
            Self::IsbnIssn => "%@".to_string(),
            Self::OriginalPublication => "%(".to_string(),
            Self::PdfLink => "%>".to_string(),
            Self::AccessDate => "%[".to_string(),
            Self::Unknown(code) => format!("%{}", code),
        }
    }

    fn is_contributor_tag(&self) -> bool {
        matches!(
            self,
            Self::Author
                | Self::Editor
                | Self::TertiaryAuthor
                | Self::SubsidiaryAuthor
                | Self::TranslatedAuthor
        )
    }
}

#[derive(Debug, Clone)]
struct RawEnwRecord {
    data: HashMap<EnwTag, Vec<String>>,
    authors: Vec<Author>,
    start_line: Option<usize>,
    record_span: Option<SourceSpan>,
}

impl RawEnwRecord {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
            authors: Vec::new(),
            start_line: None,
            record_span: None,
        }
    }

    fn add_data(&mut self, tag: EnwTag, value: String) {
        self.data.entry(tag).or_default().push(value);
    }

    fn add_author(&mut self, author: Author) {
        self.authors.push(author);
    }

    fn has_content(&self) -> bool {
        !self.data.is_empty() || !self.authors.is_empty()
    }

    fn has_started(&self) -> bool {
        self.start_line.is_some()
    }

    fn extend_span(&mut self, end: usize) {
        if let Some(ref mut span) = self.record_span {
            span.end = end;
        }
    }

    fn remove_all(&mut self, tag: &EnwTag) -> Vec<String> {
        self.data.remove(tag).unwrap_or_default()
    }

    fn take_first_non_empty(&mut self, tag: &EnwTag) -> Option<String> {
        let mut values = self.data.remove(tag)?;
        let index = values.iter().position(|value| !value.trim().is_empty())?;
        let value = values.remove(index);
        if !values.is_empty() {
            self.data.insert(tag.clone(), values);
        }
        Some(value)
    }
}

pub(crate) fn looks_like_enw(content: &str) -> bool {
    content.lines().any(is_enw_record_start)
}

pub(crate) fn parse_enw(content: &str) -> Result<Vec<Citation>, ParseError> {
    let mut records = Vec::new();
    let mut current = RawEnwRecord::new();
    let mut line_number = 0usize;
    let text_ptr = content.as_ptr() as usize;
    let mut last_tag: Option<EnwTag> = None;

    for raw_line in content.lines() {
        line_number += 1;
        let line_byte_start = raw_line.as_ptr() as usize - text_ptr;
        let line_byte_end = line_byte_start + raw_line.len();

        if raw_line.trim().is_empty() {
            continue;
        }

        if raw_line.starts_with('%') {
            let (tag, value) =
                parse_enw_line(raw_line, line_number, line_byte_start, line_byte_end)?;

            if matches!(tag, EnwTag::ReferenceType) {
                if current.has_content() {
                    records.push(current);
                    current = RawEnwRecord::new();
                }
                current.start_line = Some(line_number);
                current.record_span = Some(SourceSpan::new(line_byte_start, line_byte_end));
            } else if !current.has_started() {
                continue;
            } else {
                current.extend_span(line_byte_end);
            }

            current.add_data(tag.clone(), value.clone());
            last_tag = Some(tag.clone());

            if tag.is_contributor_tag() {
                current.add_author(parse_author(&value));
            }
        } else if current.has_started() {
            current.extend_span(line_byte_end);
            if let Some(ref tag) = last_tag
                && let Some(values) = current.data.get_mut(tag)
                && let Some(last_value) = values.last_mut()
            {
                last_value.push('\n');
                last_value.push_str(raw_line.trim());
            }
        }
    }

    if current.has_content() {
        records.push(current);
    }

    records.into_iter().map(TryInto::try_into).collect()
}

fn parse_enw_line(
    line: &str,
    line_number: usize,
    line_start: usize,
    line_end: usize,
) -> Result<(EnwTag, String), ParseError> {
    if line.len() < 2 {
        return Err(ParseError::at_line(
            line_number,
            CitationFormat::Enw,
            ValueError::Syntax(format!("ENW line too short: '{}'", line)),
        )
        .with_span(SourceSpan::new(line_start, line_end)));
    }

    let mut chars = line.chars();
    let percent = chars.next();
    let tag_char = chars.next();
    let separator = chars.next();

    if percent != Some('%') || tag_char.is_none() {
        return Err(ParseError::at_line(
            line_number,
            CitationFormat::Enw,
            ValueError::Syntax(format!("Malformed ENW tag line: '{}'", line)),
        )
        .with_span(SourceSpan::new(line_start, line_end)));
    }

    let tag_char = tag_char.expect("checked above");
    if separator != Some(' ') && separator.is_some() {
        return Err(ParseError::at_line(
            line_number,
            CitationFormat::Enw,
            ValueError::Syntax(format!(
                "Malformed ENW tag separator after %{}: '{}'",
                tag_char, line
            )),
        )
        .with_span(SourceSpan::new(line_start, line_end)));
    }

    let value = if line.len() <= 2 {
        String::new()
    } else {
        line[3..].trim().to_string()
    };

    Ok((EnwTag::from_code(tag_char), value))
}

fn is_enw_record_start(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("%0 ") || trimmed == "%0"
}

fn parse_author(author_str: &str) -> Author {
    let (family, given) = crate::utils::parse_author_name(author_str);
    let (given_name, middle_name) = if given.is_empty() {
        (None, None)
    } else {
        crate::utils::split_given_and_middle(&given)
    };

    Author {
        name: family,
        given_name,
        middle_name,
        affiliations: Vec::new(),
    }
}

impl TryFrom<RawEnwRecord> for Citation {
    type Error = ParseError;

    fn try_from(mut raw: RawEnwRecord) -> Result<Self, Self::Error> {
        let start_line = raw.start_line;
        let record_span = raw.record_span.clone();

        let mut citation_type = Vec::new();
        for value in raw.remove_all(&EnwTag::ReferenceType) {
            push_unique(&mut citation_type, value);
        }
        for value in raw.remove_all(&EnwTag::WorkType) {
            push_unique(&mut citation_type, value);
        }

        let title = raw
            .take_first_non_empty(&EnwTag::Title)
            .or_else(|| raw.take_first_non_empty(&EnwTag::TranslatedTitle))
            .unwrap_or_default();

        let journal = extract_best_container(&mut raw);
        let date = extract_date(&mut raw);
        let volume = raw.take_first_non_empty(&EnwTag::Volume);
        let issue = raw.take_first_non_empty(&EnwTag::Issue);
        let pages = raw
            .take_first_non_empty(&EnwTag::Pages)
            .map(|pages| crate::utils::format_page_numbers(&pages));
        let accession_number = raw.take_first_non_empty(&EnwTag::AccessionNumber);
        let publisher = raw.take_first_non_empty(&EnwTag::Publisher);
        let language = raw.take_first_non_empty(&EnwTag::Language);
        let keywords = raw.remove_all(&EnwTag::Keywords);
        let abstract_text = join_field_values(raw.remove_all(&EnwTag::Abstract));
        let (doi, urls) = extract_doi_and_urls(&mut raw);
        let issn = extract_isbn_issn(&mut raw);
        raw.remove_all(&EnwTag::Author);

        if title.is_empty() && raw.authors.is_empty() {
            let err = ParseError::new(
                start_line,
                None,
                CitationFormat::Enw,
                ValueError::MissingValue {
                    field: "title or author",
                    key: "title/author",
                },
            );
            return Err(if let Some(span) = record_span {
                err.with_span(span)
            } else {
                err
            });
        }

        Ok(Citation {
            citation_type,
            title,
            authors: raw.authors,
            journal,
            journal_abbr: None,
            date,
            volume,
            issue,
            pages,
            issn,
            doi,
            accession_number,
            pmid: None,
            pmc_id: None,
            abstract_text,
            keywords,
            urls,
            language,
            mesh_terms: Vec::new(),
            publisher,
            extra_fields: raw
                .data
                .drain()
                .map(|(tag, values)| (tag.as_key(), values))
                .collect(),
        })
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    let trimmed = value.trim();
    if !trimmed.is_empty() && !values.iter().any(|existing| existing == trimmed) {
        values.push(trimmed.to_string());
    }
}

fn extract_best_container(raw: &mut RawEnwRecord) -> Option<String> {
    raw.take_first_non_empty(&EnwTag::Journal)
        .or_else(|| raw.take_first_non_empty(&EnwTag::SecondaryTitle))
        .or_else(|| raw.take_first_non_empty(&EnwTag::TertiaryTitle))
}

fn extract_date(raw: &mut RawEnwRecord) -> Option<crate::Date> {
    if let Some(date_text) = raw
        .data
        .get(&EnwTag::Date)
        .and_then(|values| values.iter().find(|value| !value.trim().is_empty()))
        .cloned()
        && let Some(date) = crate::utils::parse_enw_date(&date_text)
    {
        let _ = raw.take_first_non_empty(&EnwTag::Date);
        return Some(date);
    }

    if let Some(date_text) = raw
        .data
        .get(&EnwTag::Year)
        .and_then(|values| values.iter().find(|value| !value.trim().is_empty()))
        .cloned()
        && let Some(date) = crate::utils::parse_year_only(&date_text)
    {
        let _ = raw.take_first_non_empty(&EnwTag::Year);
        return Some(date);
    }

    None
}

fn extract_doi_and_urls(raw: &mut RawEnwRecord) -> (Option<String>, Vec<String>) {
    let mut doi = None;

    let resource_values = raw.remove_all(&EnwTag::ElectronicResourceNumber);
    let mut leftovers = Vec::new();
    for value in resource_values {
        if doi.is_none()
            && let Some(candidate) = crate::utils::format_doi(&value)
        {
            doi = Some(candidate);
        } else {
            leftovers.push(value);
        }
    }
    if !leftovers.is_empty() {
        raw.data.insert(EnwTag::ElectronicResourceNumber, leftovers);
    }

    let mut urls = Vec::new();
    for tag in [EnwTag::Url, EnwTag::PdfLink] {
        for url in raw.remove_all(&tag) {
            if doi.is_none() && url.contains("doi.org") {
                doi = crate::utils::format_doi(&url);
            }
            urls.push(url);
        }
    }

    (doi, urls)
}

fn extract_isbn_issn(raw: &mut RawEnwRecord) -> Vec<String> {
    let mut identifiers = Vec::new();
    for value in raw.remove_all(&EnwTag::IsbnIssn) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }

        if looks_like_isbn(trimmed) {
            identifiers.push(trimmed.to_string());
            continue;
        }

        let split = crate::utils::split_issns(trimmed);
        if split.is_empty() {
            identifiers.push(trimmed.to_string());
        } else {
            identifiers.extend(split);
        }
    }
    identifiers
}

fn join_field_values(values: Vec<String>) -> Option<String> {
    let joined = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    (!joined.is_empty()).then_some(joined)
}

fn looks_like_isbn(value: &str) -> bool {
    let compact: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();

    match compact.len() {
        10 => compact
            .chars()
            .enumerate()
            .all(|(idx, c)| c.is_ascii_digit() || (idx == 9 && matches!(c, 'X' | 'x'))),
        13 => compact.chars().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}
