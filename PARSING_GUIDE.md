# Parsing Guide

This guide documents the parsing behaviors, assumptions, and data transformations for each supported citation format in `biblib`.

## Table of Contents

- [RIS Format](#ris-format)
- [PubMed/MEDLINE Format](#pubmedmedline-format)
- [EndNote XML Format](#endnote-xml-format)
- [CSV Format](#csv-format)
- [Common Transformations](#common-transformations)

---

## RIS Format

RIS (Research Information Systems) uses two-letter tags to identify fields. Each line follows the pattern: `TAG  - Content`.

### Tag Mappings

| Tag | Field | Notes |
|-----|-------|-------|
| TY | Citation type | Required, marks start of record |
| TI, T1 | Title | TI takes priority over T1 |
| AU, A1-A4 | Authors | All treated as authors; multi-author lines supported |
| JF | Journal (full) | Priority 1 for journal name |
| T2 | Secondary title | Priority 2 for journal name |
| JO | Journal (alt) | Priority 3 for journal name |
| JA | Journal abbreviation | Priority 1 for abbreviation |
| J2 | Alt abbreviation | Priority 2 for abbreviation |
| PY, Y1 | Publication date | Format: `YYYY/MM/DD/extra` |
| VL | Volume | |
| IS | Issue | |
| SP, EP | Start/End page | Combined into page range |
| DO | DOI | |
| AB, N2 | Abstract | AB takes priority |
| KW | Keywords | One per line |
| SN | ISSN/ISBN | |
| UR, L1-L4, LK | URLs | All collected |
| ER | End of reference | Marks end of record |

### Multi-Author Handling

**New in v0.3.x**: The parser now handles multiple authors on a single AU line:

```
AU  - Smith, J.; Doe, A. & Brown, B.
```

**Splitting rules** (in order):
1. `;` (semicolon) - primary separator
2. ` & ` (ampersand with spaces) - secondary separator  
3. ` and ` (word with spaces) - secondary separator

**Important**: Commas are NOT used as separators since "Last, First" format uses commas.

### Date Parsing

Dates are parsed from `PY` or `Y1` fields in format: `YYYY/MM/DD/extra`

- Year is required
- Month and day are optional
- Extra text after third `/` is ignored

Examples:
- `2023/12/25/Christmas edition` → Year: 2023, Month: 12, Day: 25
- `2023/05` → Year: 2023, Month: 5, Day: None
- `2023///` → Year: 2023 only

### DOI Extraction

DOI is extracted using a two-pass strategy:

1. **First pass**: Check dedicated `DO` field
2. **Second pass**: If no DOI found, check URL fields (UR, L1-L4, LK) for `doi.org` URLs

DOI normalization removes:
- URL prefixes (`https://doi.org/`, `http://dx.doi.org/`)
- `[doi]` suffix
- Leading/trailing whitespace

### Page Number Formatting

Pages are formatted consistently:
- `1234-45` → `1234-1245` (partial end page completed)
- `R575-82` → `R575-R582` (prefix preserved)
- `101-101` → `101` (duplicate removed)

---

## PubMed/MEDLINE Format

PubMed format uses multi-character tags with continuation lines for long values.

### Key Tag Mappings

| Tag | Field | Notes |
|-----|-------|-------|
| PMID | PubMed ID | Unique identifier |
| TI | Title | |
| AU | Author (short) | Format: `LastName Initials` |
| FAU | Full author name | Format: `LastName, FirstName MiddleNames` |
| AD | Affiliation | Associated with preceding author |
| JT | Full journal title | |
| TA | Journal abbreviation | |
| DP | Publication date | Format: `YYYY MMM DD` |
| VI | Volume | |
| IP | Issue | |
| PG | Pagination | |
| LID | Location ID | May contain DOI |
| AB | Abstract | |
| MH | MeSH terms | One per line |
| IS | ISSN | |
| PMC | PMC ID | |

### Author Handling

PubMed provides both short (`AU`) and full (`FAU`) author names:

```
FAU - Watson, James Dewey
AU  - Watson JD
AD  - Cambridge University
```

**Deduplication**: When `FAU` immediately precedes a matching `AU`, only one author is created.

**Affiliation assignment**: Affiliations (`AD`) are assigned to the most recently parsed author.

### Date Parsing

PubMed dates follow format: `YYYY MMM DD`

Examples:
- `2023 Jun 15` → Year: 2023, Month: 6, Day: 15
- `2023 May` → Year: 2023, Month: 5
- `2023` → Year: 2023 only

### DOI Extraction

DOI is extracted from `LID` field when it ends with ` [doi]`:

```
LID - 10.1234/example [doi]
```

---

## EndNote XML Format

EndNote XML uses a nested XML structure with specific element names.

### Element Mappings

| Element | Field | Notes |
|---------|-------|-------|
| `<ref-type>` | Citation type | `name` attribute |
| `<title>` | Title | Primary |
| `<alt-title>` | Title (fallback) | Used if no `<title>` |
| `<secondary-title>` | Journal | Also fallback for title |
| `<author>` | Authors | Inside `<authors>` |
| `<year>` | Year | May be inside `<dates>` |
| `<volume>` | Volume | |
| `<number>` | Issue | |
| `<pages>` | Pages | |
| `<electronic-resource-num>` | DOI | |
| `<url>` | URL | |
| `<abstract>` | Abstract | |
| `<keyword>` | Keywords | Inside `<keywords>` |
| `<isbn>` | ISSN/ISBN | |
| `<custom2>` | PMC ID | If contains "PMC" |

### Title Fallback Logic

Title is selected with fallback:

1. `<title>` - primary title element
2. `<alt-title>` - alternative title
3. `<secondary-title>` - typically journal, used as last resort

### Author Name Parsing

Author elements contain full names in various formats:

- `Smith, John A.` → Family: "Smith", Given: "John", Middle: "A."
- `Anonymous Author` → Family: "Anonymous", Given: "Author"

---

## CSV Format

CSV parsing is highly configurable with automatic format detection.

### Default Header Mappings

| Standard Names | Field |
|----------------|-------|
| Title, Article Title | title |
| Author, Authors, Author(s) | authors |
| Year, Publication Year, Pub Year | year |
| Journal, Source, Publication | journal |
| Volume, Vol | volume |
| Issue, Number | issue |
| Pages, Pagination | pages |
| DOI | doi |
| Abstract | abstract |
| Keywords | keywords |


### Author Parsing in CSV

Authors are split on semicolons:

```csv
Authors
"Smith, John; Doe, Jane; Brown, Bob"
```

Results in 3 separate authors.

### Auto-Detection

When enabled, the parser automatically detects:
- **Delimiter**: comma, semicolon, or tab
- **Header row**: Checks first row for known field names

### Extra Fields

Unrecognized columns are preserved in `extra_fields` HashMap:

```csv
Title,Author,Custom Field
Paper,Smith,Custom Value
```

`citation.extra_fields["Custom Field"] = ["Custom Value"]`

---

## Common Transformations

### DOI Normalization

All DOI values are normalized:

1. Convert to lowercase
2. Remove URL prefixes (`https://doi.org/`, `doi:`, etc.)
3. Remove `[doi]` suffix
4. Remove all whitespace
5. Extract DOI starting from `10.`

### ISSN Splitting

Multiple ISSNs are split from a single field:

```
1234-5678 (Print) 5678-1234 (Electronic)
```

Becomes: `["1234-5678 (Print)", "5678-1234 (Electronic)"]`

### Author Name Parsing

All formats use the same author parsing logic:

1. If contains comma: split as "Last, First"
2. If contains space: split as "First Last"
3. Single word: treat as family name only

Given name is further split into given and middle names.
