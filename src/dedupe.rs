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
//!     .no_doi_title_threshold(0.93)
//!     .exact_title_threshold(0.99)
//!     .build();
//!
//! let _ = deduplicator;
//! ```
//!
//! `build()` panics when a threshold is outside `(0.0, 1.0]`, or when
//! `no_doi_title_threshold` exceeds `exact_title_threshold`.
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
//! | Title similarity | `jaro >= 0.90`, or a stricter corroborated rescue path |
//!
//! If either normalized title is empty, pass 1 falls back to metadata
//! agreement on journal, ISSN, volume, or normalized start page.
//!
//! When both normalized titles are non-empty but `jaro < 0.90`, pass 1 only
//! rescues the pair when there is no first-author disagreement, either journal
//! or ISSN matches, start pages match, and the series guard does not reject
//! the pair.
//!
//! ### Pass 2: Blocked fuzzy matching
//!
//! Records with non-empty normalized titles are compared in year-based blocks.
//! Unknown-year records are compared with the unknown-year block and each
//! concrete year block.
//!
//! When both records have normalized DOIs:
//!
//! - Matching normalized DOIs are handled in pass 1.
//! - Conflicting normalized DOIs are treated as non-duplicates.
//!
//! When at least one DOI is missing:
//!
//! | Path | Required |
//! | --- | --- |
//! | Standard fuzzy path | `jaro_winkler >= no_doi_title_threshold` and year compatible and tiered publication evidence (page match, or volume+issue match, or same-year volume-only fallback when issue/page are missing) and `(journal or ISSN)` |
//! | Exact-title fallback | `jaro_winkler >= exact_title_threshold` and year compatible and volume match and page match |
//!
//! Additional guards:
//!
//! - Series and erratum suffixes such as `part 1` / `part 2` require matching
//!   start pages for any borderline title match below `exact_title_threshold`.
//! - Different first-author surnames block borderline matches below the internal
//!   author-guard threshold.
//! - A strict metadata-only path exists for fully bracketed translated-title
//!   records when journal/ISSN, year, volume, pages, first author, and (if
//!   present on both sides) issue all agree exactly.
//! - Contradictory no-DOI page metadata, and contradictory issue metadata when
//!   volume agrees, veto otherwise borderline no-DOI matches.
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
//! - Author keys use lowercase alphanumerics after Unicode NFKD folding
//! - Issue values normalize into conservative lowercase alphanumeric keys
//! - Titles decode numeric/common HTML entities, lowercase, strip supported
//!   HTML tags, expand Greek characters to word forms, apply Unicode NFKD,
//!   strip combining marks, remove explicit markup artifacts, and reduce to
//!   alphanumerics
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

const NO_DOI_TITLE_SIMILARITY_THRESHOLD: f64 = 0.93;
const PASS1_DOI_TITLE_SANITY: f64 = 0.90;
const AUTHOR_GUARD_THRESHOLD: f64 = 0.96;

static UNICODE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<U\+([0-9A-Fa-f]+)>").unwrap());

