## Spec: Rewrite of `biblib` deduplication engine (`src/dedupe.rs`)

### Context

- Project: `biblib`, Rust citation parsing/dedup library. Dedup lives in `src/dedupe.rs`; shared normalization helpers in `src/utils.rs` (`format_doi`, `format_page_numbers`); public types `Citation`, `Date`, `DuplicateGroup` in `src/lib.rs`; docs in `docs/deduplication-guide.md`.
- Parsers already normalize DOIs and pages via `utils::format_doi` / `utils::format_page_numbers`, but `Citation` is publicly constructible, so dedup must defensively re-normalize (both functions are idempotent).
- This is a breaking change. Target version **0.8.0**. Update `Cargo.toml` and `CHANGELOG.md`.
- Branch: `dedupe-engine-rewrite`. Commit in logical units: (1) preprocessing/normalization, (2) union-find + engine, (3) API/builder, (4) tests, (5) docs.

### Part 1: New public API

1. **Index-based `DuplicateGroup`**. Define both types in `src/dedupe.rs`; keep the existing re-export path so `biblib::DuplicateGroup` points to the new type:

```rust
pub struct DuplicateGroup {
    /// Index into the input slice of the citation selected as unique.
    pub unique: usize,
    /// Indices of the duplicates of `unique`, sorted ascending.
    pub duplicates: Vec<usize>,
}

pub struct OwnedDuplicateGroup {
    pub unique: Citation,
    pub duplicates: Vec<Citation>,
}
```

2. **Methods take `&self` and are infallible**:

```rust
impl Deduplicator {
    pub fn builder() -> DeduplicatorBuilder;
    pub fn new() -> Self; // = builder().build()
    pub fn find_duplicates(&self, citations: &[Citation]) -> Vec<DuplicateGroup>;
    pub fn find_duplicates_with_sources(&self, citations: &[Citation], sources: &[&str]) -> Vec<DuplicateGroup>;
    pub fn find_duplicates_cloned(&self, citations: &[Citation]) -> Vec<OwnedDuplicateGroup>;
    pub fn find_duplicates_with_sources_cloned(&self, citations: &[Citation], sources: &[&str]) -> Vec<OwnedDuplicateGroup>;
}
```

- Remove `DedupeError` entirely. If `sources.len() > citations.len()`, ignore the extras (document it). Missing trailing sources = no source.

3. **Builder replaces `DeduplicatorConfig`** (delete `DeduplicatorConfig`, `with_config`, `group_by_year`, `run_in_parallel`):

```rust
Deduplicator::builder()
    .year_tolerance(1)            // u8, default 1
    .parallel(false)              // default false
    .source_preferences(iter)     // default empty
    .doi_title_threshold(0.85)    // default
    .no_doi_title_threshold(0.93) // default
    .exact_title_threshold(0.99)  // default
    .build()
```

- `build()` **panics** with a clear message if any threshold is outside `(0.0, 1.0]`, or if `doi_title_threshold > exact_title_threshold`, or `no_doi_title_threshold > exact_title_threshold`.

4. **Determinism guarantee**: groups sorted by smallest member index; `duplicates` sorted ascending; every input index appears in exactly one group (singletons have empty `duplicates`).

### Part 2: Engine rewrite (internal)

Delete the greedy loop, all `*const Citation` pointer maps, and the `source_map` HashMap.

**Internal named constants** (with explanatory comments, NOT in the builder):

```rust
const PASS1_DOI_TITLE_SANITY: f64 = 0.70;
const AUTHOR_GUARD_THRESHOLD: f64 = 0.96;
```

**Step A: Preprocessing (never errors).** Build per citation:

```rust
struct DedupRecord {
    idx: usize,
    norm_title: String,              // may be empty
    norm_doi: Option<String>,        // via utils::format_doi
    norm_journal: Option<String>,    // None if empty AFTER normalization
    norm_journal_abbr: Option<String>,
    norm_issns: Vec<String>,
    norm_volume: Option<String>,
    norm_start_page: Option<String>,
    year: Option<i32>,
    first_author_key: Option<String>,
}
```

