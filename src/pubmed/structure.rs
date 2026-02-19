use crate::error::{ParseError, SourceSpan, ValueError, fields};
use crate::pubmed::author::PubmedAuthor;
use crate::pubmed::tags::PubmedTag;
use crate::utils::parse_pubmed_date;
use crate::{CitationFormat, Date};
use std::collections::HashMap;

/// Structured raw data from a PubMed formatted .nbib file.
pub(crate) struct RawPubmedData {
    /// Key-value pair data from the .nbib file data.
    pub(crate) data: HashMap<PubmedTag, Vec<String>>,
    /// Authors of the cited work.
    pub(crate) authors: Vec<PubmedAuthor>,
    /// Invalid lines found in the .nbib file data, which were skipped by the parser.
    pub(crate) ignored_lines: Vec<String>,
    /// Starting line number of this citation in the source text (1-based).
    pub(crate) start_line: usize,
    /// Byte-offset span of the entire citation chunk in the source text.
    pub(crate) record_span: SourceSpan,
}

impl TryFrom<RawPubmedData> for crate::Citation {
    type Error = ParseError;
    fn try_from(
        RawPubmedData {
            mut data,
            authors,
            ignored_lines: _,
            start_line,
            record_span,
        }: RawPubmedData,
    ) -> Result<Self, Self::Error> {
        // unresolved question: what should we do if multiple values are found for
        // a field where one value is expected?
        // https://github.com/AliAzlanDev/biblib/pull/7#issuecomment-2984871452
        // current solution: join multiple values on hard-coded string " AND "
        // alternative solutions:
        let date = data
            .remove(&PubmedTag::PublicationDate)
            // multiple values ignored
            .and_then(|v| v.into_iter().next())
            .map(|v| parse_pubmed_date_err(v, start_line, &record_span))
            .transpose()?;

        Ok(Self {
            citation_type: data
                .remove(&PubmedTag::PublicationType)
                .unwrap_or_else(Vec::new),
            title: data
                .remove(&PubmedTag::Title)
                .and_then(join_if_some)
                .ok_or_else(|| {
                    ParseError::at_line(
                        start_line,
                        CitationFormat::PubMed,
                        ValueError::MissingValue {
                            field: fields::TITLE,
                            key: "TI",
                        },
                    )
                    .with_span(record_span.clone())
                })?,
            authors: authors.into_iter().map(|a| a.into()).collect(),
            journal: data
                .remove(&PubmedTag::FullJournalTitle)
                .and_then(join_if_some),
            journal_abbr: data
                .remove(&PubmedTag::JournalTitleAbbreviation)
                .and_then(join_if_some),
            date,
            volume: data.remove(&PubmedTag::Volume).and_then(join_if_some),
            issue: data.remove(&PubmedTag::Issue).and_then(join_if_some),
            pages: data.remove(&PubmedTag::Pagination).and_then(join_if_some),
            issn: data.remove(&PubmedTag::Issn).unwrap_or_else(Vec::new),
            doi: data
                .remove(&PubmedTag::LocationId)
                .unwrap_or_else(Vec::new)
                .into_iter()
                .filter_map(parse_doi_from_lid)
                .next()
                // Fallback to AID field if DOI not found in LID
                .or_else(|| {
                    data.remove(&PubmedTag::ArticleIdentifier)
                        .unwrap_or_else(Vec::new)
                        .into_iter()
                        .filter_map(parse_doi_from_lid)
                        .next()
                }),
            pmid: data
                .remove(&PubmedTag::PubmedUniqueIdentifier)
                .and_then(join_if_some),
            pmc_id: data
                .remove(&PubmedTag::PubmedCentralIdentifier)
                .and_then(join_if_some),
            abstract_text: data.remove(&PubmedTag::Abstract).and_then(join_if_some),
            keywords: Vec::new(),
            urls: Vec::new(),
            language: data.remove(&PubmedTag::Language).and_then(join_if_some),
            mesh_terms: data.remove(&PubmedTag::MeshTerms).unwrap_or_else(Vec::new),
            publisher: data.remove(&PubmedTag::Publisher).and_then(join_if_some),
            extra_fields: data
                .into_iter()
                .map(|(k, v)| (k.as_tag().to_string(), v))
                .collect(),
        })
    }
}

// FIXME when `CitationError::MultipleValues` is implemented.
// https://github.com/AliAzlanDev/biblib/pull/7#issuecomment-2989915130
fn join_if_some(v: Vec<String>) -> Option<String> {
    if v.is_empty() {
        None
    } else {
        Some(v.join(" AND "))
    }
}

/// Wraps [parse_pubmed_date] to change its types.
fn parse_pubmed_date_err<S: AsRef<str>>(date: S, start_line: usize, record_span: &SourceSpan) -> Result<Date, ParseError> {
    let s = date.as_ref();
    parse_pubmed_date(s).ok_or_else(|| {
        ParseError::at_line(
            start_line,
            CitationFormat::PubMed,
            ValueError::BadValue {
                field: fields::DATE,
                key: "DP",
                value: s.to_string(),
                reason: "not a valid date in YYYY MMM D format".to_string(),
            },
        )
        .with_span(record_span.clone())
    })
}

fn parse_doi_from_lid(s: String) -> Option<String> {
    s.strip_suffix(" [doi]").map(|s| s.to_string())
}

impl From<PubmedAuthor> for crate::Author {
    fn from(PubmedAuthor { name, affiliations }: PubmedAuthor) -> Self {
        let (given_name_opt, middle_name_opt) = name
            .given_name()
            .map(crate::utils::split_given_and_middle)
            .unwrap_or((None, None));
        Self {
            name: name.last_name().to_string(),
            given_name: given_name_opt,
            middle_name: middle_name_opt,
            affiliations,
        }
    }
}
