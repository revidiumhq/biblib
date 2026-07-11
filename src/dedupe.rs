//! Citations deduplicator implementation.
//!
//! The deduplicator works on slices of [`Citation`] values and returns
//! deterministic, index-based duplicate groups.
//!
//! `DuplicateGroup` stores indices into the input slice. If you want owned
//! results, use [`Deduplicator::find_duplicates_cloned`] or
//! [`Deduplicator::find_duplicates_with_sources_cloned`].
//!
//! Trailing missing sources are treated as "no source". Extra source entries are
//! ignored.
//!
//! ## Quick Start
//!
//! ```rust
//! use biblib::dedupe::Deduplicator;
//! use biblib::{Citation, Date};
//!
//! let citations = vec![
//!     Citation {
//!         title: "Machine Learning Basics".to_string(),
//!         doi: Some("10.1234/ml.2023.001".to_string()),
//!         journal: Some("Journal of Example Research".to_string()),
//!         date: Some(Date {
//!             year: 2023,
//!             month: None,
//!             day: None,
//!         }),
//!         ..Default::default()
//!     },
//!     Citation {
//!         title: "Machine Learning Basics.".to_string(),
//!         doi: Some("https://doi.org/10.1234/ML.2023.001".to_string()),
//!         journal: Some("Journal of Example Research".to_string()),
//!         date: Some(Date {
//!             year: 2023,
//!             month: None,
//!             day: None,
//!         }),
//!         ..Default::default()
//!     },
//! ];
//!
//! let groups = Deduplicator::new().find_duplicates(&citations);
//!
//! assert_eq!(groups.len(), 1);
//! assert_eq!(groups[0].unique, 0);
//! assert_eq!(groups[0].duplicates, vec![1]);
//! ```
//!
//! ## Source-Aware Selection
//!
//! ```rust
//! use biblib::dedupe::Deduplicator;
//! use biblib::Citation;
//!
//! let citations = vec![
//!     Citation {
//!         title: "Example Title".to_string(),
//!         doi: Some("10.1234/example".to_string()),
//!         ..Default::default()
//!     },
//!     Citation {
//!         title: "Example Title".to_string(),
//!         doi: Some("10.1234/example".to_string()),
//!         ..Default::default()
//!     },
//! ];
//!
//! let sources = vec!["Embase", "PubMed", "Ignored extra source"];
//!
//! let groups = Deduplicator::builder()
//!     .source_preferences(["PubMed", "Embase"])
//!     .build()
//!     .find_duplicates_with_sources(&citations, &sources);
//!
//! assert_eq!(groups[0].unique, 1);
//! assert_eq!(groups[0].duplicates, vec![0]);
//! ```
//!
//! ## Builder Options
//!
//! ```rust
//! use biblib::dedupe::Deduplicator;
//!
//! let deduplicator = Deduplicator::builder()
//!     .year_tolerance(1)
//!     .parallel(false)
//!     .source_preferences(["PubMed", "CrossRef"])
//!     .doi_title_threshold(0.85)
//!     .no_doi_title_threshold(0.93)
//!     .exact_title_threshold(0.99)
//!     .build();
//!
//! let _ = deduplicator;
//! ```
//!
//! `build()` panics when a threshold is outside `(0.0, 1.0]`, or when either
//! `doi_title_threshold` or `no_doi_title_threshold` exceeds
//! `exact_title_threshold`.
//!
//! ## Matching Algorithm
//!
//! The engine uses two passes backed by union-find clustering.
//!
//! ### Pass 1: Exact DOI clustering
//!
//! Records with the same normalized DOI are bucketed together and merged when:
//!
//! | Condition | Required |
//! | --- | --- |
//! | DOI match | Yes |
//! | Title similarity | `jaro >= 0.70` |
//!
//! If either normalized title is empty, pass 1 falls back to metadata
//! agreement on journal, ISSN, volume, or normalized start page.
//!
//! ### Pass 2: Blocked fuzzy matching
//!
//! Records with non-empty normalized titles are compared in year-based blocks.
//! Unknown-year records are compared with the unknown-year block and each
//! concrete year block.
//!
//! When both records have DOIs:
//!
//! | Condition | Required |
//! | --- | --- |
//! | Similarity algorithm | `jaro` |
//! | Title similarity | `>= doi_title_threshold` |
//! | Year compatibility | Yes |
//! | Volume or page match | Yes |
//! | Journal or ISSN match | Yes |
//!
//! When at least one DOI is missing:
//!
//! | Path | Required |
//! | --- | --- |
//! | Standard fuzzy path | `jaro_winkler >= no_doi_title_threshold` and year compatible and `(volume or page)` and `(journal or ISSN)` |
//! | Exact-title fallback | `jaro_winkler >= exact_title_threshold` and year compatible and volume match and page match |
//!
//! Additional guards:
//!
//! - Series and erratum suffixes such as `part 1` / `part 2` require matching
//!   start pages when the title match is below `exact_title_threshold`.
//! - Different first-author surnames block borderline matches below the internal
//!   author-guard threshold.
//!
//! ## Normalization
//!
//! Deduplication defensively normalizes user-constructed citations before
//! matching:
//!
//! - DOI values go through [`format_doi`]
//! - Page ranges go through [`format_page_numbers`], then compare on normalized
//!   start page
//! - Journal names and abbreviations normalize empty results to `None`
//! - Titles are lowercased, HTML-cleaned, Unicode-normalized with NFKD, stripped
//!   of combining marks, and reduced to alphanumerics
//!
//! ## Determinism
//!
//! Output ordering is stable:
//!
//! - Every input index appears in exactly one group
//! - Groups are sorted by their smallest member index
//! - Each `duplicates` list is sorted ascending
//! - Parallel matching produces the same final groups as sequential matching

use crate::Citation;
use crate::regex::Regex;
use crate::utils::{format_doi, format_page_numbers};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;
use strsim::jaro;
use strsim::jaro_winkler;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

const DEFAULT_BOTH_DOI_TITLE_THRESHOLD: f64 = 0.85;
const NO_DOI_TITLE_SIMILARITY_THRESHOLD: f64 = 0.93;
const PASS1_DOI_TITLE_SANITY: f64 = 0.70;
const AUTHOR_GUARD_THRESHOLD: f64 = 0.96;

static UNICODE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<U\+([0-9A-Fa-f]+)>").unwrap());

const HTML_REPLACEMENTS: [(&str, &str); 12] = [
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("<sup>", ""),
    ("</sup>", ""),
    ("<sub>", ""),
    ("</sub>", ""),
    ("<inf>", ""),
    ("</inf>", ""),
    ("\u{03B2}", "b"),
    ("\u{03B1}", "a"),
    ("\u{00DF}", "b"),
    ("\u{03B3}", "g"),
];

/// Represents a group of duplicate citations using indices into the input slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateGroup {
    /// Index into the input slice of the citation selected as unique.
    pub unique: usize,
    /// Indices of the duplicates of `unique`, sorted ascending.
    pub duplicates: Vec<usize>,
}

/// Represents a group of duplicate citations as owned `Citation` values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedDuplicateGroup {
    /// The unique citation selected from the duplicate group.
    pub unique: Citation,
    /// The duplicate citations corresponding to `unique`.
    pub duplicates: Vec<Citation>,
}

/// Builder for configuring a [`Deduplicator`].
#[derive(Debug, Clone)]
pub struct DeduplicatorBuilder {
    year_tolerance: u8,
    parallel: bool,
    source_preferences: Vec<String>,
    doi_title_threshold: f64,
    no_doi_title_threshold: f64,
    exact_title_threshold: f64,
}

