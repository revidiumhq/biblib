# biblib

[![Crates.io](https://img.shields.io/crates/v/biblib.svg)](https://crates.io/crates/biblib)
[![Documentation](https://docs.rs/biblib/badge.svg)](https://docs.rs/biblib)
[![License](https://img.shields.io/crates/l/biblib.svg)](LICENSE-MIT)

`biblib` is a Rust library for parsing citation exports into a shared data model and deduplicating the resulting records.

It is built for import pipelines, evidence synthesis tooling, registry ingestion, and any workflow that needs to turn citation files from multiple sources into one normalized `Citation` shape.

## What It Supports

`biblib` currently ships parsers for:

| Source format | Feature | Parser |
| --- | --- | --- |
| RIS | `ris` | `RisParser` |
| PubMed / MEDLINE (`.nbib`) | `pubmed` | `PubMedParser` |
| EndNote XML | `xml` | `EndNoteXmlParser` |
| EndNote Tagged / EndNote Web (`.enw`) | `enw` | `EnwParser` |
| BibTeX / BibLaTeX (`.bib`) | `bib` | `BibParser` |
| Generic CSV / delimited data | `csv` | `csv::CsvParser` |
| ICTRP registry CSV exports | `csv` | `IctrpCsvParser` |

All parser outputs converge on the same `Citation` struct, including normalized fields such as `title`, `authors`, `date`, `doi`, `accession_number`, `pmid`, `pmc_id`, `urls`, and `extra_fields`.

## Installation

```toml
[dependencies]
biblib = "0.6"
```

For a smaller build:

```toml
[dependencies]
biblib = { version = "0.6", default-features = false, features = ["ris"] }
```

## Quick Start

### Parse RIS

```rust
use biblib::{CitationParser, RisParser};

let input = r#"TY  - JOUR
TI  - Machine Learning in Healthcare
AU  - Smith, John
AU  - Doe, Jane
PY  - 2023
DO  - 10.1000/example
ER  -"#;

let citations = RisParser::new().parse(input).unwrap();

assert_eq!(citations.len(), 1);
assert_eq!(citations[0].title, "Machine Learning in Healthcare");
assert_eq!(citations[0].doi.as_deref(), Some("10.1000/example"));
```

### Parse PubMed / MEDLINE

```rust
use biblib::{CitationParser, PubMedParser};

let input = r#"PMID- 12345678
TI  - Immunotherapy in Oncology
FAU - Smith, John
JT  - Journal of Clinical Research
DP  - 2024 Jun 15
AB  - Example abstract."#;

let citations = PubMedParser::new().parse(input).unwrap();

assert_eq!(citations.len(), 1);
assert_eq!(citations[0].pmid.as_deref(), Some("12345678"));
assert_eq!(citations[0].title, "Immunotherapy in Oncology");
assert_eq!(citations[0].journal.as_deref(), Some("Journal of Clinical Research"));
```

### Parse EndNote Tagged (`.enw`)

```rust
use biblib::{CitationParser, EnwParser};

let input = r#"%0 Journal Article
%T Machine Learning in Healthcare
%A Smith, John
%D 2023
%R 10.1000/example
"#;

let citations = EnwParser::new().parse(input).unwrap();

assert_eq!(citations.len(), 1);
assert_eq!(citations[0].citation_type, vec!["Journal Article"]);
assert_eq!(citations[0].title, "Machine Learning in Healthcare");
assert_eq!(citations[0].doi.as_deref(), Some("10.1000/example"));
```

### Parse BibTeX / BibLaTeX (`.bib`)

```rust
use biblib::{BibParser, CitationParser};

let input = r#"@article{smith2024,
  title = {Machine Learning in Healthcare},
  author = {Smith, John and Doe, Jane},
  date = {2024-05-02},
  doi = {10.1000/example}
}"#;

let citations = BibParser::new().parse(input).unwrap();

assert_eq!(citations.len(), 1);
assert_eq!(citations[0].citation_type, vec!["article"]);
assert_eq!(citations[0].title, "Machine Learning in Healthcare");
assert_eq!(citations[0].doi.as_deref(), Some("10.1000/example"));
```

### Auto-detect Supported Formats

`detect_and_parse()` currently auto-detects RIS, PubMed, EndNote XML, EndNote Tagged (`.enw`), BibTeX / BibLaTeX (`.bib`), and ICTRP CSV. Generic CSV should still be parsed explicitly with `CsvParser`.

```rust
use biblib::detect_and_parse;

let input = "TY  - JOUR\nTI  - Example\nER  -";
let (citations, format) = detect_and_parse(input).unwrap();

assert_eq!(format.as_str(), "RIS");
assert_eq!(citations[0].title, "Example");
```

### Parse ICTRP CSV

```rust
use biblib::{CitationParser, IctrpCsvParser};

let input = concat!(
    "TrialID,Public title,Scientific title,Date registration,Date registration3,Study type,Source Register\n",
    "NCT00000001,Public title,Scientific title,01/05/2026,20260501,Interventional,ClinicalTrials.gov\n"
);

let citations = IctrpCsvParser::new().parse(input).unwrap();
let citation = &citations[0];

assert_eq!(citation.accession_number.as_deref(), Some("NCT00000001"));
assert_eq!(citation.title, "Scientific title");
assert_eq!(citation.citation_type, vec!["Clinical Trial", "Interventional"]);
```

### Parse Generic CSV with Custom Headers

```rust
use biblib::csv::{CsvConfig, CsvParser};
use biblib::CitationParser;

let mut config = CsvConfig::new();
config
    .set_delimiter(b';')
    .set_header_mapping("title", vec!["Article Name".to_string()])
    .set_header_mapping("authors", vec!["Writers".to_string()])
    .set_header_mapping("year", vec!["Published".to_string()]);

let input = "Article Name;Writers;Published\nExample Paper;Smith, John;2023";
let citations = CsvParser::with_config(config).parse(input).unwrap();

assert_eq!(citations[0].title, "Example Paper");
assert_eq!(citations[0].date.as_ref().unwrap().year, 2023);
```

### Deduplicate Parsed Records

```rust
use biblib::dedupe::{Deduplicator, DeduplicatorConfig};
use biblib::{Citation, Date};

let citations = vec![
    Citation {
        title: "Example Title".to_string(),
        doi: Some("10.1000/example".to_string()),
        date: Some(Date { year: 2023, month: None, day: None }),
        journal: Some("Example Journal".to_string()),
        ..Default::default()
    },
    Citation {
        title: "Example Title".to_string(),
        doi: Some("10.1000/example".to_string()),
        date: Some(Date { year: 2023, month: None, day: None }),
        journal: Some("Example Journal".to_string()),
        ..Default::default()
    },
];

let config = DeduplicatorConfig {
    group_by_year: true,
    run_in_parallel: true,
    source_preferences: vec!["PubMed".to_string()],
};

let groups = Deduplicator::new()
    .with_config(config)
    .find_duplicates(&citations)
    .unwrap();

let duplicate_group = groups
    .iter()
    .find(|group| group.unique.doi.as_deref() == Some("10.1000/example"))
    .unwrap();

assert_eq!(duplicate_group.duplicates.len(), 1);
```

## Data Model

The core output type is `Citation`.

Important fields include:

| Field | Type | Purpose |
| --- | --- | --- |
| `citation_type` | `Vec<String>` | Source and work-type labels |
| `title` | `String` | Main normalized title |
| `authors` | `Vec<Author>` | Parsed people with name parts and affiliations |
| `journal` | `Option<String>` | Full journal or source title |
| `journal_abbr` | `Option<String>` | Journal abbreviation |
| `date` | `Option<Date>` | Year with optional month/day |
| `volume` | `Option<String>` | Volume string |
| `issue` | `Option<String>` | Issue or number string |
| `pages` | `Option<String>` | Normalized page range |
| `issn` | `Vec<String>` | One or more ISSNs/serial identifiers |
| `doi` | `Option<String>` | Normalized DOI |
| `accession_number` | `Option<String>` | Registry or source accession identifier |
| `pmid` | `Option<String>` | PubMed identifier |
| `pmc_id` | `Option<String>` | PubMed Central identifier |
| `abstract_text` | `Option<String>` | Abstract text |
| `keywords` | `Vec<String>` | Parsed keywords |
| `urls` | `Vec<String>` | Collected links |
| `language` | `Option<String>` | Language code or label |
| `mesh_terms` | `Vec<String>` | PubMed MeSH terms |
| `publisher` | `Option<String>` | Publisher or sponsor |
| `extra_fields` | `HashMap<String, Vec<String>>` | Source-specific leftovers preserved raw |

This makes it easy to normalize aggressively where the library has clear semantics, while still keeping source-specific information available.

## Feature Flags

| Feature | Enables |
| --- | --- |
| `ris` | RIS parser |
| `pubmed` | PubMed / MEDLINE parser |
| `xml` | EndNote XML parser |
| `enw` | EndNote Tagged (`.enw`) parser |
| `bib` | BibTeX / BibLaTeX (`.bib`) parser |
| `csv` | Generic CSV parser and ICTRP CSV parser |
| `dedupe` | Deduplication engine |
| `diagnostics` | Pretty parse diagnostics via `ariadne` |

Default features: `csv`, `pubmed`, `xml`, `ris`, `enw`, `bib`, `dedupe`

Since `v0.5`, `biblib` no longer uses the `regex` crate or exposes regex-backend feature flags. It uses `regex-lite` internally, and regex backend selection is no longer part of the public API surface.

## Errors and Diagnostics

All parsers return `ParseError` on malformed input. Errors carry:

- The source format
- A 1-based line number when available
- A byte span when available
- A structured `ValueError`

Example:

```rust
use biblib::{CitationParser, RisParser, ValueError};

let input = "TY  - JOUR\nAU  - Smith, John\nER  -\n";

match RisParser::new().parse(input) {
    Ok(_) => unreachable!("expected a parse error"),
    Err(err) => {
        assert_eq!(err.line, Some(1));
        assert!(matches!(err.error, ValueError::MissingValue { key: "TI", .. }));
    }
}
```

For human-friendly diagnostics, enable `diagnostics`:

```toml
[dependencies]
biblib = { version = "0.6", features = ["diagnostics"] }
```

Then use `parse_with_diagnostics()`:

```rust
use biblib::{RisParser, parse_with_diagnostics};

let input = "TY  - JOUR\nAU  - Smith, John\nER  -\n";
let rendered = parse_with_diagnostics(&RisParser::new(), input, "refs.ris");

assert!(rendered.is_err());
```

## Guides

- [PARSING_GUIDE.md](PARSING_GUIDE.md) - format-specific mapping and normalization rules
- [DEDUPLICATION_GUIDE.md](DEDUPLICATION_GUIDE.md) - duplicate matching behavior and configuration
- [docs.rs/biblib](https://docs.rs/biblib) - API reference

## License

Licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