static HTML_ENTITY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&(#x[0-9a-fA-F]+|#\d+|[A-Za-z]+);").unwrap());

const HTML_TAG_REPLACEMENTS: [(&str, &str); 8] = [
    ("<sup>", ""),
    ("</sup>", ""),
    ("<sub>", ""),
    ("</sub>", ""),
    ("<inf>", ""),
    ("</inf>", ""),
    ("<i>", ""),
    ("</i>", ""),
];

const TITLE_MARKUP_TOKEN_REPLACEMENTS: [(&str, &str); 8] = [
    ("[sub]", ""),
    ("[/sub]", ""),
    ("[sup]", ""),
    ("[/sup]", ""),
    ("(sub)", ""),
    ("(/sub)", ""),
    ("(sup)", ""),
    ("(/sup)", ""),
];

const BOILERPLATE_TITLE_PREFIXES: [&str; 5] = [
    "brief report",
    "clinical note",
    "case report",
    "short communication",
    "letter",
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
    no_doi_title_threshold: f64,
    exact_title_threshold: f64,
}

#[derive(Debug, Clone)]
struct DedupRecord {
    idx: usize,
    norm_title: String,
    alt_norm_title: Option<String>,
    has_bracketed_translation_title: bool,
    norm_doi: Option<String>,
    norm_journal: Option<String>,
    norm_journal_abbr: Option<String>,
    norm_issns: Vec<String>,
    norm_volume: Option<String>,
    norm_issue: Option<String>,
    norm_start_page: Option<String>,
    year: Option<i32>,
    first_author_key: Option<String>,
}

#[derive(Debug, Clone)]
enum BlockTask {
    Within(Vec<usize>),
    Cross(Vec<usize>, Vec<usize>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataRelation {
    Match,
    Missing,
    Conflict,
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
        Self::validate_threshold("no_doi_title_threshold", self.no_doi_title_threshold);
        Self::validate_threshold("exact_title_threshold", self.exact_title_threshold);

        assert!(
            self.no_doi_title_threshold <= self.exact_title_threshold,
            "DeduplicatorBuilder::build(): no_doi_title_threshold must be less than or equal to exact_title_threshold"
        );

        Deduplicator {
            year_tolerance: self.year_tolerance,
            parallel: self.parallel,
            source_preferences: self.source_preferences,
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
                let converted_title = Self::convert_unicode_string(&citation.title);
                let norm_title = Self::normalize_string(&converted_title);
                let mut seen_issns = HashSet::new();
                let norm_issns = citation
                    .issn
                    .iter()
                    .filter_map(|issn| Self::format_issn(issn))
                    .filter(|issn| seen_issns.insert(issn.clone()))
                    .collect();

                DedupRecord {
                    idx,
                    alt_norm_title: Self::strip_boilerplate_title_prefix(&converted_title)
                        .map(Self::normalize_string)
                        .filter(|alt_title| !alt_title.is_empty() && alt_title != &norm_title),
                    has_bracketed_translation_title: Self::is_bracketed_translation_title(
                        &converted_title,
                    ),
                    norm_title,
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
                    norm_issue: citation
                        .issue
                        .as_deref()
                        .map(Self::normalize_issue)
                        .filter(|issue| !issue.is_empty()),
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

                    let should_union = if left.norm_title.is_empty() || right.norm_title.is_empty()
                    {
                        Self::has_metadata_agreement(left, right)
                    } else {
                        self.pass1_same_doi_titles_match(left, right)
                    };

                    if should_union {
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

        let mut normalized = Self::convert_unicode_string(string);
        normalized = Self::decode_html_entities(&normalized);
        normalized = normalized.trim().to_lowercase();
        for article in ["the ", "a ", "an "] {
            if let Some(stripped) = normalized.strip_prefix(article) {
                normalized = stripped.to_string();
                break;
            }
        }
        for (needle, replacement) in HTML_TAG_REPLACEMENTS {
            normalized = normalized.replace(needle, replacement);
        }
        for (needle, replacement) in TITLE_MARKUP_TOKEN_REPLACEMENTS {
            normalized = normalized.replace(needle, replacement);
        }
        normalized = Self::expand_greek_characters(&normalized);

        normalized
            .nfkd()
            .filter(|ch| !is_combining_mark(*ch))
            .filter(|ch| ch.is_alphanumeric())
            .collect()
    }

    fn strip_boilerplate_title_prefix(title: &str) -> Option<&str> {
        let trimmed = title.trim();
        let lower = trimmed.to_lowercase();

        for prefix in BOILERPLATE_TITLE_PREFIXES {
            if let Some(remainder) = lower.strip_prefix(prefix) {
                if remainder.is_empty()
                    || !remainder
                        .chars()
                        .next()
                        .is_some_and(Self::is_boilerplate_separator)
                {
                    continue;
                }

                let original_remainder = &trimmed[prefix.len()..];
                let stripped = original_remainder
                    .trim_start_matches(Self::is_boilerplate_separator)
                    .trim();
                if !stripped.is_empty() {
                    return Some(stripped);
                }
            }
        }

        None
    }

    fn is_boilerplate_separator(ch: char) -> bool {
        ch.is_whitespace() || matches!(ch, '.' | ':' | ';' | ',' | '-' | '–' | '—')
    }

    fn decode_html_entities(input: &str) -> String {
        HTML_ENTITY_REGEX
            .replace_all(input, |caps: &crate::regex::Captures| {
                Self::decode_html_entity(&caps[1]).unwrap_or_else(|| caps[0].to_string())
            })
            .to_string()
    }

    fn decode_html_entity(entity: &str) -> Option<String> {
        if let Some(hex) = entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
        {
            return u32::from_str_radix(hex, 16)
                .ok()
                .and_then(char::from_u32)
                .map(|ch| ch.to_string());
        }

        if let Some(decimal) = entity.strip_prefix('#') {
            return decimal
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(|ch| ch.to_string());
        }

        match entity.to_ascii_lowercase().as_str() {
            "amp" => Some("&".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" => Some("'".to_string()),
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "nbsp" => Some(" ".to_string()),
            "alpha" => Some("\u{03B1}".to_string()),
            "beta" => Some("\u{03B2}".to_string()),
            "gamma" => Some("\u{03B3}".to_string()),
            "delta" => Some("\u{03B4}".to_string()),
            "epsilon" => Some("\u{03B5}".to_string()),
            "kappa" => Some("\u{03BA}".to_string()),
            "lambda" => Some("\u{03BB}".to_string()),
            "mu" => Some("\u{03BC}".to_string()),
            "omega" => Some("\u{03C9}".to_string()),
            _ => None,
        }
    }

    fn expand_greek_characters(input: &str) -> String {
        let mut expanded = String::with_capacity(input.len());
        for ch in input.chars() {
            match ch {
                '\u{00DF}' | '\u{03B2}' => expanded.push_str("beta"),
                '\u{03B1}' => expanded.push_str("alpha"),
                '\u{03B3}' => expanded.push_str("gamma"),
                '\u{03B4}' => expanded.push_str("delta"),
                '\u{03B5}' => expanded.push_str("epsilon"),
                '\u{03B6}' => expanded.push_str("zeta"),
                '\u{03B7}' => expanded.push_str("eta"),
                '\u{03B8}' => expanded.push_str("theta"),
                '\u{03B9}' => expanded.push_str("iota"),
                '\u{03BA}' => expanded.push_str("kappa"),
                '\u{03BB}' => expanded.push_str("lambda"),
                '\u{03BC}' | '\u{00B5}' => expanded.push_str("mu"),
                '\u{03BD}' => expanded.push_str("nu"),
                '\u{03BE}' => expanded.push_str("xi"),
                '\u{03BF}' => expanded.push_str("omicron"),
                '\u{03C0}' => expanded.push_str("pi"),
                '\u{03C1}' => expanded.push_str("rho"),
                '\u{03C3}' | '\u{03C2}' => expanded.push_str("sigma"),
                '\u{03C4}' => expanded.push_str("tau"),
                '\u{03C5}' => expanded.push_str("upsilon"),
                '\u{03C6}' => expanded.push_str("phi"),
                '\u{03C7}' => expanded.push_str("chi"),
                '\u{03C8}' => expanded.push_str("psi"),
                '\u{03C9}' => expanded.push_str("omega"),
                _ => expanded.push(ch),
            }
        }

        expanded
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

    fn normalize_issue(issue: &str) -> String {
        if issue.is_empty() {
            return String::new();
        }

        let mut normalized = issue.trim().to_lowercase();
        normalized = normalized
            .trim_matches(|ch: char| matches!(ch, '(' | ')' | '[' | ']'))
            .to_string();

        for prefix in ["issue", "no.", "no", "number", "nr.", "nr"] {
            if let Some(stripped) = normalized.strip_prefix(prefix) {
                normalized = stripped
                    .trim_start_matches(|ch: char| {
                        ch.is_whitespace() || matches!(ch, '.' | ':' | '-' | '/' | '#')
                    })
                    .trim()
                    .to_string();
                break;
            }
        }

        normalized
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .collect()
    }

    fn normalize_start_page(pages: &str) -> Option<String> {
        let formatted = format_page_numbers(pages);
        let start_segment = formatted.split('-').next().unwrap_or("");
        let canonical_start = start_segment
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();

        if matches!(canonical_start.as_str(), "npag" | "nopagination" | "na") {
            return None;
        }

        let start_page = start_segment
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
            .nfkd()
            .filter(|ch| !is_combining_mark(*ch))
            .filter(|ch| ch.is_alphanumeric())
            .collect::<String>();

        (!normalized.is_empty()).then_some(normalized)
    }

    fn pass1_same_doi_titles_match(&self, left: &DedupRecord, right: &DedupRecord) -> bool {
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

    fn max_title_similarity(
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

    fn passes_author_guard(left: &DedupRecord, right: &DedupRecord, sim: f64) -> bool {
        if sim >= AUTHOR_GUARD_THRESHOLD {
            return true;
        }

        !Self::authors_conflict(left, right)
    }

    fn authors_conflict(left: &DedupRecord, right: &DedupRecord) -> bool {
        matches!(
            (&left.first_author_key, &right.first_author_key),
            (Some(left), Some(right)) if left != right
        )
    }

    fn matches_bracketed_translation_metadata_path(
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

    fn issues_compatible(left: &DedupRecord, right: &DedupRecord, issue_match: bool) -> bool {
        issue_match || left.norm_issue.is_none() || right.norm_issue.is_none()
    }

    fn same_first_author(left: &DedupRecord, right: &DedupRecord) -> bool {
        left.first_author_key
            .as_ref()
            .zip(right.first_author_key.as_ref())
            .is_some_and(|(left, right)| left == right)
    }

    fn fails_series_guard(
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
            || Self::options_match(&left.norm_issue, &right.norm_issue)
            || Self::options_match(&left.norm_start_page, &right.norm_start_page)
    }

    fn is_bracketed_translation_title(title: &str) -> bool {
        let trimmed = title.trim();
        trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && trimmed[1..trimmed.len().saturating_sub(1)]
                .trim()
                .chars()
                .any(|ch| !ch.is_whitespace())
    }

    fn options_match(left: &Option<String>, right: &Option<String>) -> bool {
        matches!(Self::option_relation(left, right), MetadataRelation::Match)
    }

    fn options_conflict(left: &Option<String>, right: &Option<String>) -> bool {
        matches!(
            Self::option_relation(left, right),
            MetadataRelation::Conflict
        )
    }

    fn option_relation(left: &Option<String>, right: &Option<String>) -> MetadataRelation {
        match (left, right) {
            (Some(left), Some(right)) if left == right => MetadataRelation::Match,
            (Some(_), Some(_)) => MetadataRelation::Conflict,
            _ => MetadataRelation::Missing,
        }
    }

    fn has_issue_conflict(left: &DedupRecord, right: &DedupRecord) -> bool {
        Self::options_conflict(&left.norm_issue, &right.norm_issue)
    }

    fn has_page_conflict(left: &DedupRecord, right: &DedupRecord) -> bool {
        Self::options_conflict(&left.norm_start_page, &right.norm_start_page)
    }

    fn no_doi_publication_match(
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
            let normalized = name
                .split(". Conference")
                .next()
                .unwrap_or(name)
                .trim()
                .to_lowercase();
            let normalized = normalized.strip_prefix("the ").unwrap_or(&normalized);
            normalized
                .replace("&amp;", " and ")
                .replace('&', " and ")
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

    fn make_citation_with_author_and_doi(
        title: &str,
        author_name: &str,
        year: Option<i32>,
        journal: Option<&str>,
        volume: Option<&str>,
        pages: Option<&str>,
        doi: Option<&str>,
    ) -> Citation {
        let mut citation = make_citation(title, year, journal, volume, pages, doi);
        citation.authors.push(Author {
            name: author_name.to_string(),
            given_name: None,
            middle_name: None,
            affiliations: Vec::new(),
        });
        citation
    }

    fn with_issue(mut citation: Citation, issue: Option<&str>) -> Citation {
        citation.issue = issue.map(str::to_string);
        citation
    }

    fn with_issn(mut citation: Citation, issn: &[&str]) -> Citation {
        citation.issn = issn.iter().map(|value| (*value).to_string()).collect();
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
        assert_eq!(
            Deduplicator::normalize_string(
                "β-blocker effects in &#946;-cells &amp; &#x3B2;-agonists"
            ),
            "betablockereffectsinbetacellsbetaagonists".to_string()
        );
        assert_eq!(
            Deduplicator::normalize_string("beta-blocker effects in β-cells"),
            "betablockereffectsinbetacells".to_string()
        );
        assert_eq!(
            Deduplicator::normalize_string("&quot;α&quot; vs &Alpha; and ß"),
            "alphavsalphaandbeta".to_string()
        );
        assert_eq!(
            Deduplicator::normalize_string("Gene[sub]A[/sub] (sup)2(/sup)"),
            "genea2".to_string()
        );
        assert_eq!(
            Deduplicator::normalize_string("Subgroup analysis [sup]A[/sup]"),
            "subgroupanalysisa".to_string()
        );
        assert_eq!(
            Deduplicator::normalize_string("The Immune Response"),
            "immuneresponse".to_string()
        );
        assert_eq!(
            Deduplicator::normalize_string("A clinical pathway"),
            "clinicalpathway".to_string()
        );
        assert_eq!(
            Deduplicator::normalize_string("An observational cohort"),
            "observationalcohort".to_string()
        );
    }

    #[test]
    fn test_normalize_author_key_unicode_folding() {
        assert_eq!(
            Deduplicator::normalize_author_key("Keyriläinen"),
            Some("keyrilainen".to_string())
        );
        assert_eq!(
            Deduplicator::normalize_author_key("Keyrilainen"),
            Some("keyrilainen".to_string())
        );
    }

    #[test]
    fn test_normalize_start_page_treats_placeholders_as_missing() {
        for placeholder in ["N.PAG", "N.PAG-N.PAG", "no pagination", "n/a", "na"] {
            assert_eq!(Deduplicator::normalize_start_page(placeholder), None);
        }
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
            Some("fasebjournal".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some(
                "Arteriosclerosis Thrombosis and Vascular Biology. Conference: American Heart Association's Arteriosclerosis Thrombosis and Vascular Biology"
            )),
            Some("arteriosclerosisthrombosisandvascularbiology".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some(
                "Journal of Sports Medicine & Physical Fitness"
            )),
            Some("journalofsportsmedicineandphysicalfitness".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some(
                "The Journal of sports medicine and physical fitness"
            )),
            Some("journalofsportsmedicineandphysicalfitness".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some(
                "Journal of sports medicine and physical fitness"
            )),
            Some("journalofsportsmedicineandphysicalfitness".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some(
                "Journal of Sports Medicine &amp; Physical Fitness"
            )),
            Some("journalofsportsmedicineandphysicalfitness".to_string())
        );
        assert_eq!(
            Deduplicator::format_journal_name(Some("Thorax")),
            Some("thorax".to_string())
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
    fn test_normalize_issue() {
        assert_eq!(Deduplicator::normalize_issue("Issue 4"), "4".to_string());
        assert_eq!(Deduplicator::normalize_issue("No. 4"), "4".to_string());
        assert_eq!(
            Deduplicator::normalize_issue("(Suppl 2)"),
            "suppl2".to_string()
        );
        assert_eq!(Deduplicator::normalize_issue("S-1"), "s1".to_string());
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
                Some("70-80"),
                None,
            ),
            make_citation(
                "Yearless cross block",
                Some(2020),
                Some("Journal"),
                Some("6"),
                Some("70-80"),
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
    fn test_unicode_dash_pages_share_start_page_match() {
        let citations = vec![
            make_citation(
                "Paged article",
                Some(2020),
                Some("Journal"),
                None,
                Some("1417‐1422"),
                None,
            ),
            make_citation(
                "Paged article",
                Some(2020),
                Some("Journal"),
                None,
                Some("1417-1422"),
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
                Some("100-110"),
                None,
            ),
            make_citation(
                "Cross-year match",
                Some(2021),
                Some("Journal"),
                Some("8"),
                Some("100-110"),
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
                Some("200-210"),
                None,
            ),
            make_citation(
                "Two year gap",
                Some(2022),
                Some("Journal"),
                Some("9"),
                Some("200-210"),
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
                Some("50-60"),
                None,
            ),
            make_citation(
                "Yearless article",
                None,
                Some("Journal"),
                Some("4"),
                Some("50-60"),
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_issue_can_help_match_when_volume_matches_and_page_is_missing() {
        let citations = vec![
            with_issue(
                make_citation(
                    "Immune response study",
                    Some(2020),
                    Some("Journal"),
                    Some("12"),
                    None,
                    None,
                ),
                Some("Issue 4"),
            ),
            with_issue(
                make_citation(
                    "Immune response study",
                    Some(2020),
                    Some("Journal"),
                    Some("12"),
                    None,
                    None,
                ),
                Some("4"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_issue_mismatch_does_not_help_when_volume_and_page_are_missing() {
        let citations = vec![
            with_issue(
                make_citation(
                    "Immune response study",
                    Some(2020),
                    Some("Journal"),
                    None,
                    None,
                    None,
                ),
                Some("4"),
            ),
            with_issue(
                make_citation(
                    "Immune response study",
                    Some(2020),
                    Some("Journal"),
                    None,
                    None,
                    None,
                ),
                Some("5"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
    }

    #[test]
    fn test_volume_only_fallback_requires_explicit_same_year() {
        let citations = vec![
            make_citation(
                "Immune response study",
                None,
                Some("Journal"),
                Some("12"),
                None,
                None,
            ),
            make_citation(
                "Immune response study",
                None,
                Some("Journal"),
                Some("12"),
                None,
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
    }

    #[test]
    fn test_no_doi_rejects_when_issue_and_page_conflict_despite_volume_match() {
        let citations = vec![
            with_issue(
                make_citation(
                    "Shared study title",
                    Some(2020),
                    Some("Journal"),
                    Some("8"),
                    Some("100-110"),
                    None,
                ),
                Some("1"),
            ),
            with_issue(
                make_citation(
                    "Shared study title",
                    Some(2020),
                    Some("Journal"),
                    Some("8"),
                    Some("120-130"),
                    None,
                ),
                Some("2"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
    }

    #[test]
    fn test_no_doi_volume_only_fallback_allows_missing_issue_and_page() {
        let citations = vec![
            with_issue(
                make_citation(
                    "Shared study title",
                    Some(2020),
                    Some("Journal"),
                    Some("8"),
                    None,
                    None,
                ),
                Some("1"),
            ),
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                Some("8"),
                None,
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_no_doi_matches_with_volume_and_issue_when_page_missing() {
        let citations = vec![
            with_issue(
                make_citation(
                    "Shared study title",
                    Some(2020),
                    Some("Journal"),
                    Some("8"),
                    None,
                    None,
                ),
                Some("1"),
            ),
            with_issue(
                make_citation(
                    "Shared study title",
                    Some(2020),
                    Some("Journal"),
                    Some("8"),
                    None,
                    None,
                ),
                Some("Issue 1"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_no_doi_rejects_when_pages_conflict() {
        let citations = vec![
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                None,
                Some("100-110"),
                None,
            ),
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                None,
                Some("120-130"),
                None,
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
    }

    #[test]
    fn test_no_doi_obvious_exact_duplicate_still_matches() {
        let citations = vec![
            with_issue(
                make_citation(
                    "Shared study title",
                    Some(2020),
                    Some("Journal"),
                    Some("8"),
                    Some("100-110"),
                    None,
                ),
                Some("1"),
            ),
            with_issue(
                make_citation(
                    "Shared study title",
                    Some(2020),
                    Some("Journal"),
                    Some("8"),
                    Some("100-110"),
                    None,
                ),
                Some("1"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_metadata_precheck_preserves_exact_title_fallback_without_journal() {
        let citations = vec![
            make_citation(
                "Shared study title",
                Some(2020),
                None,
                Some("8"),
                Some("100-110"),
                None,
            ),
            make_citation(
                "Shared study title",
                Some(2020),
                None,
                Some("8"),
                Some("100-110"),
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
    fn test_series_guard_blocks_both_doi_matches_without_page_agreement() {
        let citations = vec![
            make_citation(
                "Study part 1",
                Some(2020),
                Some("Journal"),
                Some("5"),
                Some("100-110"),
                Some("10.1000/part1"),
            ),
            make_citation(
                "Study part 2",
                Some(2020),
                Some("Journal"),
                Some("5"),
                Some("200-210"),
                Some("10.1000/part2"),
            ),
        ];

        let sim = strsim::jaro(
            &Deduplicator::normalize_string(&citations[0].title),
            &Deduplicator::normalize_string(&citations[1].title),
        );
        assert!(sim < 0.99, "unexpected both-doi series sim {sim}");

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
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
                Some("10-12"),
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
    fn test_boilerplate_prefix_aware_title_similarity_can_recover_match() {
        let citations = vec![
            make_citation(
                "Brief report. Kidney injury markers",
                Some(2020),
                Some("Journal"),
                Some("7"),
                None,
                None,
            ),
            make_citation(
                "Kidney injury markers",
                Some(2020),
                Some("Journal"),
                Some("7"),
                None,
                None,
            ),
        ];

        let deduplicator = Deduplicator::new();
        let records = deduplicator.preprocess_citations(&citations);
        let raw_similarity = strsim::jaro_winkler(&records[0].norm_title, &records[1].norm_title);
        let effective_similarity =
            Deduplicator::max_title_similarity(&records[0], &records[1], strsim::jaro_winkler);
        assert!(
            raw_similarity < NO_DOI_TITLE_SIMILARITY_THRESHOLD,
            "expected raw similarity below threshold, got {raw_similarity}"
        );
        assert!(
            effective_similarity >= NO_DOI_TITLE_SIMILARITY_THRESHOLD,
            "expected stripped-title similarity to recover the match, got {effective_similarity}"
        );

        let groups = deduplicator.find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
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
                "Renal biomarker observational studies",
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
            (PASS1_DOI_TITLE_SANITY..0.99).contains(&sim),
            "unexpected pass1 sim {sim}"
        );

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_pass1_same_doi_rescue_with_strong_corroboration() {
        let citations = vec![
            make_citation_with_author_and_doi(
                "Renal biomarker study in adults",
                "Smith",
                Some(2020),
                Some("Journal"),
                Some("12"),
                Some("100-110"),
                Some("10.1000/rescue"),
            ),
            make_citation_with_author_and_doi(
                "Observational study of renal biomarkers",
                "Smith",
                Some(2021),
                Some("Journal"),
                Some("12"),
                Some("100-118"),
                Some("10.1000/rescue"),
            ),
        ];

        let sim = strsim::jaro(
            &Deduplicator::normalize_string(&citations[0].title),
            &Deduplicator::normalize_string(&citations[1].title),
        );
        assert!(
            sim < PASS1_DOI_TITLE_SANITY,
            "expected rescue similarity below pass1 sanity threshold, got {sim}"
        );

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_pass1_same_doi_rescue_requires_no_author_conflict() {
        let citations = vec![
            make_citation_with_author_and_doi(
                "Renal biomarker study in adults",
                "Smith",
                Some(2020),
                Some("Journal"),
                Some("12"),
                Some("100-110"),
                Some("10.1000/rescue-author"),
            ),
            make_citation_with_author_and_doi(
                "Observational study of renal biomarkers",
                "Jones",
                Some(2021),
                Some("Journal"),
                Some("12"),
                Some("100-118"),
                Some("10.1000/rescue-author"),
            ),
        ];

        let sim = strsim::jaro(
            &Deduplicator::normalize_string(&citations[0].title),
            &Deduplicator::normalize_string(&citations[1].title),
        );
        assert!(
            sim < PASS1_DOI_TITLE_SANITY,
            "expected rescue similarity below pass1 sanity threshold, got {sim}"
        );

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
    }

    #[test]
    fn test_pass1_same_doi_rescue_requires_page_corroboration() {
        let citations = vec![
            make_citation_with_author_and_doi(
                "Renal biomarker study in adults",
                "Smith",
                Some(2020),
                Some("Journal"),
                Some("12"),
                None,
                Some("10.1000/rescue-metadata"),
            ),
            make_citation_with_author_and_doi(
                "Observational study of renal biomarkers",
                "Smith",
                Some(2021),
                Some("Journal"),
                Some("12"),
                None,
                Some("10.1000/rescue-metadata"),
            ),
        ];

        let sim = strsim::jaro(
            &Deduplicator::normalize_string(&citations[0].title),
            &Deduplicator::normalize_string(&citations[1].title),
        );
        assert!(
            sim < PASS1_DOI_TITLE_SANITY,
            "expected rescue similarity below pass1 sanity threshold, got {sim}"
        );

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
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
    fn test_conflicting_normalized_dois_are_not_duplicates() {
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
            (0.85..0.99).contains(&sim),
            "unexpected conflicting-doi sim {sim}"
        );

        assert!(
            Deduplicator::new()
                .find_duplicates(&citations)
                .iter()
                .all(|group| group.duplicates.is_empty())
        );
    }

    #[test]
    fn test_bracketed_translated_title_can_match_with_strict_metadata() {
        let citations = vec![
            with_issue(
                with_issn(
                    make_citation_with_author(
                        "[Translated title of the study]",
                        "Keyriläinen",
                        Some(2020),
                        Some("Journal"),
                        Some("5"),
                        Some("100-110"),
                    ),
                    &["1234-5678"],
                ),
                Some("2"),
            ),
            with_issue(
                with_issn(
                    make_citation_with_author(
                        "Titulo original del estudio",
                        "Keyrilainen",
                        Some(2020),
                        Some("Journal"),
                        Some("5"),
                        Some("100-110"),
                    ),
                    &["1234-5678"],
                ),
                Some("2"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates, vec![1]);
    }

    #[test]
    fn test_bracketed_translated_title_requires_full_metadata_agreement() {
        let citations = vec![
            with_issue(
                make_citation_with_author(
                    "[Translated title of the study]",
                    "Keyriläinen",
                    Some(2020),
                    Some("Journal"),
                    Some("5"),
                    Some("100-110"),
                ),
                Some("2"),
            ),
            with_issue(
                make_citation_with_author(
                    "Titulo original del estudio",
                    "Keyrilainen",
                    Some(2020),
                    Some("Journal"),
                    Some("5"),
                    Some("100-110"),
                ),
                Some("3"),
            ),
        ];

        let groups = Deduplicator::new().find_duplicates(&citations);

        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.duplicates.is_empty()));
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
}