/// Core deduplication engine for finding duplicate citations.
///
/// `Deduplicator` runs a two-pass union-find algorithm:
///
/// 1. Pass 1 clusters records with the same normalized DOI using a fixed
///    sanity threshold or metadata fallback for empty titles.
/// 2. Pass 2 performs blocked fuzzy matching across year-derived groups using
///    the builder's configured thresholds.
///
/// See the module-level documentation for the full matching criteria,
/// normalization rules, and determinism guarantees.
///
/// # Example
///
/// ```
/// use biblib::dedupe::Deduplicator;
///
/// let deduplicator = Deduplicator::builder()
///     .parallel(true)
///     .source_preferences(["PubMed", "Embase"])
///     .build();
/// ```
///
#[derive(Debug, Clone)]
pub struct Deduplicator {
    year_tolerance: u8,
    parallel: bool,
    source_preferences: Vec<String>,
    doi_title_threshold: f64,
    no_doi_title_threshold: f64,
    exact_title_threshold: f64,
}

#[derive(Debug, Clone)]
struct DedupRecord {
    idx: usize,
    norm_title: String,
    norm_doi: Option<String>,
    norm_journal: Option<String>,
    norm_journal_abbr: Option<String>,
    norm_issns: Vec<String>,
    norm_volume: Option<String>,
    norm_start_page: Option<String>,
    year: Option<i32>,
    first_author_key: Option<String>,
}

#[derive(Debug, Clone)]
enum BlockTask {
    Within(Vec<usize>),
    Cross(Vec<usize>, Vec<usize>),
}

#[derive(Debug, Clone)]
struct UnionFind {
    parents: Vec<usize>,
    ranks: Vec<u8>,
}

impl Default for DeduplicatorBuilder {
    fn default() -> Self {
        Self {
            year_tolerance: 1,
            parallel: false,
            source_preferences: Vec::new(),
            doi_title_threshold: DEFAULT_BOTH_DOI_TITLE_THRESHOLD,
            no_doi_title_threshold: NO_DOI_TITLE_SIMILARITY_THRESHOLD,
            exact_title_threshold: 0.99,
        }
    }
}

impl DeduplicatorBuilder {
    #[must_use]
    pub fn year_tolerance(mut self, year_tolerance: u8) -> Self {
        self.year_tolerance = year_tolerance;
        self
    }

    #[must_use]
    pub fn parallel(mut self, parallel: bool) -> Self {
        self.parallel = parallel;
        self
    }

    #[must_use]
    pub fn source_preferences<I, S>(mut self, source_preferences: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.source_preferences = source_preferences.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn doi_title_threshold(mut self, doi_title_threshold: f64) -> Self {
        self.doi_title_threshold = doi_title_threshold;
        self
    }

    #[must_use]
    pub fn no_doi_title_threshold(mut self, no_doi_title_threshold: f64) -> Self {
        self.no_doi_title_threshold = no_doi_title_threshold;
        self
    }

    #[must_use]
    pub fn exact_title_threshold(mut self, exact_title_threshold: f64) -> Self {
        self.exact_title_threshold = exact_title_threshold;
        self
    }

    #[must_use]
    pub fn build(self) -> Deduplicator {
        Self::validate_threshold("doi_title_threshold", self.doi_title_threshold);
        Self::validate_threshold("no_doi_title_threshold", self.no_doi_title_threshold);
        Self::validate_threshold("exact_title_threshold", self.exact_title_threshold);

        assert!(
            self.doi_title_threshold <= self.exact_title_threshold,
            "DeduplicatorBuilder::build(): doi_title_threshold must be less than or equal to exact_title_threshold"
        );
        assert!(
            self.no_doi_title_threshold <= self.exact_title_threshold,
            "DeduplicatorBuilder::build(): no_doi_title_threshold must be less than or equal to exact_title_threshold"
        );

        Deduplicator {
            year_tolerance: self.year_tolerance,
            parallel: self.parallel,
            source_preferences: self.source_preferences,
            doi_title_threshold: self.doi_title_threshold,
            no_doi_title_threshold: self.no_doi_title_threshold,
            exact_title_threshold: self.exact_title_threshold,
        }
    }

    fn validate_threshold(name: &str, value: f64) {
        assert!(
            (0.0..=1.0).contains(&value) && value > 0.0,
            "DeduplicatorBuilder::build(): {name} must be within (0.0, 1.0]"
        );
    }
}

impl Default for Deduplicator {
    fn default() -> Self {
        Self::new()
    }
}

impl Deduplicator {
    /// Creates a new builder for configuring a deduplicator.
    #[must_use]
    pub fn builder() -> DeduplicatorBuilder {
        DeduplicatorBuilder::default()
    }

    /// Creates a new Deduplicator with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Processes a list of citations and returns groups of duplicates.
    ///
    /// This method analyzes the provided citations and groups them based on
    /// similarity criteria including DOIs, titles, and other metadata.
    /// One citation in each group is designated as the unique (original) citation.
    ///
    /// # Arguments
    ///
    /// * `citations` - A slice of Citation objects to be analyzed
    ///
    /// # Returns
    ///
    /// Returns a vector of `DuplicateGroup`s, where each group contains
    /// one unique citation and its identified duplicates.
    ///
    /// # Examples
    ///
    /// ```
    /// use biblib::{dedupe::Deduplicator, Citation};
    ///
    /// let citations = vec![
    ///     Citation {
    ///         title: "Example Title".to_string(),
    ///         doi: Some("10.1234/example".to_string()),
    ///         ..Default::default()
    ///     },
    ///     // ... more citations ...
    /// ];
    ///
    /// let deduplicator = Deduplicator::new();
    /// let duplicate_groups = deduplicator.find_duplicates(&citations);
    /// ```
    pub fn find_duplicates(&self, citations: &[Citation]) -> Vec<DuplicateGroup> {
        self.find_duplicates_with_sources(citations, &[])
    }

    /// Processes citations with their source information and returns groups of duplicates.
    ///
    /// This method is similar to `find_duplicates` but allows you to specify source
    /// information for each citation, enabling source-based preferences during deduplication.
    /// Citations without corresponding source entries are treated as having no source.
    ///
    /// # Arguments
    ///
    /// * `citations` - A slice of Citation objects to be analyzed
    /// * `sources` - A slice of source names corresponding to each citation.
    ///   If shorter than citations, remaining citations have no source.
    ///
    /// # Returns
    ///
    /// Returns a vector of `DuplicateGroup`s, where each group contains
    /// one unique citation and its identified duplicates.
    ///
    /// # Examples
    ///
    /// ```
    /// use biblib::{dedupe::Deduplicator, Citation};
    ///
    /// let citations = vec![
    ///     Citation {
    ///         title: "Example Title".to_string(),
    ///         doi: Some("10.1234/example".to_string()),
    ///         ..Default::default()
    ///     },
    ///     Citation {
    ///         title: "Example Title".to_string(),
    ///         doi: Some("10.1234/example".to_string()),
    ///         ..Default::default()
    ///     },
    /// ];
    ///
    /// let sources = vec!["PubMed", "CrossRef"];
    ///
    /// let deduplicator = Deduplicator::new();
    /// let duplicate_groups = deduplicator.find_duplicates_with_sources(&citations, &sources);
    /// ```
    pub fn find_duplicates_with_sources(
        &self,
        citations: &[Citation],
        sources: &[&str],
    ) -> Vec<DuplicateGroup> {
        if citations.is_empty() {
            return Vec::new();
        }

        let records = self.preprocess_citations(citations);
        let mut union_find = UnionFind::new(citations.len());

        self.run_pass1_doi_clustering(&records, &mut union_find);

        let pass1_roots = union_find.snapshot_roots();
        let pass2_matches = self.collect_pass2_matches(&records, &pass1_roots);
        for (left, right) in pass2_matches {
            union_find.union(left, right);
        }

        let mut duplicate_groups = self.assemble_groups(citations, sources, &mut union_find);
        Self::sort_duplicate_groups(&mut duplicate_groups);
        duplicate_groups
    }

