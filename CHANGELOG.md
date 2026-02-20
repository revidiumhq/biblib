# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.2] - 2026-02-21

### Added

- **RIS Citation Type Mapping**: The RIS parser now automatically maps abbreviated citation types (like `JOUR`, `BOOK`) directly to their full, human-readable equivalents (like `Journal Article`, `Book`) during parsing, resulting in cleaner and standardized `citation_type` values downstream. Unrecognized abbreviations fall back to preserving their original strings.

## [0.4.1] - 2026-02-20

### Fixed

- **PubMed book title fallback**: When a PubMed record has no `TI` (Title) field, the parser now falls back to the `BTI` (Book Title) field. This fixes a `MissingValue` error for book citations that use `BTI` instead of `TI`.

## [0.4.0] - 2026-02-20

### Added

- **Line numbers in all parser errors**: Every `ParseError` now carries the 1-based line number (`ParseError::line`) of the citation record that triggered the error. Previously, most conversion errors (missing title, bad date, etc.) had `line: None`. Now all parsers — RIS, PubMed, CSV, and EndNote XML — populate this field consistently.

- **`SourceSpan` type**: New public struct `SourceSpan { start: usize, end: usize }` representing an inclusive-start, exclusive-end byte-offset range into the original source text. Available via `use biblib::SourceSpan`.

- **`ParseError::span` field** (`Option<SourceSpan>`): Every `ParseError` optionally carries a byte-offset span covering the full citation record where the error occurred. Populated by the RIS, PubMed, and CSV parsers for all conversion errors.

- **`ParseError::with_span(span)` builder method**: Attaches a `SourceSpan` to an existing `ParseError`, enabling builder-style construction.