- **Title**: keep existing pipeline (`convert_unicode_string` → lowercase → HTML entity/tag removal → alphanumeric filter) with fixes: (a) remove substring replacements `("beta","b")` and `("alpha","a")` from `HTML_REPLACEMENTS`; keep only Greek chars `α→a, ß→b, γ→g` and add `β→b`; (b) apply Unicode NFKD normalization + strip combining marks (crate `unicode-normalization`, optional dependency enabled by the `dedupe` feature) before the alphanumeric filter. Empty input → empty string, never an error.
- **DOI**: `utils::format_doi` on the raw value.
- **Journal / abbr**: keep `format_journal_name`; map empty-string results to `None`.
- **Start page**: `utils::format_page_numbers` on `pages`, take segment before first `-`, strip non-alphanumerics, lowercase; empty → `None`.
- **Volume**: keep `normalize_volume`; empty → `None`.
- **ISSN**: keep `format_issn`, additionally reject `X` anywhere except the last position.
- **first_author_key**: lowercase alphanumeric-only `Author.name` (surname) of the first author; `None` if absent/empty.

**Step B: Union-Find.** Private disjoint-set over `0..n` (path compression + union by rank). All match decisions call `union(i, j)`.

**Step C: Pass 1, exact DOI clustering, O(n).** `HashMap` keyed by `norm_doi`. Within each bucket, union every pair with `jaro(title_a, title_b) >= PASS1_DOI_TITLE_SANITY`. If either normalized title is empty, replace the title guard with: journal OR ISSN OR volume OR start-page agreement. Add `// TODO: extend to PMID / accession_number keys`.

**Step D: Pass 2, blocked fuzzy matching.** Records with empty `norm_title` are excluded (they may already be matched in Pass 1).

- **Blocking (derived from config)**: each record with year `y` joins blocks `y ..= y + year_tolerance` (with `year_tolerance(0)`, only block `y`). Records with `year: None` join a dedicated `unknown` block whose members are additionally compared against every year block. Do NOT sub-block oversized blocks in this version; add `// TODO: sorted-token fingerprint sub-blocking for very large year blocks. The unknown-year block is compared against every year block: O(u × n) if a source systematically lacks dates.`.
- **Predicates** per pair:
  - `journal_match`: existing 4-way full/abbr cross-comparison on `Option`-cleaned values (`Some("")` can no longer occur).
  - `issn_match`: any common normalized ISSN.
  - `volume_match` / `page_match`: both `Some` and equal (pages via `norm_start_page`).
  - `year_compatible`: both `None` → **true**; exactly one `None` → **true**; both `Some` → `|y1 - y2| <= year_tolerance`.
- **Criteria**:
  - **Both have DOIs** (necessarily different or Pass 1-rejected): `jaro`; duplicate iff `sim >= exact_title_threshold && year_compatible && (volume_match || page_match) && (journal_match || issn_match)`.
  - **At least one DOI missing**: `jaro_winkler`; duplicate iff `(sim >= no_doi_title_threshold && year_compatible && (volume_match || page_match) && (journal_match || issn_match))` OR `(sim >= exact_title_threshold && year_compatible && volume_match && page_match)`.
- **Series/erratum guard** (applies to any Jaro-Winkler match with `sim < exact_title_threshold`): strip the longest common prefix of the two normalized titles; if both remainders (one may be empty) consist solely of ASCII digits or roman-numeral characters (`i v x l c d m`) and at least one remainder is non-empty, require `page_match` to union.
- **Author guard**: if both `first_author_key` are `Some` and differ, and `sim < AUTHOR_GUARD_THRESHOLD`, do not union.
- **Parallelism**: when `parallel = true`, evaluate pairs per block via rayon, collecting matches into `Vec<(usize, usize)>`, then apply unions sequentially. The "skip pairs already in the same set" optimization uses a union-find snapshot taken after Pass 1 (no concurrent mutation).

