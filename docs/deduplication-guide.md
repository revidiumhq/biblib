# Deduplication Guide

This guide describes the deduplication API in `biblib 0.8`, how the matching
engine works, and what changed from the 0.7 series.

## Overview

The deduplicator works on slices of [`Citation`] values and returns
index-based duplicate groups. Every input index appears exactly once in the
output. Duplicate groups are deterministic:

- Groups are sorted by their smallest member index.
- Each group's `duplicates` vector is sorted ascending.
- Singletons are kept as groups with an empty `duplicates` list.

## Basic Usage

```rust
use biblib::dedupe::Deduplicator;
use biblib::{Citation, Date};

let citations = vec![
    Citation {
        title: "Example Title".to_string(),
        doi: Some("10.1000/example".to_string()),
        date: Some(Date {
            year: 2023,
            month: None,
            day: None,
        }),
        journal: Some("Example Journal".to_string()),
        ..Default::default()
    },
    Citation {
        title: "Example Title".to_string(),
        doi: Some("10.1000/example".to_string()),
        date: Some(Date {
            year: 2023,
            month: None,
            day: None,
        }),
        journal: Some("Example Journal".to_string()),
        ..Default::default()
    },
];

let groups = Deduplicator::new().find_duplicates(&citations);

assert_eq!(groups.len(), 1);
assert_eq!(groups[0].unique, 0);
assert_eq!(groups[0].duplicates, vec![1]);
```

## Source-Aware Deduplication

`find_duplicates_with_sources()` accepts a parallel slice of source names. If
`sources` is shorter than `citations`, the trailing citations are treated as
having no source. If `sources` is longer, the extra entries are ignored.

```rust
use biblib::dedupe::Deduplicator;
use biblib::Citation;

let citations = vec![
    Citation {
        title: "Example Title".to_string(),
        doi: Some("10.1000/example".to_string()),
        ..Default::default()
    },
    Citation {
        title: "Example Title".to_string(),
        doi: Some("10.1000/example".to_string()),
        ..Default::default()
    },
];

let sources = vec!["Embase", "PubMed"];

let groups = Deduplicator::builder()
    .source_preferences(["PubMed", "Embase"])
    .build()
    .find_duplicates_with_sources(&citations, &sources);

assert_eq!(groups[0].unique, 1);
assert_eq!(groups[0].duplicates, vec![0]);
```

## Owned Results

If you want the previous owned-group shape from the 0.7 series, use the
`*_cloned` methods:

```rust
use biblib::dedupe::Deduplicator;

let groups = Deduplicator::new().find_duplicates_cloned(&[]);
assert!(groups.is_empty());
```

## Builder Options

`Deduplicator` is configured through `Deduplicator::builder()`:

```rust
use biblib::dedupe::Deduplicator;

let deduplicator = Deduplicator::builder()
    .year_tolerance(1)
    .parallel(false)
    .source_preferences(["PubMed", "CrossRef"])
    .no_doi_title_threshold(0.93)
    .exact_title_threshold(0.99)
    .build();

let _ = deduplicator;
```

### Defaults

| Option | Default | Meaning |
| --- | --- | --- |
| `year_tolerance` | `1` | Cross-year matching window for fuzzy matching |
| `parallel` | `false` | Evaluate pass-2 blocks with Rayon |
| `source_preferences` | `[]` | Source priority for choosing `unique` |
| `no_doi_title_threshold` | `0.93` | Fuzzy threshold when at least one DOI is missing |
| `exact_title_threshold` | `0.99` | High-confidence title threshold |

### Builder Validation

`build()` panics with a clear message when:

- Any threshold is outside `(0.0, 1.0]`
- `no_doi_title_threshold > exact_title_threshold`

## Matching Engine

The deduplicator uses a two-pass engine.

### Pass 1: DOI Clustering

Records with the same normalized DOI are bucketed together and compared in
O(n) DOI-bucket time.

| Condition | Required |
| --- | --- |
| DOI match | Yes |
| Title similarity | `jaro >= 0.90`, or a stricter corroborated rescue path |

If either normalized title is empty, the title guard is replaced with metadata
agreement on any of:

- Journal match
- ISSN match
- Volume match
- Start-page match

If both normalized titles are non-empty but `jaro < 0.90`, pass 1 only
rescues the pair when all of the following hold:

- No first-author disagreement when both author keys are present
- `journal_match OR issn_match`
- `page_match`
- The series/erratum guard does not reject the pair

### Pass 2: Blocked Fuzzy Matching

Records with empty normalized titles are excluded from pass 2. All other
records are placed into year-based blocks derived from `year_tolerance`.

#### Blocking

- A record with year `y` joins every block `y..=y + year_tolerance`
- A record with no year joins a dedicated unknown-year block
- The unknown-year block is also compared against every concrete year block

#### Pair Predicates

| Predicate | Meaning |
| --- | --- |
| `journal_match` | Full/full, abbr/abbr, full/abbr, or abbr/full match |
| `issn_match` | Any shared normalized ISSN |
| `volume_match` | Both normalized volumes are present and equal |
| `issue_match` | Both normalized issues are present and equal |
| `page_match` | Both normalized start pages are present and equal |
| `year_compatible` | `true` when years differ by at most `year_tolerance`, or either side is missing |

#### Metadata Prechecks

Before calculating Jaro or Jaro-Winkler title similarity, pass 2 evaluates the
required year and publication-metadata conditions. Pairs that cannot satisfy
any existing match path are rejected without an expensive fuzzy-title
comparison. This changes evaluation order only; it does not change thresholds
or matching criteria.

