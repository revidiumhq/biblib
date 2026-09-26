//! Property-based robustness tests for the parsers.
//!
//! Each parser is fed generated input that mixes well-formed lines for its
//! format with malformed fragments, odd whitespace and non-ASCII text. The
//! core property is that parsing never panics; RIS additionally checks that
//! record boundaries survive missing `ER` lines and indentation.

use proptest::prelude::*;

/// Free text that cannot be mistaken for a tag line in any line-based format.
fn plain_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ,.]{1,40}"
        .prop_map(|s| s.trim().to_string())
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Text that is hostile to parsers: separators, brackets, quotes, escapes,
/// multi-byte characters, control characters and a BOM.
fn noisy_text() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => "[ -~]{0,40}",
        1 => "[\\PC]{0,20}",
        1 => Just("\u{feff}".to_string()),
        1 => Just("\r".to_string()),
        1 => "[\u{2010}\u{2013}\u{2212}é漢字🙂\t]{1,8}",
    ]
}

/// Joins generated lines, randomly choosing LF or CRLF endings.
fn join_lines(lines: Vec<String>, crlf: bool) -> String {
    lines.join(if crlf { "\r\n" } else { "\n" })
}

/// Lines built from `tags` with assorted separators and indentation, plus
/// noise and blank lines.
fn tagged_lines(
    tags: &'static [&'static str],
    sep: &'static [&'static str],
) -> impl Strategy<Value = String> {
    let line = prop_oneof![
        6 => (
            prop::sample::select(tags),
            prop::sample::select(sep),
            prop_oneof![Just(""), Just(" "), Just("   ")],
            noisy_text(),
        )
            .prop_map(|(tag, sep, indent, value)| format!("{indent}{tag}{sep}{value}")),
        2 => noisy_text(),
        1 => Just(String::new()),
    ];
    (prop::collection::vec(line, 0..40), any::<bool>()).prop_map(|(l, crlf)| join_lines(l, crlf))
}

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

#[cfg(feature = "ris")]
mod ris {
    use super::*;
    use biblib::{CitationParser, RisParser};

    const TAGS: &[&str] = &[
        "TY", "ER", "AU", "A1", "TI", "T1", "AB", "N2", "KW", "PY", "Y1", "DA", "DO", "SP", "EP",
        "JO", "JF", "T2", "VL", "IS", "SN", "UR", "L1", "PB", "LA", "ID", "!!", "T",
    ];
    const SEPS: &[&str] = &["  - ", "  -", " - ", " -", "- ", "-", " ", ""];

    #[derive(Debug, Clone)]
    struct Record {
        ty: &'static str,
        title: String,
        abstract_: String,
    }

    fn record() -> impl Strategy<Value = Record> {
        (
            prop::sample::select(&["JOUR", "BOOK", "CHAP", "CONF", "RPRT"][..]),
            plain_text(),
            plain_text(),
        )
            .prop_map(|(ty, title, abstract_)| Record {
                ty,
                title,
                abstract_,
            })
    }

    /// Renders records, optionally dropping `ER` lines and indenting the
    /// `TY`/`ER` lines, which must not change how records are split.
    fn render(records: &[Record], drop_er: &[bool], indent: &[bool]) -> String {
        let mut out = String::new();
        for (i, r) in records.iter().enumerate() {
            let pad = if indent[i] { "  " } else { "" };
            out.push_str(&format!("{pad}TY  - {}\n", r.ty));
            out.push_str(&format!("TI  - {}\n", r.title));
            out.push_str(&format!("AB  - {}\n", r.abstract_));
            if !drop_er[i] {
                out.push_str(&format!("{pad}ER  -\n"));
            }
        }
        out
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn never_panics(input in tagged_lines(TAGS, SEPS)) {
            let _ = RisParser::new().parse(&input);
        }

        #[test]
        fn never_panics_on_arbitrary_text(input in "\\PC*") {
            let _ = RisParser::new().parse(&input);
        }

        #[test]
        fn record_boundaries_survive_missing_er_and_indentation(
            (records, drop_er, indent) in prop::collection::vec(record(), 1..8)
                .prop_flat_map(|records| {
                    let n = records.len();
                    (
                        Just(records),
                        prop::collection::vec(any::<bool>(), n),
                        prop::collection::vec(any::<bool>(), n),
                    )
                })
        ) {
            let input = render(&records, &drop_er, &indent);
            let parsed = RisParser::new().parse(&input).unwrap();

            prop_assert_eq!(parsed.len(), records.len(), "input:\n{}", input);
            for (citation, record) in parsed.iter().zip(&records) {
                prop_assert_eq!(&citation.title, &record.title);
                prop_assert_eq!(citation.abstract_text.as_deref(), Some(record.abstract_.as_str()));
            }
        }
    }
}

#[cfg(feature = "pubmed")]
mod pubmed {
    use super::*;
    use biblib::{CitationParser, PubMedParser};

