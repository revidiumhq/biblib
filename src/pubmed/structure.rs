use crate::error::{ParseError, SourceSpan, ValueError, fields};
use crate::pubmed::author::{AuthorName, PubmedAuthor};
use crate::pubmed::tags::PubmedTag;
use crate::utils::{format_page_numbers, parse_pubmed_date_lenient};
use crate::{CitationFormat, Date};
use std::collections::HashMap;

/// Structured raw data from a PubMed formatted .nbib file.
pub(crate) struct RawPubmedData {
    /// Key-value pair data from the .nbib file data.
    pub(crate) data: HashMap<PubmedTag, Vec<String>>,
    /// Authors of the cited work.
    pub(crate) authors: Vec<PubmedAuthor>,
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
        // A book's editors stand in for its authors when it has none, as in
        // the BibTeX parser; otherwise they stay in `extra_fields`.
        let authors = if authors.is_empty() {
            editors(&mut data)
        } else {
            authors
        };
        let mut issn = data.remove(&PubmedTag::Issn).unwrap_or_default();
        issn.extend(data.remove(&PubmedTag::Isbn).unwrap_or_default());
        // A Bookshelf record's NBK ID, e.g. `AID - NBK1116 [bookaccession]`.
        let accession_number = data
            .get(&PubmedTag::ArticleIdentifier)
            .into_iter()
            .flatten()
            .find_map(|id| id.strip_suffix(" [bookaccession]"))
            .map(str::to_string);

        Ok(Self {
            citation_type: data
                .remove(&PubmedTag::PublicationType)
                .unwrap_or_else(Vec::new),
            title: data
                .remove(&PubmedTag::Title)
                .or_else(|| data.remove(&PubmedTag::BookTitle))
                .or_else(|| data.remove(&PubmedTag::TransliteratedTitle))
                .and_then(join_if_some)
                .unwrap_or_default(),
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
            // MEDLINE abbreviates page ranges ("940-5"); they are expanded as
            // in the other parsers.
            pages: data
                .remove(&PubmedTag::Pagination)
                .and_then(join_if_some)
                .map(|pages| format_page_numbers(&pages)),
            issn,
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
            accession_number,
            pmid: data
                .remove(&PubmedTag::PubmedUniqueIdentifier)
                .and_then(join_if_some),
            pmc_id: data
                .remove(&PubmedTag::PubmedCentralIdentifier)
                .and_then(join_if_some),
            abstract_text: data.remove(&PubmedTag::Abstract).and_then(join_if_some),
            keywords: data.remove(&PubmedTag::OtherTerm).unwrap_or_default(),
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

/// `FED` (full editor names), or `AU`-style `ED` when there is no `FED`.
fn editors(data: &mut HashMap<PubmedTag, Vec<String>>) -> Vec<PubmedAuthor> {
    let names: Vec<AuthorName> = match data.remove(&PubmedTag::FullEditorName) {
        Some(full) => full.into_iter().map(AuthorName::fau).collect(),
        None => data
            .remove(&PubmedTag::Editor)
            .unwrap_or_default()
            .into_iter()
            .map(AuthorName::au)
            .collect(),
    };
    if !names.is_empty() {
        data.remove(&PubmedTag::Editor);
    }
    names
        .into_iter()
        .map(|name| PubmedAuthor {
            name,
            affiliations: Vec::new(),
        })
        .collect()
}

/// Wraps [parse_pubmed_date_lenient] to change its types.
fn parse_pubmed_date_err<S: AsRef<str>>(
    date: S,
    start_line: usize,
    record_span: &SourceSpan,
) -> Result<Date, ParseError> {
    let s = date.as_ref();
    parse_pubmed_date_lenient(s).ok_or_else(|| {
        ParseError::at_line(
            start_line,
            CitationFormat::PubMed,
            ValueError::BadValue {
                field: fields::DATE,
                key: "DP",
                value: s.to_string(),
                reason: "no year in the date".to_string(),
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