- **`diagnostics` feature** (optional dependency on [`ariadne`](https://crates.io/crates/ariadne)): Enable with `features = ["diagnostics"]` to unlock:
  - `ParseError::to_diagnostic(filename: &str, source: &str) -> String` — renders a human-readable diagnostic with ANSI colour highlighting, source context lines, and underlined error spans using the ariadne crate.
  - `parse_with_diagnostics(parser, input, filename)` — convenience free function that calls any `CitationParser` and on failure returns `Err(String)` containing the rendered diagnostic.

### Changed

- **`ParseError` struct layout** (BREAKING for struct-literal construction): A new `span: Option<SourceSpan>` field has been inserted between `column` and `format`. Any code constructing `ParseError` with struct literal syntax (i.e. `ParseError { line: …, column: …, format: …, error: … }`) must add `span: None`. Code using the provided constructors (`at_line`, `at_position`, `without_position`, `new`) is unaffected.

### Migration Guide

#### `ParseError` struct literal construction

If you construct `ParseError` with a struct literal, add the new `span` field:

```rust
// Before (0.3.x):
ParseError { line: Some(1), column: None, format: CitationFormat::Ris, error: ValueError::Syntax("…".into()) }

// After (0.4.x):
ParseError { line: Some(1), column: None, span: None, format: CitationFormat::Ris, error: ValueError::Syntax("…".into()) }
```

Using the constructors instead avoids this:

```rust
// No change needed:
ParseError::at_line(1, CitationFormat::Ris, ValueError::Syntax("…".into()))
```

#### Opt-in rich diagnostics

```toml
[dependencies]
biblib = { version = "0.4", features = ["diagnostics"] }
```

```rust
use biblib::{CitationParser, RisParser, parse_with_diagnostics};

let source = std::fs::read_to_string("citations.ris")?;
match parse_with_diagnostics(&RisParser::new(), &source, "citations.ris") {
    Ok(citations) => println!("Parsed {} citations", citations.len()),
    Err(diagnostic) => eprintln!("{}", diagnostic), // pretty, coloured output
}
```

## [0.3.2] - 2025-12-30

### Added

- **PubMed DOI fallback to AID field**: The PubMed parser now checks the `AID` (Article Identifier) field for DOI when not found in the `LID` (Location ID) field. This improves DOI extraction for citations where the DOI is only present in the AID field.

## [0.3.1] - 2025-12-24

### Added

- **Multi-author parsing for RIS**: The RIS parser now handles multiple authors on a single AU line, splitting on semicolons (`;`), ampersands (`&`), and the word `and`
- **PARSING_GUIDE.md**: Comprehensive documentation for all format parsers (RIS, PubMed, EndNote XML, CSV) including tag mappings, date formats, and data transformations
- **DEDUPLICATION_GUIDE.md**: Detailed documentation of the deduplication algorithm, similarity thresholds, normalization rules, and configuration options

### Changed

- **README.md**: Complete rewrite with cleaner structure and accurate API examples

## [0.3.0] - 2025-08-17

### Added

- **Enhanced deduplication API**: New `find_duplicates_with_sources()` method for source-aware deduplication when sources are managed externally
- **Enhanced CSV parser functionality**: Added configuration options for quote character, trimming, flexible parsing, memory optimization, additional header aliases, validation, and automatic format detection

### Changed

- **Error handling system**: Complete restructure of error types for better debugging and programmatic error handling
  - `CitationParser::parse` now returns `Result<Vec<Citation>, ParseError>` instead of `Result<Vec<Citation>, CitationError>`
  - `CitationError` restructured with `UnknownFormat` and `Parse(ParseError)` variants
  - Enhanced error reporting with line/column tracking and semantic error types (`Syntax`, `MissingValue`, `BadValue`, `MultipleValues`)
  - Empty input now returns `Ok(Vec::new())` instead of errors across all parsers
- **`detect_and_parse()` function signature**: Now takes only one parameter (`content`) instead of two (`content`, `source`)
- **Author struct (BREAKING)**: The `Author` schema has changed to support richer name handling and multiple affiliations
  - Before: `Author { family_name: String, given_name: String, affiliation: Option<String> }`
  - After: `Author { name: String, given_name: Option<String>, middle_name: Option<String>, affiliations: Vec<String> }`
  - Name parsing is standardized via a shared utility; mononyms and middle names are handled consistently across all parsers

### Fixed

- **CSV extra fields functionality**: Fixed critical issue where extra fields were completely ignored and not populated in Citation structs

### Removed

- **Citation ID field**: The `id` field has been completely removed from the `Citation` struct as it is not part of the actual bibliographic data parsed from citation formats
- **Source tracking in Citation struct**: The `source` field has been completely removed from the `Citation` struct and all parser implementations
- **Parser `with_source()` methods**: All parsers no longer have the `with_source()` method for setting citation source
- **Citation year field (BREAKING)**: The deprecated `year: Option<i32>` field has been removed from `Citation`; use `date.year` instead

### Migration Guide

#### 1. Citation ID Management

If you were relying on the auto-generated `id` field in citations, you'll need to manage IDs at the application level:

**Before (v0.2.x):**

```rust
let parser = RisParser::new();
let citations = parser.parse(input).unwrap();
let citation_id = citations[0].id.clone(); // Auto-generated nanoid
```

**After (v0.3.x):**

```rust
let parser = RisParser::new();
let citations = parser.parse(input).unwrap();
// Generate IDs in your application if needed:
let citation_id = nanoid::nanoid!(); // or your preferred ID system
```

#### 2. Source Tracking

Source tracking must now be handled at the application level instead of within the Citation struct:

**Before (v0.2.x):**

```rust
let parser = RisParser::new().with_source("PubMed");
let citations = parser.parse(input).unwrap();
let source = citations[0].source.clone(); // "PubMed"
```

**After (v0.3.x):**

```rust
let parser = RisParser::new();
let citations = parser.parse(input).unwrap();
// Handle source tracking in your application:
let source = "PubMed"; // manage this in your app
```

#### 3. Error Handling

The error system has been completely restructured for better debugging and programmatic error handling:

**Before (v0.2.x):**

```rust
use biblib::CitationError;

match parser.parse(input) {
    Ok(citations) => println!("Parsed {} citations", citations.len()),
    Err(CitationError::ParseError(msg)) => eprintln!("Parse error: {}", msg),
    Err(CitationError::IoError(e)) => eprintln!("IO error: {}", e),
}
```

**After (v0.3.x):**

```rust
use biblib::{CitationError, ParseError, ValueError};

match parser.parse(input) {
    Ok(citations) => println!("Parsed {} citations", citations.len()),
    Err(parse_err) => {
        // Much more detailed error information
        eprintln!("Parse error at line {}: {}",
            parse_err.line.unwrap_or(0), parse_err);

        // Handle specific error types
        match &parse_err.error {
            ValueError::Syntax(msg) => eprintln!("Syntax error: {}", msg),
            ValueError::MissingValue { field, key } => {
                eprintln!("Missing required field {}: {}", field, key)
            },
            ValueError::BadValue { field, key, value, reason } => {
                eprintln!("Invalid value in field {} ({}): {} ({})", field, key, value, reason)
            },
            ValueError::MultipleValues { field, key, .. } => {
                eprintln!("Multiple values for field {} ({})", field, key)
            },
        }
    }
}

// For top-level API (detect_and_parse), handle CitationError:
match detect_and_parse(content) {
    Ok((citations, format)) => println!("Detected {} format, parsed {} citations", format, citations.len()),
    Err(CitationError::UnknownFormat) => eprintln!("Could not detect citation format"),
    Err(CitationError::Parse(parse_err)) => eprintln!("Parse error: {}", parse_err),
}
```

Note: Empty input now returns `Ok(Vec::new())` instead of an error, improving API usability.

#### 4. Format Detection API

The `detect_and_parse()` function no longer accepts a source parameter:

**Before (v0.2.x):**

```rust
let (citations, format) = detect_and_parse(content, "PubMed").unwrap();
```

**After (v0.3.x):**

```rust
let (citations, format) = detect_and_parse(content).unwrap();
// Track source separately in your application
```

#### 5. Enhanced CSV Parser API

The CSV parser has been significantly enhanced. Existing code will continue to work, but new features are available:

```rust
use biblib::csv::{CsvParser, CsvConfig};

// Auto-detection (new)
let parser = CsvParser::with_auto_detection();

// Enhanced configuration (new options)
let mut config = CsvConfig::new();
config.set_quote(b'\'').set_flexible(true).add_header_aliases("title", vec!["paper_title".to_string()]);

let parser = CsvParser::with_config(config);
```

#### 6. Deduplication with Sources

Use the new `find_duplicates_with_sources()` method when you need source-aware deduplication:

**Before (v0.2.x):**

```rust
let citations = vec![/* citations with source field */];
let deduplicator = Deduplicator::new().with_config(config);
let groups = deduplicator.find_duplicates(&citations).unwrap();
```

**After (v0.3.x):**

```rust
let citations = vec![/* citations without source field */];
let sources = vec!["PubMed", "CrossRef"]; // source for each citation
let deduplicator = Deduplicator::new().with_config(config);
let groups = deduplicator.find_duplicates_with_sources(&citations, &sources).unwrap();
```

#### 7. Author Struct Changes (BREAKING)

The `Author` struct now supports richer name handling and multiple affiliations. Update your code as follows:

**Before (v0.2.x):**

```rust
let a = biblib::Author {
    family_name: "Smith".into(),
    given_name: "John".into(),
    affiliation: Some("University of Nowhere".into()),
};
```

**After (v0.3.x):**

```rust
let a = biblib::Author {
    name: "Smith".into(),
    given_name: Some("John".into()),
    middle_name: None,
    affiliations: vec!["University of Nowhere".into()],
};
```

Notes:

- `name` contains the full name; `given_name`/`middle_name` are optional.
- Multiple affiliations are stored in the `affiliations` vector.
- Parsers now handle name splitting consistently.

#### 8. Citation Year Field Removed (BREAKING)

The `Citation.year` field has been removed. Use `date.year` instead:

**Before (v0.2.x):**

```rust
let year = citation.year;
```

**After (v0.3.x):**

```rust
let year = citation.date.as_ref().map(|d| d.year);
```

Notes:

- Parsers now populate `citation.date` only.
- CSV mappings for a `year` header still populate `citation.date`.

## [0.2.4] - 2025-06-11

### Fixed

- Fixed the line continuation for `TI` and `AB` tags in PubMed parser to handle cases where these tags are split across multiple lines

## [0.2.3] - 2025-06-09

### Fixed

- Improved modularity of CSV and XML feature. Fixes compilation errors when default features are not in use

### Changed

- License changed to MIT or Apache-2.0

## [0.2.2] - 2025-01-31

### Fixed

- Fixed RIS parser line to handle tags like T1, A2, etc.

## [0.2.1] - 2025-01-28

### Added

- New `detect_and_parse` function for automatic format detection and parsing
- Support for automatic detection of RIS, PubMed, and EndNote XML formats

## [0.2.0] - 2025-01-28

### Added

- New `source` field in `Citation` struct to track citation origin
- `.with_source()` method on all parsers (RIS, PubMed, EndNote XML, CSV) to specify citation source
- `source_preferences` option in `DeduplicatorConfig` for controlling unique citation selection
- Cargo features for optional components:
  - `csv` - Enable CSV format support
  - `pubmed` - Enable PubMed/MEDLINE format support
  - `xml` - Enable EndNote XML support
  - `ris` - Enable RIS format support
  - `dedupe` - Enable citation deduplication
  - All features enabled by default

### Changed

- Enhanced unique citation selection logic in deduplicator:
  1. Prefers citations from sources listed in `source_preferences`
  2. Falls back to citations with abstracts if no source preference matches
  3. Prefers citations with DOIs if abstracts exist in both citations
  4. Uses first citation as fallback if all above criteria are equal

## [0.1.0] - 2025-01-25

### Added

- Initial release with core functionality
- Support for multiple citation formats (RIS, PubMed, EndNote XML, CSV)
- Citation deduplication engine
- Comprehensive metadata handling
