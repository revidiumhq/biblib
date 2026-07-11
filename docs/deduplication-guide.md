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
    .doi_title_threshold(0.85)
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
| `doi_title_threshold` | `0.85` | Stored builder threshold for DOI-title matching configuration |
| `no_doi_title_threshold` | `0.93` | Fuzzy threshold when at least one DOI is missing |
| `exact_title_threshold` | `0.99` | High-confidence title threshold |

### Builder Validation

`build()` panics with a clear message when:

- Any threshold is outside `(0.0, 1.0]`
- `doi_title_threshold > exact_title_threshold`
- `no_doi_title_threshold > exact_title_threshold`

## Matching Engine

The deduplicator uses a two-pass engine.

### Pass 1: DOI Clustering

Records with the same normalized DOI are bucketed together and compared in
O(n) DOI-bucket time.

| Condition | Required |
| --- | --- |
| DOI match | Yes |
| Title similarity | `jaro >= 0.70` |

If either normalized title is empty, the title guard is replaced with metadata
agreement on any of:

- Journal match
- ISSN match
- Volume match
- Start-page match

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
| `page_match` | Both normalized start pages are present and equal |
| `year_compatible` | `true` when years differ by at most `year_tolerance`, or either side is missing |

#### When Both Records Have DOIs

| Condition | Required |
| --- | --- |
| Similarity algorithm | `jaro` |
| Title similarity | `>= exact_title_threshold` |
| Year compatibility | Yes |
| Volume or page match | Yes |
| Journal or ISSN match | Yes |

#### When At Least One DOI Is Missing

| Path | Required |
| --- | --- |
| Standard fuzzy path | `jaro_winkler >= no_doi_title_threshold` AND year compatible AND `(volume OR page)` AND `(journal OR ISSN)` |
| Exact-title fallback | `jaro_winkler >= exact_title_threshold` AND year compatible AND volume match AND page match |

#### Additional Guards

- Series/erratum guard: if a Jaro-Winkler match is below
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
2. Lowercase
3. Remove supported HTML entities and tags
4. Replace selected Greek characters with ASCII fallbacks
5. Apply Unicode NFKD normalization
6. Strip combining marks
7. Keep only alphanumeric characters

Empty titles normalize to the empty string and do not error.

### DOI

Raw DOI values are normalized with `utils::format_doi()`, so values like
`https://doi.org/10.1000/X` and `10.1000/x` compare as the same DOI.

### Pages

Page ranges are normalized with `utils::format_page_numbers()`, then the start
page is extracted, lowercased, and stripped to alphanumerics only.

Examples:

- `1234-45` -> `1234`
- `1234-1245` -> `1234`

### Journal and Abbreviation

Journal names keep the existing normalization pipeline, but empty results are
stored as `None`, so `Some("")` never counts as a match.

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