    /// Processes a list of citations and returns owned groups of duplicates.
    pub fn find_duplicates_cloned(&self, citations: &[Citation]) -> Vec<OwnedDuplicateGroup> {
        self.find_duplicates_with_sources_cloned(citations, &[])
    }

    /// Processes citations with their source information and returns owned groups.
    pub fn find_duplicates_with_sources_cloned(
        &self,
        citations: &[Citation],
        sources: &[&str],
    ) -> Vec<OwnedDuplicateGroup> {
        self.find_duplicates_with_sources(citations, sources)
            .into_iter()
            .map(|group| OwnedDuplicateGroup {
                unique: citations[group.unique].clone(),
                duplicates: group
                    .duplicates
                    .into_iter()
                    .map(|idx| citations[idx].clone())
                    .collect(),
            })
            .collect()
    }

    fn preprocess_citations(&self, citations: &[Citation]) -> Vec<DedupRecord> {
        citations
            .iter()
            .enumerate()
            .map(|(idx, citation)| {
                let mut seen_issns = HashSet::new();
                let norm_issns = citation
                    .issn
                    .iter()
                    .filter_map(|issn| Self::format_issn(issn))
                    .filter(|issn| seen_issns.insert(issn.clone()))
                    .collect();

                DedupRecord {
                    idx,
                    norm_title: Self::normalize_string(&Self::convert_unicode_string(
                        &citation.title,
                    )),
                    norm_doi: citation.doi.as_deref().and_then(format_doi),
                    norm_journal: Self::format_journal_name(citation.journal.as_deref())
                        .filter(|journal| !journal.is_empty()),
                    norm_journal_abbr: Self::format_journal_name(citation.journal_abbr.as_deref())
                        .filter(|journal| !journal.is_empty()),
                    norm_issns,
                    norm_volume: citation
                        .volume
                        .as_deref()
                        .map(Self::normalize_volume)
                        .filter(|volume| !volume.is_empty()),
                    norm_start_page: citation
                        .pages
                        .as_deref()
                        .and_then(Self::normalize_start_page),
                    year: citation.date.as_ref().map(|date| date.year),
                    first_author_key: citation
                        .authors
                        .first()
                        .and_then(|author| Self::normalize_author_key(&author.name)),
                }
            })
            .collect()
    }

    fn run_pass1_doi_clustering(&self, records: &[DedupRecord], union_find: &mut UnionFind) {
        let mut doi_buckets: HashMap<String, Vec<usize>> = HashMap::new();
        for record in records {
            if let Some(norm_doi) = &record.norm_doi {
                doi_buckets
                    .entry(norm_doi.clone())
                    .or_default()
                    .push(record.idx);
            }
        }

        // TODO: extend to PMID / accession_number keys.
        for bucket in doi_buckets.values() {
            for left_idx in 0..bucket.len() {
                for right_idx in (left_idx + 1)..bucket.len() {
                    let left = &records[bucket[left_idx]];
                    let right = &records[bucket[right_idx]];

                    let title_guard = if left.norm_title.is_empty() || right.norm_title.is_empty() {
                        Self::has_metadata_agreement(left, right)
                    } else {
                        jaro(&left.norm_title, &right.norm_title) >= PASS1_DOI_TITLE_SANITY
                    };

                    if title_guard {
                        union_find.union(left.idx, right.idx);
                    }
                }
            }
        }
    }

    fn collect_pass2_matches(
        &self,
        records: &[DedupRecord],
        pass1_roots: &[usize],
    ) -> Vec<(usize, usize)> {
        let tasks = self.build_pass2_block_tasks(records);
        let mut matches = if self.parallel {
            use rayon::prelude::*;

            tasks
                .par_iter()
                .flat_map_iter(|task| self.evaluate_block_task(task, records, pass1_roots))
                .collect::<Vec<_>>()
        } else {
            let mut matches = Vec::new();
            for task in &tasks {
                matches.extend(self.evaluate_block_task(task, records, pass1_roots));
            }
            matches
        };

        matches.sort_unstable();
        matches.dedup();
        matches
    }

    fn build_pass2_block_tasks(&self, records: &[DedupRecord]) -> Vec<BlockTask> {
        let mut year_blocks: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
        let mut unknown_year_block = Vec::new();

        for record in records
            .iter()
            .filter(|record| !record.norm_title.is_empty())
        {
            match record.year {
                Some(year) => {
                    let max_year = year.saturating_add(i32::from(self.year_tolerance));
                    for block_year in year..=max_year {
                        year_blocks.entry(block_year).or_default().push(record.idx);
                    }
                }
                None => unknown_year_block.push(record.idx),
            }
        }

        let mut tasks = Vec::new();
        for members in year_blocks.values_mut() {
            members.sort_unstable();
            members.dedup();
            if members.len() > 1 {
                // TODO: sorted-token fingerprint sub-blocking for very large year blocks.
                // The unknown-year block is compared against every year block: O(u x n) if a
                // source systematically lacks dates.
                tasks.push(BlockTask::Within(members.clone()));
            }
        }

        unknown_year_block.sort_unstable();
        unknown_year_block.dedup();
        if unknown_year_block.len() > 1 {
            tasks.push(BlockTask::Within(unknown_year_block.clone()));
        }

        if !unknown_year_block.is_empty() {
            for members in year_blocks.values() {
                if !members.is_empty() {
                    tasks.push(BlockTask::Cross(
                        unknown_year_block.clone(),
                        members.clone(),
                    ));
                }
            }
        }

        tasks
    }

    fn evaluate_block_task(
        &self,
        task: &BlockTask,
        records: &[DedupRecord],
        pass1_roots: &[usize],
    ) -> Vec<(usize, usize)> {
        let mut matches = Vec::new();

        match task {
            BlockTask::Within(members) => {
                for left_pos in 0..members.len() {
                    for right_pos in (left_pos + 1)..members.len() {
                        let left = members[left_pos];
                        let right = members[right_pos];
                        if pass1_roots[left] == pass1_roots[right] {
                            continue;
                        }

                        if self.records_match(&records[left], &records[right]) {
                            matches.push((left, right));
                        }
                    }
                }
            }
            BlockTask::Cross(left_members, right_members) => {
                for &left in left_members {
                    for &right in right_members {
                        if left == right {
                            continue;
                        }

                        let pair = Self::ordered_pair(left, right);
                        if pass1_roots[pair.0] == pass1_roots[pair.1] {
                            continue;
                        }

                        if self.records_match(&records[pair.0], &records[pair.1]) {
                            matches.push(pair);
                        }
                    }
                }
            }
        }

        matches
    }

    fn records_match(&self, left: &DedupRecord, right: &DedupRecord) -> bool {
        let journal_match = Self::journals_match(
            &left.norm_journal,
            &left.norm_journal_abbr,
            &right.norm_journal,
            &right.norm_journal_abbr,
        );
        let issn_match = Self::match_issns(&left.norm_issns, &right.norm_issns);
        let volume_match = Self::options_match(&left.norm_volume, &right.norm_volume);
        let page_match = Self::options_match(&left.norm_start_page, &right.norm_start_page);
        let year_compatible = self.years_match(left.year, right.year);

        match (&left.norm_doi, &right.norm_doi) {
            (Some(_), Some(_)) => {
                let sim = jaro(&left.norm_title, &right.norm_title);
                if !Self::passes_author_guard(left, right, sim) {
                    return false;
                }

                sim >= self.doi_title_threshold
                    && year_compatible
                    && (volume_match || page_match)
                    && (journal_match || issn_match)
            }
            _ => {
                let sim = jaro_winkler(&left.norm_title, &right.norm_title);
                if !Self::passes_author_guard(left, right, sim) {
                    return false;
                }

                let matches = (sim >= self.no_doi_title_threshold
                    && year_compatible
                    && (volume_match || page_match)
                    && (journal_match || issn_match))
                    || (sim >= self.exact_title_threshold
                        && year_compatible
                        && volume_match
                        && page_match);

                if !matches {
                    return false;
                }

                if sim < self.exact_title_threshold
                    && Self::looks_like_series_suffix_difference(
                        &left.norm_title,
                        &right.norm_title,
                    )
                    && !page_match
                {
                    return false;
                }

                true
            }
        }
    }

