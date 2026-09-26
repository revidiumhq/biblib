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
//! - DOI values are normalized (URL prefixes, percent-encoding and trailing
//!   `[doi]` noise are stripped)
//! - Page ranges go through page-range normalization, then compare on normalized
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

mod clustering;
mod matching;
mod normalize;
mod record;
#[cfg(test)]
mod tests;

const NO_DOI_TITLE_SIMILARITY_THRESHOLD: f64 = 0.93;
const PASS1_DOI_TITLE_SANITY: f64 = 0.90;
const AUTHOR_GUARD_THRESHOLD: f64 = 0.96;

/// Represents a group of duplicate citations using indices into the input slice.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DuplicateGroup {
    /// Index into the input slice of the citation selected as unique.
    pub unique: usize,
    /// Indices of the duplicates of `unique`, sorted ascending.
    pub duplicates: Vec<usize>,
}

/// Represents a group of duplicate citations as owned `Citation` values.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
}
