# Deduplication Guide

This guide documents the deduplication algorithm, matching criteria, configuration options, and performance characteristics in `biblib`.

## Table of Contents

- [Overview](#overview)
- [Matching Algorithm](#matching-algorithm)
- [Configuration](#configuration)
- [Normalization](#normalization)
- [Performance](#performance)
- [Source Preferences](#source-preferences)

---

## Overview

The deduplicator identifies duplicate citations by comparing multiple fields using fuzzy string matching and exact field comparisons. It groups duplicates together and selects one "unique" citation from each group.

### Basic Usage

```rust
use biblib::dedupe::{Deduplicator, DeduplicatorConfig};

let config = DeduplicatorConfig {
    group_by_year: true,
    run_in_parallel: true,
    source_preferences: vec!["PubMed".to_string()],
};

let deduplicator = Deduplicator::new().with_config(config);
let groups = deduplicator.find_duplicates(&citations).unwrap();
```

---

## Matching Algorithm

### With DOI Present

When both citations have DOIs, matching uses the **Jaro** similarity algorithm:

| Condition | Required |
|-----------|----------|
| Title similarity | ≥ 0.85 |
| DOI match | Yes |
| Journal or ISSN match | Yes |

**Alternative criteria** (same DOI):
- Title similarity ≥ 0.99 AND (volume OR pages match)

**Different DOIs** (still may match):
- Title similarity ≥ 0.99 AND year match AND (volume OR pages match) AND (journal OR ISSN match)

### Without DOI

When DOIs are missing or empty, matching uses **Jaro-Winkler** with stricter thresholds:

| Condition | Required |
|-----------|----------|
| Title similarity | ≥ 0.93 |
| Volume or pages match | Yes |
| Journal or ISSN match | Yes |

**Alternative criteria**:
- Title similarity ≥ 0.99 AND year match AND volume AND pages match

### Why Different Algorithms?

- **Jaro**: Used with DOIs because the DOI already provides high confidence; looser title matching is acceptable
- **Jaro-Winkler**: Weights prefix matches more heavily, useful for catching title variations without DOI confirmation

---

## Configuration

### DeduplicatorConfig Options

```rust
pub struct DeduplicatorConfig {
    pub group_by_year: bool,
    pub run_in_parallel: bool,
    pub source_preferences: Vec<String>,
}
```

| Option | Default | Description |
|--------|---------|-------------|
| `group_by_year` | `true` | Group citations by year before comparing |
| `run_in_parallel` | `false` | Use Rayon for parallel processing |
| `source_preferences` | `[]` | Ordered list of preferred sources |

### Important Notes

- `run_in_parallel` is **ignored** if `group_by_year` is false
- Year grouping is recommended for datasets > 1000 citations
- Parallel processing requires the `dedupe` feature

---

## Normalization

Before comparison, all fields are normalized to improve matching accuracy.

### Title Normalization

1. Convert Unicode escape sequences (e.g., `<U+00E9>` → `é`)
2. Replace HTML entities (`&lt;` → `<`, etc.)
3. Remove HTML tags (`<sup>`, `<sub>`, etc.)
4. Replace Greek letters with ASCII equivalents:
   - `α` → `a`, `β/ß` → `b`, `γ` → `g`
5. Convert to lowercase
6. Remove all non-alphanumeric characters

**Example:**
```
"Machine Learning: A β-test <sup>2</sup>" 
→ "machinelearningabtest2"
```

### Journal Normalization

1. Strip ". Conference" suffix and anything after
2. Convert to lowercase
3. Remove all non-alphanumeric characters

### Volume Normalization

1. Find first sequence of digits
2. Extract only the numeric portion

**Example:** `"Vol. 23 (Suppl)"` → `"23"`

### ISSN Normalization

1. Strip common suffixes like "(Print)", "(Electronic)", "(Linking)"
2. Keep only the ISSN pattern (e.g., `1234-5678`)

### Journal Matching

Two journals are considered matching if **any** of these are true:
- Both full names match (after normalization)
- Both abbreviations match
- One's full name matches the other's abbreviation
- One's abbreviation matches the other's full name

---

## Performance

### Time Complexity

| Configuration | Complexity |
|---------------|------------|
| No year grouping | O(n²) |
| With year grouping | O(Σ n_y²) |
| With parallel + year | Same but parallelized |

Where `n` = total citations, `n_y` = citations per year.

### Recommendations

| Dataset Size | Recommended Configuration |
|--------------|---------------------------|
| < 100 | Any configuration works |
| 100-1000 | Enable `group_by_year` |
| > 1000 | Enable both `group_by_year` and `run_in_parallel` |

### Memory Usage

Each citation is preprocessed once, storing:
- Normalized title
- Normalized journal name
- Normalized journal abbreviation
- Normalized volume
- Normalized ISSNs

---

## Source Preferences

When multiple sources provide the same citation, you can specify which source's version to keep.

### Configuration

```rust
let config = DeduplicatorConfig {
    source_preferences: vec![
        "PubMed".to_string(),
        "Embase".to_string(),
        "CrossRef".to_string(),
    ],
    ..Default::default()
};
```

### Usage

```rust
let citations = vec![/* ... */];
let sources = vec!["Embase", "PubMed"];

let groups = deduplicator
    .find_duplicates_with_sources(&citations, &sources)
    .unwrap();
```

### Selection Logic

When selecting the "unique" citation from a duplicate group:

1. **First**: Check source preferences (in order)
2. **Second**: Prefer citations with abstracts
3. **Third**: Among those with abstracts, prefer ones with DOIs
4. **Fallback**: Use first citation in group

---

## Similarity Thresholds

| Scenario | Algorithm | Threshold |
|----------|-----------|-----------|
| With DOI + journal/ISSN | Jaro | 0.85 |
| With DOI without journal | Jaro | 0.99 |
| Without DOI + journal/ISSN | Jaro-Winkler | 0.93 |
| Without DOI without journal | Jaro-Winkler | 0.99 |

These thresholds were tuned to balance precision (avoiding false positives) and recall (catching true duplicates).
