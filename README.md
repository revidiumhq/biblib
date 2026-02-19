# biblib

[![Crates.io](https://img.shields.io/crates/v/biblib.svg)](https://crates.io/crates/biblib)
[![Documentation](https://docs.rs/biblib/badge.svg)](https://docs.rs/biblib)
[![License](https://img.shields.io/crates/l/biblib.svg)](LICENSE-MIT)

A Rust library for parsing and deduplicating academic citations.

## Installation

```toml
[dependencies]
biblib = "0.3.0"
```

For minimal builds:

```toml
[dependencies]
biblib = { version = "0.3.2", default-features = false, features = ["ris", "regex"] }
```

## Supported Formats

| Format      | Feature  | Description                         |
| ----------- | -------- | ----------------------------------- |
| RIS         | `ris`    | Research Information Systems format |
| PubMed      | `pubmed` | MEDLINE/PubMed `.nbib` files        |
| EndNote XML | `xml`    | EndNote XML export format           |
| CSV         | `csv`    | Configurable delimited files        |

All format features are enabled by default.

## Quick Start

### Parsing Citations

```rust
use biblib::{CitationParser, RisParser};

let ris_content = r#"TY  - JOUR
TI  - Machine Learning in Healthcare
AU  - Smith, John
AU  - Doe, Jane
PY  - 2023
ER  -"#;

let parser = RisParser::new();
let citations = parser.parse(ris_content).unwrap();

println!("Title: {}", citations[0].title);
println!("Authors: {:?}", citations[0].authors);
```

### Auto-Detecting Format

```rust
use biblib::detect_and_parse;

let content = "TY  - JOUR\nTI  - Example\nER  -";
let (citations, format) = detect_and_parse(content).unwrap();

println!("Detected format: {}", format); // "RIS"
```

### Deduplicating Citations

```rust
use biblib::dedupe::{Deduplicator, DeduplicatorConfig};

let config = DeduplicatorConfig {
    group_by_year: true,      // Group by year for performance
    run_in_parallel: true,    // Use parallel processing
    source_preferences: vec!["PubMed".to_string()], // Prefer PubMed records
};

let deduplicator = Deduplicator::new().with_config(config);
let groups = deduplicator.find_duplicates(&citations).unwrap();

for group in groups {
    if !group.duplicates.is_empty() {
        println!("Kept: {}", group.unique.title);
        println!("Duplicates: {}", group.duplicates.len());
    }
}
```

### CSV with Custom Headers

```rust
use biblib::csv::{CsvParser, CsvConfig};
use biblib::CitationParser;

let mut config = CsvConfig::new();
config
    .set_delimiter(b';')
    .set_header_mapping("title", vec!["Article Name".to_string()])
    .set_header_mapping("authors", vec!["Writers".to_string()]);

let parser = CsvParser::with_config(config);
let citations = parser.parse("Article Name;Writers\nMy Paper;Smith J").unwrap();
```

## Citation Fields

Each parsed citation contains:

| Field           | Type             | Description                                 |
| --------------- | ---------------- | ------------------------------------------- |
| `title`         | `String`         | Work title                                  |
| `authors`       | `Vec<Author>`    | Authors with name, given name, affiliations |
| `journal`       | `Option<String>` | Full journal name                           |
| `journal_abbr`  | `Option<String>` | Journal abbreviation                        |
| `date`          | `Option<Date>`   | Year, month, day                            |
| `volume`        | `Option<String>` | Volume number                               |
| `issue`         | `Option<String>` | Issue number                                |
| `pages`         | `Option<String>` | Page range                                  |
| `doi`           | `Option<String>` | Digital Object Identifier                   |
| `pmid`          | `Option<String>` | PubMed ID                                   |
| `pmc_id`        | `Option<String>` | PubMed Central ID                           |
| `issn`          | `Vec<String>`    | ISSNs                                       |
| `abstract_text` | `Option<String>` | Abstract                                    |
| `keywords`      | `Vec<String>`    | Keywords                                    |
| `urls`          | `Vec<String>`    | Related URLs                                |
| `mesh_terms`    | `Vec<String>`    | MeSH terms (PubMed)                         |
| `extra_fields`  | `HashMap`        | Additional format-specific fields           |

## Features

| Feature  | Dependencies      | Description                        |
| -------- | ----------------- | ---------------------------------- |
| `ris`    | -                 | RIS format parser                  |
| `pubmed` | -                 | PubMed/MEDLINE parser              |
| `xml`    | `quick-xml`       | EndNote XML parser                 |
| `csv`    | `csv`             | CSV parser                         |
| `dedupe` | `rayon`, `strsim` | Deduplication engine               |
| `regex`  | `regex`           | Full regex support                 |
| `lite`   | `regex-lite`      | Lightweight regex (smaller binary) |

Default: all features enabled except `lite`.

> **Note:** At least one of `regex` or `lite` must always be enabled — the crate will not compile without one of them. They are mutually exclusive; do not enable both.

## Documentation

- **[Parsing Guide](PARSING_GUIDE.md)** — Format-specific tag mappings, date formats, and author handling
- **[Deduplication Guide](DEDUPLICATION_GUIDE.md)** — Matching algorithm, similarity thresholds, and configuration
- **[API Docs](https://docs.rs/biblib)** — Complete API reference

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