    const TAGS: &[&str] = &[
        "PMID", "TI", "AB", "AU", "FAU", "AD", "DP", "DEP", "TA", "JT", "VI", "IP", "PG", "LID",
        "AID", "IS", "MH", "OT", "LA", "PT", "SO", "      ",
    ];
    const SEPS: &[&str] = &["- ", "  - ", " - ", "-", "  ", ""];

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn never_panics(input in tagged_lines(TAGS, SEPS)) {
            let _ = PubMedParser::new().parse(&input);
        }

        #[test]
        fn never_panics_on_arbitrary_text(input in "\\PC*") {
            let _ = PubMedParser::new().parse(&input);
        }
    }
}

#[cfg(feature = "enw")]
mod enw {
    use super::*;
    use biblib::{CitationParser, EnwParser};

    const TAGS: &[&str] = &[
        "%0", "%T", "%A", "%D", "%J", "%V", "%N", "%P", "%R", "%@", "%K", "%X", "%U", "%B", "%I",
        "%8", "%%", "%",
    ];
    const SEPS: &[&str] = &[" ", "", "  ", "\t"];

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn never_panics(input in tagged_lines(TAGS, SEPS)) {
            let _ = EnwParser::new().parse(&input);
        }

        #[test]
        fn never_panics_on_arbitrary_text(input in "\\PC*") {
            let _ = EnwParser::new().parse(&input);
        }
    }
}

#[cfg(feature = "bib")]
mod bib {
    use super::*;
    use biblib::{BibParser, CitationParser};

    fn fragment() -> impl Strategy<Value = String> {
        prop_oneof![
            prop::sample::select(
                &[
                    "@article{",
                    "@book{",
                    "@inproceedings{",
                    "@string{",
                    "@preamble{",
                    "@comment{",
                    "@misc(",
                    "key,",
                    "title = ",
                    "author = ",
                    "year = ",
                    "doi = ",
                    "pages = ",
                    "month = ",
                    "journal = ",
                    "{",
                    "}",
                    "(",
                    ")",
                    "\"",
                    ",",
                    "#",
                    "=",
                    " and ",
                    "\\'",
                    "\\\"",
                    "{\\\"o}",
                    "--",
                    "@",
                    "\n",
                    " ",
                ][..]
            )
            .prop_map(str::to_string),
            noisy_text(),
        ]
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn never_panics(parts in prop::collection::vec(fragment(), 0..60)) {
            let _ = BibParser::new().parse(&parts.concat());
        }

        #[test]
        fn never_panics_on_arbitrary_text(input in "\\PC*") {
            let _ = BibParser::new().parse(&input);
        }
    }
}

#[cfg(feature = "xml")]
mod xml {
    use super::*;
    use biblib::{CitationParser, EndNoteXmlParser, IctrpXmlParser};

    fn fragment(tags: &'static [&'static str]) -> impl Strategy<Value = String> {
        prop_oneof![
            4 => prop::sample::select(tags).prop_map(str::to_string),
            1 => prop::sample::select(&[
                "&amp;", "&lt;", "&#x41;", "&#65;", "&bogus;", "&", "<![CDATA[", "]]>",
                "<!-- ", " -->", "<", ">", "/>", "\"", "'", "=",
            ][..])
            .prop_map(str::to_string),
            2 => noisy_text(),
        ]
    }

    const ENDNOTE_TAGS: &[&str] = &[
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "<xml>",
        "</xml>",
        "<records>",
        "</records>",
        "<record>",
        "</record>",
        "<ref-type name=\"Journal Article\">17",
        "</ref-type>",
        "<contributors>",
        "</contributors>",
        "<authors>",
        "</authors>",
        "<author>",
        "</author>",
        "<titles>",
        "</titles>",
        "<title>",
        "</title>",
        "<secondary-title>",
        "</secondary-title>",
        "<dates>",
        "</dates>",
        "<year>",
        "</year>",
        "<pub-dates>",
        "<date>",
        "</date>",
        "<electronic-resource-num>",
        "</electronic-resource-num>",
        "<style face=\"normal\">",
        "</style>",
        "<pages>",
        "</pages>",
        "<urls>",
        "<related-urls>",
        "<url>",
        "</url>",
    ];

    const ICTRP_TAGS: &[&str] = &[
        "<?xml version=\"1.0\"?>",
        "<Trials_downloaded_from_ICTRP>",
        "</Trials_downloaded_from_ICTRP>",
        "<Trial>",
        "</Trial>",
        "<TrialID>",
        "</TrialID>",
        "<Public_title>",
        "</Public_title>",
        "<Scientific_title>",
        "</Scientific_title>",
        "<Date_registration>",
        "</Date_registration>",
        "<Date_enrollement>",
        "</Date_enrollement>",
        "<web_address>",
        "</web_address>",
        "<results_url_link>",
        "</results_url_link>",
        "20200131",
        "31/01/2020",
        "2020-01-31",
        "01/2020",
        "<Primary_sponsor>",
        "</Primary_sponsor>",
    ];

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn endnote_never_panics(parts in prop::collection::vec(fragment(ENDNOTE_TAGS), 0..60)) {
            let _ = EndNoteXmlParser::new().parse(&parts.concat());
        }

        #[test]
        fn ictrp_never_panics(parts in prop::collection::vec(fragment(ICTRP_TAGS), 0..60)) {
            let _ = IctrpXmlParser::new().parse(&parts.concat());
        }
    }
}