    fn assemble_groups(
        &self,
        citations: &[Citation],
        sources: &[&str],
        union_find: &mut UnionFind,
    ) -> Vec<DuplicateGroup> {
        let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
        for idx in 0..citations.len() {
            let root = union_find.find(idx);
            components.entry(root).or_default().push(idx);
        }

        components
            .into_values()
            .map(|mut members| {
                members.sort_unstable();
                let unique = self.select_unique_citation_index(citations, sources, &members);
                let duplicates = members
                    .into_iter()
                    .filter(|&idx| idx != unique)
                    .collect::<Vec<_>>();

                DuplicateGroup { unique, duplicates }
            })
            .collect()
    }

    fn years_match(&self, left: Option<i32>, right: Option<i32>) -> bool {
        match (left, right) {
            (Some(left), Some(right)) => (left - right).abs() <= i32::from(self.year_tolerance),
            _ => true,
        }
    }

    fn select_unique_citation_index(
        &self,
        citations: &[Citation],
        sources: &[&str],
        group_indices: &[usize],
    ) -> usize {
        if group_indices.len() == 1 {
            return group_indices[0];
        }

        for preferred_source in &self.source_preferences {
            for &idx in group_indices {
                if sources.get(idx).copied() == Some(preferred_source.as_str()) {
                    return idx;
                }
            }
        }

        let citations_with_abstract = group_indices
            .iter()
            .copied()
            .filter(|&idx| {
                citations[idx]
                    .abstract_text
                    .as_deref()
                    .is_some_and(|abstract_text| !abstract_text.trim().is_empty())
            })
            .collect::<Vec<_>>();

        if citations_with_abstract.is_empty() {
            return group_indices[0];
        }

        citations_with_abstract
            .iter()
            .copied()
            .find(|&idx| {
                citations[idx]
                    .doi
                    .as_deref()
                    .is_some_and(|doi| !doi.trim().is_empty())
            })
            .unwrap_or(citations_with_abstract[0])
    }

    fn sort_duplicate_groups(duplicate_groups: &mut [DuplicateGroup]) {
        duplicate_groups.sort_by_key(|group| {
            group
                .duplicates
                .iter()
                .copied()
                .chain(std::iter::once(group.unique))
                .min()
                .unwrap_or(group.unique)
        });
    }

    fn convert_unicode_string(input: &str) -> String {
        UNICODE_REGEX
            .replace_all(input, |caps: &crate::regex::Captures| {
                u32::from_str_radix(&caps[1], 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| caps[0].to_string())
            })
            .to_string()
    }

    fn normalize_string(string: &str) -> String {
        if string.is_empty() {
            return String::new();
        }

        let mut normalized = string.trim().to_lowercase();
        for (needle, replacement) in HTML_REPLACEMENTS {
            normalized = normalized.replace(needle, replacement);
        }

        normalized
            .nfkd()
            .filter(|ch| !is_combining_mark(*ch))
            .filter(|ch| ch.is_alphanumeric())
            .collect()
    }

    fn normalize_volume(volume: &str) -> String {
        if volume.is_empty() {
            return String::new();
        }

        let numbers: String = volume
            .chars()
            .skip_while(|c| !c.is_numeric())
            .take_while(|c| c.is_numeric())
            .collect();

        if numbers.is_empty() {
            String::new()
        } else {
            numbers
        }
    }

    fn normalize_start_page(pages: &str) -> Option<String> {
        let formatted = format_page_numbers(pages);
        let start_page = formatted
            .split('-')
            .next()
            .unwrap_or("")
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();

        (!start_page.is_empty()).then_some(start_page)
    }

    fn normalize_author_key(author_name: &str) -> Option<String> {
        let normalized = author_name
            .trim()
            .to_lowercase()
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .collect::<String>();

        (!normalized.is_empty()).then_some(normalized)
    }

    fn passes_author_guard(left: &DedupRecord, right: &DedupRecord, sim: f64) -> bool {
        if sim >= AUTHOR_GUARD_THRESHOLD {
            return true;
        }

        match (&left.first_author_key, &right.first_author_key) {
            (Some(left), Some(right)) => left == right,
            _ => true,
        }
    }

    fn looks_like_series_suffix_difference(left_title: &str, right_title: &str) -> bool {
        let (left_remainder, right_remainder) =
            Self::strip_longest_common_prefix(left_title, right_title);

        (!left_remainder.is_empty() || !right_remainder.is_empty())
            && Self::is_ascii_digits_or_roman(left_remainder)
            && Self::is_ascii_digits_or_roman(right_remainder)
    }

    fn strip_longest_common_prefix<'a>(left: &'a str, right: &'a str) -> (&'a str, &'a str) {
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

    fn is_ascii_digits_or_roman(value: &str) -> bool {
        value.is_empty()
            || value.chars().all(|ch| {
                ch.is_ascii_digit() || matches!(ch, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm')
            })
    }

    fn ordered_pair(left: usize, right: usize) -> (usize, usize) {
        if left <= right {
            (left, right)
        } else {
            (right, left)
        }
    }

    fn has_metadata_agreement(left: &DedupRecord, right: &DedupRecord) -> bool {
        Self::journals_match(
            &left.norm_journal,
            &left.norm_journal_abbr,
            &right.norm_journal,
            &right.norm_journal_abbr,
        ) || Self::match_issns(&left.norm_issns, &right.norm_issns)
            || Self::options_match(&left.norm_volume, &right.norm_volume)
            || Self::options_match(&left.norm_start_page, &right.norm_start_page)
    }

    fn options_match(left: &Option<String>, right: &Option<String>) -> bool {
        left.as_ref()
            .zip(right.as_ref())
            .is_some_and(|(left, right)| left == right)
    }

    /// Check if two journals match by comparing both full name and abbreviation.
    fn journals_match(
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

    fn format_journal_name(full_name: Option<&str>) -> Option<String> {
        full_name.map(|name| {
            name.split(". Conference")
                .next()
                .unwrap_or(name)
                .trim()
                .to_lowercase()
                .chars()
                .filter(|ch| ch.is_alphanumeric())
                .collect::<String>()
        })
    }

    fn format_issn(issn_str: &str) -> Option<String> {
        let clean_issn = issn_str
            .trim()
            .to_uppercase()
            .replace("(ELECTRONIC)", "")
            .replace("(LINKING)", "")
            .replace("(PRINT)", "")
            .replace(
                |ch: char| !ch.is_ascii_digit() && ch != '-' && ch != 'X',
                "",
            )
            .trim()
            .to_string();

        let digits: String = clean_issn
            .chars()
            .filter(|ch| ch.is_ascii_digit() || *ch == 'X')
            .collect();

        if digits[..digits.len().saturating_sub(1)].contains('X') {
            return None;
        }

        match (clean_issn.len(), digits.len()) {
            (9, 8) if clean_issn.chars().nth(4) == Some('-') => Some(clean_issn),
            (8, 8) => Some(format!("{}-{}", &digits[..4], &digits[4..])),
            _ => None,
        }
    }

    fn match_issns(left: &[String], right: &[String]) -> bool {
        left.iter()
            .any(|left_issn| right.iter().any(|right_issn| left_issn == right_issn))
    }
}

impl UnionFind {
    fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
            ranks: vec![0; len],
        }
    }