**Step E: Group assembly.** Union-find components → groups of indices. Unique selection per group: (1) Iterate `source_preferences` in order (outer loop); for each preference, scan group indices ascending (inner loop); return the first hit; (2) else prefer non-empty trimmed `abstract_text`; (3) among those, prefer non-empty DOI; (4) else lowest index. Sort per Part 1.

### Part 3: Tests

Port all existing tests in `src/dedupe.rs` to the new API (index-based assertions). Add:

1. Empty-title citation → run succeeds; record is a singleton (unless DOI-matched).
2. Both journals `Some("")` → NOT a journal match.
3. DOI variants `"https://doi.org/10.1000/X"` vs `"10.1000/x"` (manually built `Citation`s) → duplicates.
4. Truncated pages `"1234-45"` vs `"1234-1245"` → page match.
5. No-DOI records with identical title, journal, and volume; years 2020 vs 2021 → duplicates at default tolerance; NOT duplicates with `year_tolerance(0)`.
6. `year_tolerance(2)`: no-DOI pair 2 years apart with matching journal+volume → duplicates (verifies blocking derives from config).
7. No-DOI records with identical title, journal, and volume; both years `None` → duplicates.
8. Transitivity: A~B, B~C, A vs C alone wouldn't match → one group of 3.
9. Series guard: `"...part 1"` vs `"...part 2"` (same journal/volume/year, different pages, no DOIs) → NOT duplicates; `"...part ii"` vs `"...part iii"` → NOT duplicates; identical trailing-year titles → guard does not trigger.
10. Author guard: borderline no-DOI match (0.93-0.96) with different first-author surnames → NOT duplicates.
11. Same DOI, no journal/volume/pages, title sim ~0.8 → duplicates (Pass 1).
12. Determinism: shuffled input → same groups (modulo index mapping); repeated runs → identical output.
13. `parallel(true)` ≡ `parallel(false)` on a 100+ record fixture.
14. Abstract preference ignores `Some("")`.
15. Sources: preferences select correct unique; shorter `sources` works; longer is tolerated (no error).
16. Every input index appears exactly once across all groups.
17. Builder panics: threshold `0.0`, threshold `1.5`, `doi_title_threshold(0.995)` with default `exact_title_threshold`.

Gates: `cargo test` (incl. doctests — update every doc example in `src/dedupe.rs` and `src/lib.rs` to the new API), `cargo clippy -- -D warnings`, `cargo fmt --check`.

### Part 4: Docs and release

1. Rewrite `src/dedupe.rs` module docs: new API examples, two-pass algorithm, criteria tables, builder options, determinism guarantee.
2. Rewrite `docs/deduplication-guide.md` to match, including migration notes from 0.7 (old owned groups → `find_duplicates_cloned`).
3. Update dedup examples in `src/lib.rs` crate docs and `README.md` if present.
4. `Cargo.toml`: add `unicode-normalization` as optional, enabled by the `dedupe` feature; bump version to `0.8.0`.
5. `CHANGELOG.md` entry:
   - **Breaking**: index-based `DuplicateGroup`; methods take `&self` and are infallible; removed `DedupeError`, `DeduplicatorConfig`, `with_config`, `group_by_year`, `run_in_parallel`; builder-based configuration; `sources` longer than `citations` is now tolerated instead of erroring. `DuplicateGroup` is now gated behind the dedupe feature.
   - **Changed**: year tolerance with cross-year matching (default ±1); defensive DOI/page normalization; empty titles no longer abort the run; empty-string journals no longer match; transitive union-find clustering; deterministic output ordering.
   - **Added**: configurable thresholds; series/erratum guard; first-author guard; `OwnedDuplicateGroup` + `_cloned` methods.

### Constraints

- Do not change any parser modules or `utils.rs` behavior except as noted (reuse only).
- Keep everything under the existing `dedupe` feature flag.
- No temporary files, no extra summary documents.
- commit in logical units (preprocessing/normalization, union-find + engine, API/builder, tests, docs).

---
