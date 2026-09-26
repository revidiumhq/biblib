//! Converts input citations into normalized `DedupRecord`s.

use super::{DedupRecord, Deduplicator, normalize};
use crate::Citation;
use crate::utils::format_doi;
use std::collections::HashSet;

impl Deduplicator {
    pub(super) fn preprocess_citations(&self, citations: &[Citation]) -> Vec<DedupRecord> {
        citations
            .iter()
            .enumerate()
            .map(|(idx, citation)| {
                let converted_title = normalize::convert_unicode_string(&citation.title);
                let norm_title = normalize::normalize_string(&converted_title);
                let mut seen_issns = HashSet::new();
                let norm_issns = citation
                    .issn
                    .iter()
                    .filter_map(|issn| normalize::format_issn(issn))
                    .filter(|issn| seen_issns.insert(issn.clone()))
                    .collect();

                DedupRecord {
                    idx,
                    alt_norm_title: normalize::strip_boilerplate_title_prefix(&converted_title)
                        .map(normalize::normalize_string)
                        .filter(|alt_title| !alt_title.is_empty() && alt_title != &norm_title),
                    has_bracketed_translation_title: Self::is_bracketed_translation_title(
                        &converted_title,
                    ),
                    norm_title,
                    norm_doi: citation.doi.as_deref().and_then(format_doi),
                    norm_journal: normalize::format_journal_name(citation.journal.as_deref())
                        .filter(|journal| !journal.is_empty()),
                    norm_journal_abbr: normalize::format_journal_name(
                        citation.journal_abbr.as_deref(),
                    )
                    .filter(|journal| !journal.is_empty()),
                    norm_issns,
                    norm_volume: citation
                        .volume
                        .as_deref()
                        .map(normalize::normalize_volume)
                        .filter(|volume| !volume.is_empty()),
                    norm_issue: citation
                        .issue
                        .as_deref()
                        .map(normalize::normalize_issue)
                        .filter(|issue| !issue.is_empty()),
                    norm_start_page: citation
                        .pages
                        .as_deref()
                        .and_then(normalize::normalize_start_page),
                    year: citation.date.as_ref().map(|date| date.year),
                    first_author_key: citation
                        .authors
                        .first()
                        .and_then(|author| normalize::normalize_author_key(&author.name)),
                }
            })
            .collect()
    }
}