#[cfg(feature = "csv")]
mod csv {
    use super::*;
    use biblib::{CitationParser, CsvParser};

    fn cell() -> impl Strategy<Value = String> {
        prop_oneof![
            3 => noisy_text(),
            1 => noisy_text().prop_map(|s| format!("\"{}\"", s.replace('"', "\"\""))),
            1 => prop::sample::select(&["\"", "\"\"", ",", "\n", "\r\n", "10.1000/x", "12-3", "2020"][..])
                .prop_map(str::to_string),
        ]
    }

    fn header() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop::sample::select(
                &[
                    "Title",
                    "Author",
                    "Authors",
                    "Year",
                    "DOI",
                    "Journal",
                    "Abstract",
                    "Pages",
                    "Volume",
                    "Issue",
                    "ISSN",
                    "URL",
                    "Keywords",
                    "TrialID",
                    "web address",
                    "",
                ][..],
            ),
            0..10,
        )
        .prop_map(|h| h.join(","))
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn never_panics(
            header in header(),
            rows in prop::collection::vec(prop::collection::vec(cell(), 0..12), 0..12),
        ) {
            let body: Vec<String> = rows.into_iter().map(|r| r.join(",")).collect();
            let input = format!("{header}\n{}", body.join("\n"));
            let _ = CsvParser::new().parse(&input);
            #[allow(deprecated)]
            let _ = biblib::IctrpCsvParser::new().parse(&input);
        }
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn detect_and_parse_never_panics(input in "\\PC*") {
        let _ = biblib::detect_and_parse(&input);
    }
}

#[cfg(all(feature = "ris", feature = "dedupe"))]
mod dedupe {
    use super::*;
    use biblib::dedupe::Deduplicator;
    use biblib::{CitationParser, RisParser};
    use std::collections::HashSet;

    fn ris_record() -> impl Strategy<Value = String> {
        (
            prop_oneof![plain_text(), Just("Same Title".to_string())],
            prop_oneof![
                Just(String::new()),
                Just("10.1000/xyz".to_string()),
                "10\\.[0-9]{4}/[a-z]{1,6}"
            ],
            prop_oneof![
                Just(String::new()),
                Just("2020".to_string()),
                "(19|20)[0-9]{2}"
            ],
            prop_oneof![
                Just(String::new()),
                Just("Smith, J".to_string()),
                plain_text()
            ],
            prop_oneof![
                Just(String::new()),
                Just("12-15".to_string()),
                "[0-9]{1,4}(-[0-9]{1,4})?"
            ],
        )
            .prop_map(|(title, doi, year, author, pages)| {
                let mut r = format!("TY  - JOUR\nTI  - {title}\n");
                for (tag, value) in [("DO", doi), ("PY", year), ("AU", author), ("SP", pages)] {
                    if !value.is_empty() {
                        r.push_str(&format!("{tag}  - {value}\n"));
                    }
                }
                r.push_str("ER  -\n");
                r
            })
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn groups_are_disjoint_and_in_bounds(
            records in prop::collection::vec(ris_record(), 0..30),
            parallel in any::<bool>(),
        ) {
            let citations = RisParser::new().parse(&records.concat()).unwrap();
            let groups = Deduplicator::builder()
                .parallel(parallel)
                .build()
                .find_duplicates(&citations);

            let mut seen = HashSet::new();
            for group in &groups {
                for &idx in std::iter::once(&group.unique).chain(&group.duplicates) {
                    prop_assert!(idx < citations.len());
                    prop_assert!(seen.insert(idx), "index {} appears in two groups", idx);
                }
            }
        }
    }
}

/// Minimal inputs the property tests found; kept as plain regression tests.
mod regressions {
    use biblib::CitationParser;

    #[cfg(feature = "enw")]
    #[test]
    fn enw_multibyte_tag_char() {
        let _ = biblib::EnwParser::new().parse("%\u{feff}");
        let _ = biblib::EnwParser::new().parse("%\u{1107f}");
    }

    #[cfg(feature = "ris")]
    #[test]
    fn ris_page_range_with_multibyte_suffix() {
        let input = "TY  - JOUR\nTI  - T\nSP  - 1𐖁\nEP  - 1\nER  -\n";
        let _ = biblib::RisParser::new().parse(input);
    }

    #[cfg(feature = "xml")]
    #[test]
    fn xml_error_position_inside_multibyte_char() {
        // A leading BOM shifts reader positions so error offsets can land
        // inside a multi-byte character.
        let endnote = "\u{feff} &<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
        assert!(biblib::EndNoteXmlParser::new().parse(endnote).is_err());
        let ictrp = "\u{feff}<?xml version=\"1.0\"?><Trials_downloaded_from_ICTRP><Trial>éé&";
        assert!(biblib::IctrpXmlParser::new().parse(ictrp).is_err());
    }
}