    fn find(&mut self, idx: usize) -> usize {
        if self.parents[idx] != idx {
            let parent = self.parents[idx];
            self.parents[idx] = self.find(parent);
        }
        self.parents[idx]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left_root = self.find(left);
        let right_root = self.find(right);

        if left_root == right_root {
            return;
        }

        match self.ranks[left_root].cmp(&self.ranks[right_root]) {
            std::cmp::Ordering::Less => self.parents[left_root] = right_root,
            std::cmp::Ordering::Greater => self.parents[right_root] = left_root,
            std::cmp::Ordering::Equal => {
                self.parents[right_root] = left_root;
                self.ranks[left_root] += 1;
            }
        }
    }

    fn snapshot_roots(&mut self) -> Vec<usize> {
        (0..self.parents.len()).map(|idx| self.find(idx)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Author, Date};

    fn make_citation(
        title: &str,
        year: Option<i32>,
        journal: Option<&str>,
        volume: Option<&str>,
        pages: Option<&str>,
        doi: Option<&str>,
    ) -> Citation {
        Citation {
            title: title.to_string(),
            journal: journal.map(str::to_string),
            date: year.map(|year| Date {
                year,
                month: None,
                day: None,
            }),
            volume: volume.map(str::to_string),
            pages: pages.map(str::to_string),
            doi: doi.map(str::to_string),
            ..Default::default()
        }
    }

    fn make_citation_with_author(
        title: &str,
        author_name: &str,
        year: Option<i32>,
        journal: Option<&str>,
        volume: Option<&str>,
        pages: Option<&str>,
    ) -> Citation {
        let mut citation = make_citation(title, year, journal, volume, pages, None);
        citation.authors.push(Author {
            name: author_name.to_string(),
            given_name: None,
            middle_name: None,
            affiliations: Vec::new(),
        });
        citation
    }

    fn group_members(group: &DuplicateGroup) -> Vec<usize> {
        let mut members = vec![group.unique];
        members.extend(group.duplicates.iter().copied());
        members.sort_unstable();
        members
    }

    fn covered_indices(groups: &[DuplicateGroup]) -> Vec<usize> {
        let mut covered = groups.iter().flat_map(group_members).collect::<Vec<_>>();
        covered.sort_unstable();
        covered
    }

    fn canonicalize_groups(groups: &[DuplicateGroup], index_map: &[usize]) -> Vec<Vec<usize>> {
        let mut canonical = groups
            .iter()
            .map(|group| {
                let mut members = group_members(group)
                    .into_iter()
                    .map(|idx| index_map[idx])
                    .collect::<Vec<_>>();
                members.sort_unstable();
                members
            })
            .collect::<Vec<_>>();
        canonical.sort();
        canonical
    }

    #[test]
    fn test_group_by_year() {
        let citations = vec![
            Citation {
                title: "Title 1".to_string(),
                authors: vec![],
                journal: None,
                journal_abbr: None,
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                volume: None,
                abstract_text: None,
                doi: None,
                ..Default::default()
            },
            Citation {
                title: "Title 2".to_string(),
                authors: vec![],
                journal: None,
                journal_abbr: None,
                date: None,
                volume: None,
                abstract_text: None,
                doi: None,
                ..Default::default()
            },
        ];

        let deduplicator = Deduplicator::new();
        let records = deduplicator.preprocess_citations(&citations);
        let tasks = deduplicator.build_pass2_block_tasks(&records);

        assert!(tasks.iter().any(|task| matches!(
            task,
            BlockTask::Cross(left, right)
                if left.as_slice() == [1] && right.as_slice() == [0]
        )));
    }

    #[test]
    fn test_find_duplicates() {
        let citations = vec![
            Citation {
                title: "Title 1".to_string(),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                doi: Some("10.1234/abc".to_string()),
                journal: Some("Journal 1".to_string()),
                ..Default::default()
            },
            Citation {
                title: "Title 1".to_string(),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                doi: Some("10.1234/abc".to_string()),
                journal: Some("Journal 1".to_string()),
                ..Default::default()
            },
            Citation {
                title: "Title 2".to_string(),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                doi: Some("10.1234/def".to_string()),
                journal: Some("Journal 2".to_string()),
                ..Default::default()
            },
        ];

        let deduplicator = Deduplicator::new();
        let duplicate_groups = deduplicator.find_duplicates(&citations);

        assert_eq!(duplicate_groups.len(), 2);
        assert_eq!(
            duplicate_groups
                .iter()
                .find(|g| citations[g.unique].doi.as_deref() == Some("10.1234/abc"))
                .unwrap()
                .duplicates
                .len(),
            1
        );
    }

    #[test]
    fn test_missing_doi() {
        let citations = vec![
            Citation {
                title: "Title 1".to_string(),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                doi: Some("10.1234/abc".to_string()),
                journal: Some("Journal 1".to_string()),
                volume: Some("24".to_string()),
                ..Default::default()
            },
            Citation {
                title: "Title 1".to_string(),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                doi: Some("".to_string()),
                journal: Some("Journal 1".to_string()),
                volume: Some("24".to_string()),
                ..Default::default()
            },
            Citation {
                title: "Title 2".to_string(),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                doi: Some("".to_string()),
                journal: Some("Journal 2".to_string()),
                ..Default::default()
            },
        ];

        let deduplicator = Deduplicator::new();
        let duplicate_groups = deduplicator.find_duplicates(&citations);

        assert_eq!(duplicate_groups.len(), 2);
    }

    #[test]
    fn test_normalize_string() {
        assert_eq!(
            Deduplicator::normalize_string("Machine Learning! (2<sup>nd</sup> Edition)"),
            "machinelearning2ndedition".to_string()
        );
        assert_eq!(
            Deduplicator::normalize_string("[&lt;sup&gt;11&lt;/sup&gt;C] benzo"),
            "11cbenzo".to_string()
        );
    }

    #[test]
    fn test_convert_unicode_string() {
        // Test basic conversion
        assert_eq!(
            Deduplicator::convert_unicode_string("2<U+0391>-amino-4<U+0391>"),
            "2\u{0391}-amino-4\u{0391}",
            "Failed to convert basic Alpha Unicode sequences"
        );

        // Test multiple different Unicode sequences
        assert_eq!(
            Deduplicator::convert_unicode_string("Hello <U+03A9>orld <U+03A3>cience"),
            "Hello \u{03A9}orld \u{03A3}cience",
            "Failed to convert multiple Unicode sequences"
        );

        // Test string with no Unicode sequences
        assert_eq!(
            Deduplicator::convert_unicode_string("Normal String"),
            "Normal String",
            "Incorrectly modified string with no Unicode sequences"
        );

        // Test empty string
        assert_eq!(
            Deduplicator::convert_unicode_string(""),
            "",
            "Failed to handle empty string"
        );

        // Test mixed content
        assert_eq!(
            Deduplicator::convert_unicode_string("Mixed <U+0394> Unicode <U+03A9> Test"),
            "Mixed \u{0394} Unicode \u{03A9} Test",
            "Failed to handle mixed content with Unicode sequences"
        );

        // Test consecutive Unicode sequences
        assert_eq!(
            Deduplicator::convert_unicode_string("<U+0391><U+0392><U+0393>"),
            "\u{0391}\u{0392}\u{0393}",
            "Failed to convert consecutive Unicode sequences"
        );
    }

    #[test]
    fn test_normalize_volume() {
        assert_eq!(Deduplicator::normalize_volume("61"), "61");
        assert_eq!(Deduplicator::normalize_volume("61 (Supplement 1)"), "61");
        assert_eq!(Deduplicator::normalize_volume("9 (8) (no pagination)"), "9");
        assert_eq!(Deduplicator::normalize_volume("3)"), "3");
        assert_eq!(Deduplicator::normalize_volume("Part A. 242"), "242");
        assert_eq!(Deduplicator::normalize_volume("55 (10 SUPPL 1)"), "55");
        assert_eq!(Deduplicator::normalize_volume("161A"), "161");
        assert_eq!(Deduplicator::normalize_volume("74 Suppl 1"), "74");
        assert_eq!(Deduplicator::normalize_volume("20 (2)"), "20");
        assert_eq!(
            Deduplicator::normalize_volume("9 (FEB) (no pagination)"),
            "9"
        );
    }

    #[test]
    fn test_format_journal_name() {
        assert_eq!(
            Deduplicator::format_journal_name(Some(
                "Heart. Conference: British Atherosclerosis Society BAS/British Society for Cardiovascular Research BSCR Annual Meeting"
            )),
            Some("heart".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some(
                "The FASEB Journal. Conference: Experimental Biology"
            )),
            Some("thefasebjournal".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some(
                "Arteriosclerosis Thrombosis and Vascular Biology. Conference: American Heart Association's Arteriosclerosis Thrombosis and Vascular Biology"
            )),
            Some("arteriosclerosisthrombosisandvascularbiology".to_string())
        );
        assert_eq!(Deduplicator::format_journal_name(None), None);
        assert_eq!(
            Deduplicator::format_journal_name(Some("")),
            Some("".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some("Diabetologie und Stoffwechsel. Conference")),
            Some("diabetologieundstoffwechsel".to_string())
        );
    }

    #[test]
    fn test_match_issns_scenarios() {
        // Scenario 1: Matching lists
        let issns1 = vec!["1234-5678".to_string(), "8765-4321".to_string()];
        let issns2 = vec!["0000-0000".to_string(), "1234-5678".to_string()];
        assert!(
            Deduplicator::match_issns(&issns1, &issns2),
            "Should find a matching ISSN"
        );

        let non_match_issns2 = vec!["5555-6666".to_string(), "7777-8888".to_string()];
        assert!(
            !Deduplicator::match_issns(&issns1, &non_match_issns2),
            "Should not find a matching ISSN"
        );

        // Scenario 3: Empty lists
        let empty_issns1: Vec<String> = vec![];
        let empty_issns2: Vec<String> = vec![];
        assert!(
            !Deduplicator::match_issns(&empty_issns1, &empty_issns2),
            "Should return false for empty lists"
        );

        // Scenario 4: One empty list
        let partial_issns1 = vec!["1234-5678".to_string()];
        let partial_issns2: Vec<String> = vec![];
        assert!(
            !Deduplicator::match_issns(&partial_issns1, &partial_issns2),
            "Should return false when one list is empty"
        );
    }

    #[test]
    fn test_format_issn() {
        assert_eq!(
            Deduplicator::format_issn("1234-5678"),
            Some("1234-5678".to_string())
        );
        assert_eq!(
            Deduplicator::format_issn("12345678"),
            Some("1234-5678".to_string())
        );
        assert_eq!(
            Deduplicator::format_issn("1234-567X"),
            Some("1234-567X".to_string())
        );
        assert_eq!(
            Deduplicator::format_issn("1234-567X (Electronic)"),
            Some("1234-567X".to_string())
        );
        assert_eq!(
            Deduplicator::format_issn("1234-5678 (Print)"),
            Some("1234-5678".to_string())
        );
        assert_eq!(
            Deduplicator::format_issn("1234-5678 (Linking)"),
            Some("1234-5678".to_string())
        );
        assert_eq!(Deduplicator::format_issn("invalid"), None);
        assert_eq!(Deduplicator::format_issn("1234-56789"), None);
        assert_eq!(Deduplicator::format_issn("123-45678"), None);
    }

    #[test]
    fn test_format_issn_rejects_x_in_non_final_position() {
        assert_eq!(Deduplicator::format_issn("123X-5678"), None);
        assert_eq!(Deduplicator::format_issn("X234-5678"), None);
        assert_eq!(Deduplicator::format_issn("1234-5X78"), None);
        assert_eq!(
            Deduplicator::format_issn("1234-567X"),
            Some("1234-567X".to_string())
        );
    }

    #[test]
    fn test_year_tolerance_zero_disables_cross_year_no_doi_matches() {
        let citations = vec![
            Citation {
                title: "Title 1".to_string(),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                journal: Some("Journal 1".to_string()),
                volume: Some("24".to_string()),
                pages: Some("100-110".to_string()),
                doi: None,
                ..Default::default()
            },
            Citation {
                title: "Title 1".to_string(),
                date: Some(crate::Date {
                    year: 2019,
                    month: None,
                    day: None,
                }),
                journal: Some("Journal 1".to_string()),
                volume: Some("24".to_string()),
                pages: Some("100-110".to_string()),
                doi: None,
                ..Default::default()
            },
        ];

        let deduplicator = Deduplicator::new();
        let duplicate_groups = deduplicator.find_duplicates(&citations);

        assert_eq!(duplicate_groups.len(), 1);
        assert_eq!(duplicate_groups[0].duplicates.len(), 1);

        let deduplicator = Deduplicator::builder().year_tolerance(0).build();
        let duplicate_groups = deduplicator.find_duplicates(&citations);

        assert_eq!(duplicate_groups.len(), 2);
        assert!(duplicate_groups.iter().all(|g| g.duplicates.is_empty()));
    }

    #[test]
    fn test_unknown_year_matches_known_year_end_to_end() {
        let citations = vec![
            make_citation(
                "Yearless cross block",
                None,
                Some("Journal"),
                Some("6"),
                None,
                None,
            ),
            make_citation(
                "Yearless cross block",
                Some(2020),
                Some("Journal"),
                Some("6"),
                None,
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].unique, 0);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_source_preferences() {
        let citations = vec![
            Citation {
                title: "Title 1".to_string(),
                doi: Some("10.1234/abc".to_string()),
                journal: Some("Journal 1".to_string()),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                ..Default::default()
            },
            Citation {
                title: "Title 1".to_string(),
                doi: Some("10.1234/abc".to_string()),
                journal: Some("Journal 1".to_string()),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                ..Default::default()
            },
        ];

        let sources = vec!["source2", "source1"];

        let deduplicator = Deduplicator::builder()
            .source_preferences(["source1", "source2"])
            .build();
        let duplicate_groups = deduplicator.find_duplicates_with_sources(&citations, &sources);

        assert_eq!(duplicate_groups.len(), 1);
        // The second citation should be selected as unique because source1 (PubMed)
        // has higher priority than source2 (Embase) in our preferences
        assert_eq!(duplicate_groups[0].unique, 1);
        assert_eq!(duplicate_groups[0].duplicates.len(), 1);
    }

    #[test]
    fn test_abstract_preference() {
        let citations = vec![
            Citation {
                title: "Title 1".to_string(),
                abstract_text: None,
                doi: Some("10.1234/abc".to_string()),
                journal: Some("Journal 1".to_string()),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                ..Default::default()
            },
            Citation {
                title: "Title 1".to_string(),
                abstract_text: Some("Abstract".to_string()),
                doi: Some("10.1234/abc".to_string()),
                journal: Some("Journal 1".to_string()),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                ..Default::default()
            },
        ];

        let deduplicator = Deduplicator::new();
        let duplicate_groups = deduplicator.find_duplicates(&citations);

        assert_eq!(duplicate_groups.len(), 1);
        // The citation with abstract should be selected as unique
        assert!(
            citations[duplicate_groups[0].unique]
                .abstract_text
                .is_some()
        );
        assert_eq!(duplicate_groups[0].duplicates.len(), 1);
    }

    #[test]
    fn test_source_preferences_with_year_grouping() {
        // Create citations from different years to test year grouping with source preferences
        let citations = vec![
            Citation {
                title: "Test Article 2020".to_string(),
                doi: Some("10.1234/test2020".to_string()),
                journal: Some("Test Journal".to_string()),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                ..Default::default()
            },
            Citation {
                title: "Test Article 2020".to_string(), // Same as above but different source
                doi: Some("10.1234/test2020".to_string()),
                journal: Some("Test Journal".to_string()),
                date: Some(crate::Date {
                    year: 2020,
                    month: None,
                    day: None,
                }),
                ..Default::default()
            },
            Citation {
                title: "Test Article 2021".to_string(),
                doi: Some("10.1234/test2021".to_string()),
                journal: Some("Test Journal".to_string()),
                date: Some(crate::Date {
                    year: 2021,
                    month: None,
                    day: None,
                }),
                ..Default::default()
            },
            Citation {
                title: "Test Article 2021".to_string(), // Same as above but different source
                doi: Some("10.1234/test2021".to_string()),
                journal: Some("Test Journal".to_string()),
                date: Some(crate::Date {
                    year: 2021,
                    month: None,
                    day: None,
                }),
                ..Default::default()
            },
        ];

        // Sources with PubMed having higher priority
        let sources = vec!["Embase", "PubMed", "Embase", "PubMed"];

        let deduplicator = Deduplicator::builder()
            .year_tolerance(0)
            .parallel(false)
            .source_preferences(["PubMed", "Embase"])
            .build();
        let duplicate_groups = deduplicator.find_duplicates_with_sources(&citations, &sources);

        // Should find 2 duplicate groups (one for each year)
        assert_eq!(duplicate_groups.len(), 2);

        // Both unique citations should be from PubMed (indices 1 and 3)
        // We can't directly check the source, but we can check that each group has the expected structure
        let unique_titles: Vec<&str> = duplicate_groups
            .iter()
            .map(|group| citations[group.unique].title.as_str())
            .collect();

        assert!(unique_titles.contains(&"Test Article 2020"));
        assert!(unique_titles.contains(&"Test Article 2021"));

        // Each group should have exactly one duplicate
        for group in &duplicate_groups {
            assert_eq!(group.duplicates.len(), 1);
        }
    }

    #[test]
    fn test_empty_title_is_singleton_unless_doi_matched() {
        let citations = vec![
            make_citation(
                "",
                Some(2020),
                Some("Journal"),
                Some("1"),
                Some("10-20"),
                None,
            ),
            make_citation(
                "",
                Some(2020),
                Some("Journal"),
                Some("1"),
                Some("10-20"),
                Some("10.1000/empty-title"),
            ),
            make_citation(
                "Linked record",
                Some(2020),
                Some("Journal"),
                Some("1"),
                Some("10-20"),
                Some("https://doi.org/10.1000/empty-title"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|group| group_members(group) == vec![0]));
        assert!(
            groups
                .iter()
                .any(|group| group_members(group) == vec![1, 2])
        );
    }

    #[test]
    fn test_empty_journal_strings_do_not_match() {
        let citations = vec![
            make_citation("Shared title", Some(2020), Some(""), Some("12"), None, None),
            make_citation("Shared title", Some(2020), Some(""), Some("12"), None, None),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
    }

    #[test]
    fn test_doi_variants_are_deduplicated() {
        let citations = vec![
            make_citation(
                "Normalized DOI article",
                Some(2023),
                Some("Journal"),
                None,
                None,
                Some("https://doi.org/10.1000/X"),
            ),
            make_citation(
                "Normalized DOI article",
                Some(2023),
                Some("Journal"),
                None,
                None,
                Some("10.1000/x"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_truncated_pages_share_start_page_match() {
        let citations = vec![
            make_citation(
                "Paged article",
                Some(2020),
                Some("Journal"),
                None,
                Some("1234-45"),
                None,
            ),
            make_citation(
                "Paged article",
                Some(2020),
                Some("Journal"),
                None,
                Some("1234-1245"),
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_default_year_tolerance_matches_adjacent_years_without_doi() {
        let citations = vec![
            make_citation(
                "Cross-year match",
                Some(2020),
                Some("Journal"),
                Some("8"),
                None,
                None,
            ),
            make_citation(
                "Cross-year match",
                Some(2021),
                Some("Journal"),
                Some("8"),
                None,
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);

        let strict_groups = Deduplicator::builder()
            .year_tolerance(0)
            .build()
            .find_duplicates(&citations);
        assert_eq!(strict_groups.len(), 2);
    }

    #[test]
    fn test_custom_year_tolerance_two_matches_two_year_gap() {
        let citations = vec![
            make_citation(
                "Two year gap",
                Some(2020),
                Some("Journal"),
                Some("9"),
                None,
                None,
            ),
            make_citation(
                "Two year gap",
                Some(2022),
                Some("Journal"),
                Some("9"),
                None,
                None,
            ),
        ];

        let groups = Deduplicator::builder()
            .year_tolerance(2)
            .build()
            .find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_missing_years_can_match_without_doi() {
        let citations = vec![
            make_citation(
                "Yearless article",
                None,
                Some("Journal"),
                Some("4"),
                None,
                None,
            ),
            make_citation(
                "Yearless article",
                None,
                Some("Journal"),
                Some("4"),
                None,
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_transitivity_groups_three_records() {
        let citations = vec![
            make_citation(
                "",
                Some(2020),
                Some("Journal"),
                Some("1"),
                Some("10-20"),
                Some("10.1000/transitive"),
            ),
            make_citation(
                "Linked transitive study",
                Some(2020),
                Some("Journal"),
                Some("1"),
                Some("10-20"),
                Some("10.1000/transitive"),
            ),
            make_citation(
                "Linked transitive study",
                Some(2020),
                Some("Journal"),
                Some("1"),
                Some("10-20"),
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(group_members(&groups[0]), vec![0, 1, 2]);
    }

    #[test]
    fn test_series_guard_blocks_numeric_and_roman_suffixes() {
        let numeric = vec![
            make_citation(
                "Study part 1",
                Some(2020),
                Some("Journal"),
                Some("5"),
                Some("100-110"),
                None,
            ),
            make_citation(
                "Study part 2",
                Some(2020),
                Some("Journal"),
                Some("5"),
                Some("200-210"),
                None,
            ),
        ];
        let numeric_groups = Deduplicator::new().find_duplicates(&numeric);
        assert_eq!(numeric_groups.len(), 2);

        let roman = vec![
            make_citation(
                "Part ii",
                Some(2020),
                Some("Journal"),
                Some("5"),
                Some("100-110"),
                None,
            ),
            make_citation(
                "Part iii",
                Some(2020),
                Some("Journal"),
                Some("5"),
                Some("200-210"),
                None,
            ),
        ];
        let roman_groups = Deduplicator::new().find_duplicates(&roman);
        assert_eq!(roman_groups.len(), 2);
    }

    #[test]
    fn test_series_guard_does_not_trigger_for_identical_trailing_digits() {
        let citations = vec![
            make_citation(
                "Baseline report 2024",
                Some(2024),
                Some("Journal"),
                Some("2"),
                Some("10-12"),
                None,
            ),
            make_citation(
                "Baseline report 2024",
                Some(2024),
                Some("Journal"),
                Some("2"),
                Some("40-42"),
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_author_guard_blocks_borderline_similarity() {
        let left_title = "integratedgenomicsforcancer";
        let right_title = "integratedgeneticsforcancer";
        let similarity = strsim::jaro_winkler(left_title, right_title);
        assert!(
            (0.93..AUTHOR_GUARD_THRESHOLD).contains(&similarity),
            "expected a borderline similarity, got {similarity}"
        );

        let citations = vec![
            make_citation_with_author(
                left_title,
                "Smith",
                Some(2020),
                Some("Journal"),
                Some("7"),
                Some("100-110"),
            ),
            make_citation_with_author(
                right_title,
                "Jones",
                Some(2020),
                Some("Journal"),
                Some("7"),
                Some("100-110"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_pass1_same_doi_without_other_metadata_still_matches() {
        let citations = vec![
            make_citation(
                "Renal biomarker observational study",
                Some(2020),
                None,
                None,
                None,
                Some("10.1000/pass1"),
            ),
            make_citation(
                "Observational study of renal biomarkers",
                Some(2021),
                None,
                None,
                None,
                Some("10.1000/pass1"),
            ),
        ];

        let sim = strsim::jaro(
            &Deduplicator::normalize_string(&citations[0].title),
            &Deduplicator::normalize_string(&citations[1].title),
        );
        assert!(
            sim >= PASS1_DOI_TITLE_SANITY && sim < 0.9,
            "unexpected pass1 sim {sim}"
        );

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_pass1_empty_titles_without_metadata_agreement_stay_separate() {
        let citations = vec![
            make_citation(
                "",
                Some(2020),
                Some("Journal A"),
                Some("1"),
                Some("10-20"),
                Some("10.1000/meta"),
            ),
            make_citation(
                "",
                Some(2021),
                Some("Journal B"),
                Some("2"),
                Some("30-40"),
                Some("10.1000/meta"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
    }

    #[test]
    fn test_doi_title_threshold_controls_both_doi_matching() {
        let citations = vec![
            make_citation(
                "Renal biomarker observational study",
                Some(2020),
                Some("Journal"),
                Some("7"),
                Some("100-110"),
                Some("10.1000/left"),
            ),
            make_citation(
                "Observational renal biomarker study",
                Some(2020),
                Some("Journal"),
                Some("7"),
                Some("100-110"),
                Some("10.1000/right"),
            ),
        ];

        let sim = strsim::jaro(
            &Deduplicator::normalize_string(&citations[0].title),
            &Deduplicator::normalize_string(&citations[1].title),
        );
        assert!(
            sim >= DEFAULT_BOTH_DOI_TITLE_THRESHOLD && sim < 0.99,
            "unexpected doi-title sim {sim}"
        );

        let default_groups = Deduplicator::new().find_duplicates(&citations);
        assert_eq!(default_groups.len(), 1);
        assert_eq!(default_groups[0].duplicates, vec![1]);

        let strict_groups = Deduplicator::builder()
            .doi_title_threshold(0.99)
            .build()
            .find_duplicates(&citations);
        assert_eq!(strict_groups.len(), 2);
        assert!(
            strict_groups
                .iter()
                .all(|group| group.duplicates.is_empty())
        );
    }

    #[test]
    fn test_determinism_across_shuffle_and_repeated_runs() {
        let citations = vec![
            make_citation(
                "Alpha study",
                Some(2020),
                Some("J1"),
                Some("1"),
                Some("10-12"),
                None,
            ),
            make_citation(
                "Alpha study",
                Some(2020),
                Some("J1"),
                Some("1"),
                Some("10-12"),
                None,
            ),
            make_citation(
                "Beta study",
                Some(2021),
                Some("J2"),
                Some("2"),
                Some("20-22"),
                None,
            ),
            make_citation(
                "Gamma study",
                Some(2022),
                Some("J3"),
                Some("3"),
                Some("30-32"),
                None,
            ),
            make_citation(
                "Gamma study",
                Some(2022),
                Some("J3"),
                Some("3"),
                Some("30-32"),
                None,
            ),
        ];

        let deduplicator = Deduplicator::new();
        let first_run = deduplicator.find_duplicates(&citations);
        let second_run = deduplicator.find_duplicates(&citations);
        assert_eq!(first_run, second_run);

        let shuffled_order = vec![4, 1, 3, 0, 2];
        let shuffled_citations = shuffled_order
            .iter()
            .map(|&idx| citations[idx].clone())
            .collect::<Vec<_>>();
        let shuffled_groups = deduplicator.find_duplicates(&shuffled_citations);

        let identity_map = (0..citations.len()).collect::<Vec<_>>();
        assert_eq!(
            canonicalize_groups(&first_run, &identity_map),
            canonicalize_groups(&shuffled_groups, &shuffled_order)
        );
    }

    #[test]
    fn test_parallel_matches_sequential_on_large_fixture() {
        let mut citations = Vec::new();
        for group in 0..40 {
            for duplicate in 0..3 {
                let mut citation = make_citation(
                    &format!("Fixture title {group}"),
                    if group % 5 == 0 {
                        None
                    } else {
                        Some(2020 + (group % 3))
                    },
                    Some("Fixture Journal"),
                    Some(&format!("{group}")),
                    Some(&format!("{}-{}", 1000 + group, 1010 + group)),
                    None,
                );
                if duplicate == 1 {
                    citation.abstract_text = Some("Fixture abstract".to_string());
                }
                if duplicate == 2 {
                    citation.journal_abbr = Some("Fixture J".to_string());
                }
                citations.push(citation);
            }
        }

        let sequential = Deduplicator::builder()
            .parallel(false)
            .build()
            .find_duplicates(&citations);
        let parallel = Deduplicator::builder()
            .parallel(true)
            .build()
            .find_duplicates(&citations);

        assert_eq!(sequential, parallel);
        assert!(sequential.len() >= 40);
    }

    #[test]
    fn test_abstract_preference_ignores_empty_string() {
        let mut first = make_citation(
            "Abstract preference",
            Some(2020),
            Some("Journal"),
            Some("2"),
            Some("10-20"),
            Some("10.1000/abstract"),
        );
        first.abstract_text = Some("".to_string());

        let mut second = make_citation(
            "Abstract preference",
            Some(2020),
            Some("Journal"),
            Some("2"),
            Some("10-20"),
            Some("10.1000/abstract"),
        );
        second.abstract_text = Some("Real abstract".to_string());

        let groups = Deduplicator::new().find_duplicates(&[first, second.clone()]);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].unique, 1);
    }

    #[test]
    fn test_sources_shorter_and_longer_are_tolerated() {
        let citations = vec![
            make_citation(
                "Source preference",
                Some(2020),
                Some("Journal"),
                Some("2"),
                Some("10-12"),
                Some("10.1000/source"),
            ),
            make_citation(
                "Source preference",
                Some(2020),
                Some("Journal"),
                Some("2"),
                Some("10-12"),
                Some("10.1000/source"),
            ),
        ];

        let deduplicator = Deduplicator::builder()
            .source_preferences(["PubMed", "Embase"])
            .build();

        let shorter = deduplicator.find_duplicates_with_sources(&citations, &["PubMed"]);
        assert_eq!(shorter.len(), 1);
        assert_eq!(shorter[0].unique, 0);

        let longer =
            deduplicator.find_duplicates_with_sources(&citations, &["Embase", "PubMed", "Extra"]);
        assert_eq!(longer.len(), 1);
        assert_eq!(longer[0].unique, 1);
    }

    #[test]
    fn test_every_input_index_appears_exactly_once() {
        let citations = vec![
            make_citation(
                "One",
                Some(2020),
                Some("J1"),
                Some("1"),
                Some("10-11"),
                None,
            ),
            make_citation(
                "One",
                Some(2020),
                Some("J1"),
                Some("1"),
                Some("10-11"),
                None,
            ),
            make_citation(
                "Two",
                Some(2021),
                Some("J2"),
                Some("2"),
                Some("20-21"),
                None,
            ),
            make_citation(
                "Three",
                Some(2022),
                Some("J3"),
                Some("3"),
                Some("30-31"),
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(covered_indices(&groups), vec![0, 1, 2, 3]);
    }

    #[test]
    #[should_panic(expected = "within (0.0, 1.0]")]
    fn test_builder_panics_on_zero_threshold() {
        let _ = Deduplicator::builder().exact_title_threshold(0.0).build();
    }

    #[test]
    #[should_panic(expected = "within (0.0, 1.0]")]
    fn test_builder_panics_on_threshold_above_one() {
        let _ = Deduplicator::builder().no_doi_title_threshold(1.5).build();
    }

    #[test]
    #[should_panic(expected = "less than or equal to exact_title_threshold")]
    fn test_builder_panics_when_doi_threshold_exceeds_exact_threshold() {
        let _ = Deduplicator::builder().doi_title_threshold(0.995).build();
    }
}
