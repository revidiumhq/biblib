//! Pairwise match rules: title similarity, DOI sanity, metadata evidence and guards.

use super::{
    AUTHOR_GUARD_THRESHOLD, DedupRecord, Deduplicator, MetadataRelation, PASS1_DOI_TITLE_SANITY,
};
use strsim::{jaro, jaro_winkler};

impl Deduplicator {
    pub(super) fn records_match(&self, left: &DedupRecord, right: &DedupRecord) -> bool {
        let journal_match = Self::journals_match(
            &left.norm_journal,
            &left.norm_journal_abbr,
            &right.norm_journal,
            &right.norm_journal_abbr,
        );
        let issn_match = Self::match_issns(&left.norm_issns, &right.norm_issns);
        let journal_or_issn_match = journal_match || issn_match;
        let volume_match = Self::options_match(&left.norm_volume, &right.norm_volume);
        let issue_match = Self::options_match(&left.norm_issue, &right.norm_issue);
        let page_match = Self::options_match(&left.norm_start_page, &right.norm_start_page);
        let year_compatible = self.years_match(left.year, right.year);
        let same_year =
            matches!((left.year, right.year), (Some(left), Some(right)) if left == right);
        let doi_publication_match = volume_match || page_match || (issue_match && same_year);

        match (&left.norm_doi, &right.norm_doi) {
            (Some(left_doi), Some(right_doi)) if left_doi != right_doi => false,
            _ if Self::matches_bracketed_translation_metadata_path(
                left,
                right,
                journal_or_issn_match,
                volume_match,
                issue_match,
                page_match,
            ) =>
            {
                true
            }
            (Some(_), Some(_)) => {
                let metadata_match =
                    year_compatible && doi_publication_match && journal_or_issn_match;
                if !metadata_match {
                    return false;
                }

                let sim = Self::max_title_similarity(left, right, jaro);
                if !Self::passes_author_guard(left, right, sim) {
                    return false;
                }

                let matches = sim >= self.exact_title_threshold;

                if !matches {
                    return false;
                }

                if Self::fails_series_guard(
                    &left.norm_title,
                    &right.norm_title,
                    sim,
                    self.exact_title_threshold,
                    page_match,
                ) {
                    return false;
                }

                true
            }
            _ => {
                if Self::has_page_conflict(left, right) {
                    return false;
                }

                if volume_match && Self::has_issue_conflict(left, right) {
                    return false;
                }

                let publication_match = Self::no_doi_publication_match(
                    left,
                    right,
                    volume_match,
                    issue_match,
                    page_match,
                    same_year,
                );
                let standard_metadata_match =
                    year_compatible && publication_match && journal_or_issn_match;
                let exact_title_metadata_match = year_compatible && volume_match && page_match;
                if !(standard_metadata_match || exact_title_metadata_match) {
                    return false;
                }

                let sim = Self::max_title_similarity(left, right, jaro_winkler);
                if !Self::passes_author_guard(left, right, sim) {
                    return false;
                }

                let matches = (sim >= self.no_doi_title_threshold && standard_metadata_match)
                    || (sim >= self.exact_title_threshold && exact_title_metadata_match);

                if !matches {
                    return false;
                }

                if Self::fails_series_guard(
                    &left.norm_title,
                    &right.norm_title,
                    sim,
                    self.exact_title_threshold,
                    page_match,
                ) {
                    return false;
                }

                true
            }
        }
    }

    pub(super) fn years_match(&self, left: Option<i32>, right: Option<i32>) -> bool {
        match (left, right) {
            (Some(left), Some(right)) => (left - right).abs() <= i32::from(self.year_tolerance),
            _ => true,
        }
    }