#### When Both Records Have Normalized DOIs

- Matching normalized DOIs are handled in pass 1.
- Conflicting normalized DOIs are treated as non-duplicates.

#### When At Least One DOI Is Missing

| Path | Required |
| --- | --- |
| Standard fuzzy path | `jaro_winkler >= no_doi_title_threshold` AND year compatible AND tiered publication evidence AND `(journal OR ISSN)` |
| Exact-title fallback | `jaro_winkler >= exact_title_threshold` AND year compatible AND volume match AND page match |

For the standard fuzzy path, publication evidence is evaluated conservatively:

- Strong: `page_match`
- Medium: `volume_match AND issue_match`
- Weak fallback: `volume_match AND same explicit year AND issue missing on at least one side AND page missing on at least one side`

Contradictory publication metadata is treated as negative evidence in the
no-DOI path:

- If both normalized start pages are present and unequal, the pair is rejected
- If both normalized issues are present and unequal while the volume matches,
  the pair is rejected

#### Bracketed Translated Titles

When at least one title is a fully bracketed translated-title record such as
`[Translated title ...]`, dedupe also allows a strict metadata-only path. This
path requires:

- `journal_match OR issn_match`
- Same year
- Same volume
- Same pages
- Same first-author key
- Same issue when both issues are present

#### Additional Guards

- Series/erratum guard: if a borderline title match is below
  `exact_title_threshold` and the differing suffixes are only digits or roman
  numerals, `page_match` is required.
- Author guard: if both records have a first-author key and they differ,
  matches below `AUTHOR_GUARD_THRESHOLD` are rejected.

### Group Assembly

After all unions are applied, components are turned into deterministic groups.
The `unique` member is selected in this order:

1. First matching source in `source_preferences`
2. First citation with non-empty trimmed `abstract_text`
3. Among those, first citation with a non-empty DOI
4. Lowest index

## Normalization

Deduplication defensively re-normalizes records even if they came from
`biblib` parsers.

### Title

The title pipeline is:

1. Convert `<U+XXXX>` Unicode escapes
2. Decode numeric HTML entities and a small named-entity set (`&amp;`, `&quot;`,
   `&alpha;`, `&beta;`, etc.)
3. Lowercase
4. Strip a leading English article (`the`, `a`, `an`)
5. Remove supported HTML tags
6. Remove explicit markup-style sub/sup tokens such as `[sub]` and `(sup)`
7. Expand Greek characters to word forms (`β`/`ß` -> `beta`, `α` -> `alpha`)
8. Apply Unicode NFKD normalization
9. Strip combining marks
10. Keep only alphanumeric characters

Empty titles normalize to the empty string and do not error.

### DOI

Raw DOI values are normalized with `utils::format_doi()`, so values like
`https://doi.org/10.1000/X`, `doi:%2010.1000%2Fx`, and `10.1000/x.` compare as
the same DOI. The cleanup is intentionally conservative: percent-decoding,
whitespace removal, `[doi]` removal, and trailing punctuation trimming.

### Pages

Page ranges are normalized with `utils::format_page_numbers()`, then the start
page is extracted, lowercased, and stripped to alphanumerics only. Known
Unicode dash variants such as `‐`, `‑`, `–`, `—`, `−`, `﹘`, `﹣`, and `－`
are normalized to ASCII `-` first. Placeholder values such as `N.PAG`,
`N.PAG-N.PAG`, `no pagination`, and `n/a` are treated as missing page data.

Examples:

- `1234-45` -> `1234`
- `1234-1245` -> `1234`

### Journal and Abbreviation

Journal names drop a leading `the `, normalize `&amp;` and `&` to `and`, and
then reduce to alphanumerics. Empty results are stored as `None`, so `Some("")`
never counts as a match.

### Author and Issue

First-author keys use lowercase alphanumerics after Unicode NFKD folding, so
accented and unaccented surname variants compare consistently. Issue values are
normalized into conservative lowercase alphanumeric keys and can act as an
additional metadata signal only when volume and page are missing and both
records have the same explicit year.

### ISSN

ISSNs are normalized into standard forms such as `1234-5678`. Values with `X`
outside the final position are rejected.

## Performance Notes

- Pass 1 is DOI-bucketed and linear in the number of DOI records per bucket.
- Pass 2 is block-based rather than a global greedy O(n²) scan.
- `parallel(true)` parallelizes pass-2 block evaluation only; unions are still
  applied sequentially for determinism.

## Migration from 0.7

### Duplicate Groups

`DuplicateGroup` is now index-based:

```rust
// 0.7
// group.unique: Citation
// group.duplicates: Vec<Citation>

// 0.8
// group.unique: usize
// group.duplicates: Vec<usize>
```

Use `find_duplicates_cloned()` or `find_duplicates_with_sources_cloned()` if
you want owned `Citation` values in the result.

### Configuration

The old `DeduplicatorConfig`, `with_config()`, `group_by_year`, and
`run_in_parallel` APIs were removed. Use the builder instead.

### Error Handling

Deduplication methods are now infallible. Overlong `sources` input is
tolerated and truncated logically instead of returning an error.


## Benchmarking

The repository includes a synthetic benchmark for the metadata-rejected fuzzy
workload optimized in `0.8.1`:

```console
cargo bench --bench dedupe
```

The default benchmark generates 750 near-title records whose publication
metadata prevents a match. Set `BIBLIB_BENCH_RECORDS` to change the input size.
The benchmark verifies that every record remains a singleton and reports
sequential and parallel median runtimes. It is intended for comparing builds
on the same machine, not as a cross-machine performance guarantee.