    pub(super) fn pass1_same_doi_titles_match(
        &self,
        left: &DedupRecord,
        right: &DedupRecord,
    ) -> bool {
        let sim = Self::max_title_similarity(left, right, jaro);
        if sim >= PASS1_DOI_TITLE_SANITY {
            return true;
        }

        if Self::authors_conflict(left, right) {
            return false;
        }

        let journal_or_issn_match = Self::journals_match(
            &left.norm_journal,
            &left.norm_journal_abbr,
            &right.norm_journal,
            &right.norm_journal_abbr,
        ) || Self::match_issns(&left.norm_issns, &right.norm_issns);
        let page_match = Self::options_match(&left.norm_start_page, &right.norm_start_page);

        if !(journal_or_issn_match && page_match) {
            return false;
        }

        if Self::fails_series_guard(
            &left.norm_title,
            &right.norm_title,
            sim,
            self.exact_title_threshold,
            page_match,
        ) {
            return false;
        }

        true
    }

    pub(super) fn max_title_similarity(
        left: &DedupRecord,
        right: &DedupRecord,
        similarity: fn(&str, &str) -> f64,
    ) -> f64 {
        let mut max_similarity = similarity(&left.norm_title, &right.norm_title);

        if let Some(left_alt_title) = left.alt_norm_title.as_deref() {
            max_similarity = max_similarity.max(similarity(left_alt_title, &right.norm_title));
        }

        if let Some(right_alt_title) = right.alt_norm_title.as_deref() {
            max_similarity = max_similarity.max(similarity(&left.norm_title, right_alt_title));
        }

        if let (Some(left_alt_title), Some(right_alt_title)) = (
            left.alt_norm_title.as_deref(),
            right.alt_norm_title.as_deref(),
        ) {
            max_similarity = max_similarity.max(similarity(left_alt_title, right_alt_title));
        }

        max_similarity
    }

    pub(super) fn passes_author_guard(left: &DedupRecord, right: &DedupRecord, sim: f64) -> bool {
        if sim >= AUTHOR_GUARD_THRESHOLD {
            return true;
        }

        !Self::authors_conflict(left, right)
    }

    pub(super) fn authors_conflict(left: &DedupRecord, right: &DedupRecord) -> bool {
        matches!(
            (&left.first_author_key, &right.first_author_key),
            (Some(left), Some(right)) if left != right
        )
    }

    pub(super) fn matches_bracketed_translation_metadata_path(
        left: &DedupRecord,
        right: &DedupRecord,
        journal_or_issn_match: bool,
        volume_match: bool,
        issue_match: bool,
        page_match: bool,
    ) -> bool {
        (left.has_bracketed_translation_title || right.has_bracketed_translation_title)
            && journal_or_issn_match
            && matches!((left.year, right.year), (Some(left_year), Some(right_year)) if left_year == right_year)
            && volume_match
            && Self::issues_compatible(left, right, issue_match)
            && page_match
            && Self::same_first_author(left, right)
    }

    pub(super) fn issues_compatible(
        left: &DedupRecord,
        right: &DedupRecord,
        issue_match: bool,
    ) -> bool {
        issue_match || left.norm_issue.is_none() || right.norm_issue.is_none()
    }

    pub(super) fn same_first_author(left: &DedupRecord, right: &DedupRecord) -> bool {
        left.first_author_key
            .as_ref()
            .zip(right.first_author_key.as_ref())
            .is_some_and(|(left, right)| left == right)
    }

    pub(super) fn fails_series_guard(
        left_title: &str,
        right_title: &str,
        sim: f64,
        exact_title_threshold: f64,
        page_match: bool,
    ) -> bool {
        sim < exact_title_threshold
            && Self::looks_like_series_suffix_difference(left_title, right_title)
            && !page_match
    }

    pub(super) fn looks_like_series_suffix_difference(left_title: &str, right_title: &str) -> bool {
        let (left_remainder, right_remainder) =
            Self::strip_longest_common_prefix(left_title, right_title);

        (!left_remainder.is_empty() || !right_remainder.is_empty())
            && Self::is_ascii_digits_or_roman(left_remainder)
            && Self::is_ascii_digits_or_roman(right_remainder)
    }

    pub(super) fn strip_longest_common_prefix<'a>(
        left: &'a str,
        right: &'a str,
    ) -> (&'a str, &'a str) {
        let mut prefix_len = 0;
        let mut left_chars = left.chars();
        let mut right_chars = right.chars();

        loop {
            match (left_chars.next(), right_chars.next()) {
                (Some(left_char), Some(right_char)) if left_char == right_char => {
                    prefix_len += left_char.len_utf8();
                }
                _ => break,
            }
        }

        (&left[prefix_len..], &right[prefix_len..])
    }

    pub(super) fn is_ascii_digits_or_roman(value: &str) -> bool {
        value.is_empty()
            || value.chars().all(|ch| {
                ch.is_ascii_digit() || matches!(ch, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm')
            })
    }

    pub(super) fn ordered_pair(left: usize, right: usize) -> (usize, usize) {
        if left <= right {
            (left, right)
        } else {
            (right, left)
        }
    }

    pub(super) fn has_metadata_agreement(left: &DedupRecord, right: &DedupRecord) -> bool {
        Self::journals_match(
            &left.norm_journal,
            &left.norm_journal_abbr,
            &right.norm_journal,
            &right.norm_journal_abbr,
        ) || Self::match_issns(&left.norm_issns, &right.norm_issns)
            || Self::options_match(&left.norm_volume, &right.norm_volume)
            || Self::options_match(&left.norm_issue, &right.norm_issue)
            || Self::options_match(&left.norm_start_page, &right.norm_start_page)
    }

    pub(super) fn is_bracketed_translation_title(title: &str) -> bool {
        let trimmed = title.trim();
        trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && trimmed[1..trimmed.len().saturating_sub(1)]
                .trim()
                .chars()
                .any(|ch| !ch.is_whitespace())
    }

    pub(super) fn options_match(left: &Option<String>, right: &Option<String>) -> bool {
        matches!(Self::option_relation(left, right), MetadataRelation::Match)
    }

    pub(super) fn options_conflict(left: &Option<String>, right: &Option<String>) -> bool {
        matches!(
            Self::option_relation(left, right),
            MetadataRelation::Conflict
        )
    }

    pub(super) fn option_relation(
        left: &Option<String>,
        right: &Option<String>,
    ) -> MetadataRelation {
        match (left, right) {
            (Some(left), Some(right)) if left == right => MetadataRelation::Match,
            (Some(_), Some(_)) => MetadataRelation::Conflict,
            _ => MetadataRelation::Missing,
        }
    }

    pub(super) fn has_issue_conflict(left: &DedupRecord, right: &DedupRecord) -> bool {
        Self::options_conflict(&left.norm_issue, &right.norm_issue)
    }

    pub(super) fn has_page_conflict(left: &DedupRecord, right: &DedupRecord) -> bool {
        Self::options_conflict(&left.norm_start_page, &right.norm_start_page)
    }

    pub(super) fn no_doi_publication_match(
        left: &DedupRecord,
        right: &DedupRecord,
        volume_match: bool,
        issue_match: bool,
        page_match: bool,
        same_year: bool,
    ) -> bool {
        let issue_missing = matches!(
            Self::option_relation(&left.norm_issue, &right.norm_issue),
            MetadataRelation::Missing
        );
        let page_missing = matches!(
            Self::option_relation(&left.norm_start_page, &right.norm_start_page),
            MetadataRelation::Missing
        );

        page_match
            || (volume_match && issue_match)
            || (volume_match && same_year && issue_missing && page_missing)
    }

    /// Check if two journals match by comparing both full name and abbreviation.
    pub(super) fn journals_match(
        journal1: &Option<String>,
        journal_abbr1: &Option<String>,
        journal2: &Option<String>,
        journal_abbr2: &Option<String>,
    ) -> bool {
        journal1
            .as_ref()
            .zip(journal2.as_ref())
            .is_some_and(|(j1, j2)| j1 == j2)
            || journal_abbr1
                .as_ref()
                .zip(journal_abbr2.as_ref())
                .is_some_and(|(a1, a2)| a1 == a2)
            || journal1
                .as_ref()
                .zip(journal_abbr2.as_ref())
                .is_some_and(|(j1, a2)| j1 == a2)
            || journal_abbr1
                .as_ref()
                .zip(journal2.as_ref())
                .is_some_and(|(a1, j2)| a1 == j2)
    }

    pub(super) fn match_issns(left: &[String], right: &[String]) -> bool {
        left.iter()
            .any(|left_issn| right.iter().any(|right_issn| left_issn == right_issn))
    }
}
