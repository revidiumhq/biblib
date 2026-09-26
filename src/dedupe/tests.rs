use super::*;
use crate::{Author, Date};

fn make_citation(
    title: &str,
    year: Option<i32>,
    journal: Option<&str>,
    volume: Option<&str>,
    pages: Option<&str>,
    doi: Option<&str>,
) -> Citation {
    Citation {
        title: title.to_string(),
        journal: journal.map(str::to_string),
        date: year.map(|year| Date {
            year,
            month: None,
            day: None,
        }),
        volume: volume.map(str::to_string),
        pages: pages.map(str::to_string),
        doi: doi.map(str::to_string),
        ..Default::default()
    }
}

fn make_citation_with_author(
    title: &str,
    author_name: &str,
    year: Option<i32>,
    journal: Option<&str>,
    volume: Option<&str>,
    pages: Option<&str>,
) -> Citation {
    let mut citation = make_citation(title, year, journal, volume, pages, None);
    citation.authors.push(Author {
        name: author_name.to_string(),
        given_name: None,
        middle_name: None,
        affiliations: Vec::new(),
    });
    citation
}

fn make_citation_with_author_and_doi(
    title: &str,
    author_name: &str,
    year: Option<i32>,
    journal: Option<&str>,
    volume: Option<&str>,
    pages: Option<&str>,
    doi: Option<&str>,
) -> Citation {
    let mut citation = make_citation(title, year, journal, volume, pages, doi);
    citation.authors.push(Author {
        name: author_name.to_string(),
        given_name: None,
        middle_name: None,
        affiliations: Vec::new(),
    });
    citation
}

fn with_issue(mut citation: Citation, issue: Option<&str>) -> Citation {
    citation.issue = issue.map(str::to_string);
    citation
}

fn with_issn(mut citation: Citation, issn: &[&str]) -> Citation {
    citation.issn = issn.iter().map(|value| (*value).to_string()).collect();
    citation
}

fn group_members(group: &DuplicateGroup) -> Vec<usize> {
    let mut members = vec![group.unique];
    members.extend(group.duplicates.iter().copied());
    members.sort_unstable();
    members
}

fn covered_indices(groups: &[DuplicateGroup]) -> Vec<usize> {
    let mut covered = groups.iter().flat_map(group_members).collect::<Vec<_>>();
    covered.sort_unstable();
    covered
}

fn canonicalize_groups(groups: &[DuplicateGroup], index_map: &[usize]) -> Vec<Vec<usize>> {
    let mut canonical = groups
        .iter()
        .map(|group| {
            let mut members = group_members(group)
                .into_iter()
                .map(|idx| index_map[idx])
                .collect::<Vec<_>>();
            members.sort_unstable();
            members
        })
        .collect::<Vec<_>>();
    canonical.sort();
    canonical
}

#[test]
fn test_group_by_year() {
    let citations = vec![
        Citation {
            title: "Title 1".to_string(),
            authors: vec![],
            journal: None,
            journal_abbr: None,
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            volume: None,
            abstract_text: None,
            doi: None,
            ..Default::default()
        },
        Citation {
            title: "Title 2".to_string(),
            authors: vec![],
            journal: None,
            journal_abbr: None,
            date: None,
            volume: None,
            abstract_text: None,
            doi: None,
            ..Default::default()
        },
    ];

    let deduplicator = Deduplicator::new();
    let records = deduplicator.preprocess_citations(&citations);
    let tasks = deduplicator.build_pass2_block_tasks(&records);

    assert!(tasks.iter().any(|task| matches!(
        task,
        BlockTask::Cross(left, right)
            if left.as_slice() == [1] && right.as_slice() == [0]
    )));
}

#[test]
fn test_find_duplicates() {
    let citations = vec![
        Citation {
            title: "Title 1".to_string(),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            doi: Some("10.1234/abc".to_string()),
            journal: Some("Journal 1".to_string()),
            ..Default::default()
        },
        Citation {
            title: "Title 1".to_string(),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            doi: Some("10.1234/abc".to_string()),
            journal: Some("Journal 1".to_string()),
            ..Default::default()
        },
        Citation {
            title: "Title 2".to_string(),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            doi: Some("10.1234/def".to_string()),
            journal: Some("Journal 2".to_string()),
            ..Default::default()
        },
    ];

    let deduplicator = Deduplicator::new();
    let duplicate_groups = deduplicator.find_duplicates(&citations);

    assert_eq!(duplicate_groups.len(), 2);
    assert_eq!(
        duplicate_groups
            .iter()
            .find(|g| citations[g.unique].doi.as_deref() == Some("10.1234/abc"))
            .unwrap()
            .duplicates
            .len(),
        1
    );
}

#[test]
fn test_missing_doi() {
    let citations = vec![
        Citation {
            title: "Title 1".to_string(),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            doi: Some("10.1234/abc".to_string()),
            journal: Some("Journal 1".to_string()),
            volume: Some("24".to_string()),
            ..Default::default()
        },
        Citation {
            title: "Title 1".to_string(),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            doi: Some("".to_string()),
            journal: Some("Journal 1".to_string()),
            volume: Some("24".to_string()),
            ..Default::default()
        },
        Citation {
            title: "Title 2".to_string(),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            doi: Some("".to_string()),
            journal: Some("Journal 2".to_string()),
            ..Default::default()
        },
    ];

    let deduplicator = Deduplicator::new();
    let duplicate_groups = deduplicator.find_duplicates(&citations);

    assert_eq!(duplicate_groups.len(), 2);
}

#[test]
fn test_normalize_string() {
    assert_eq!(
        normalize::normalize_string("Machine Learning! (2<sup>nd</sup> Edition)"),
        "machinelearning2ndedition".to_string()
    );
    assert_eq!(
        normalize::normalize_string("[&lt;sup&gt;11&lt;/sup&gt;C] benzo"),
        "11cbenzo".to_string()
    );
    assert_eq!(
        normalize::normalize_string("β-blocker effects in &#946;-cells &amp; &#x3B2;-agonists"),
        "betablockereffectsinbetacellsbetaagonists".to_string()
    );
    assert_eq!(
        normalize::normalize_string("beta-blocker effects in β-cells"),
        "betablockereffectsinbetacells".to_string()
    );
    assert_eq!(
        normalize::normalize_string("&quot;α&quot; vs &Alpha; and ß"),
        "alphavsalphaandbeta".to_string()
    );
    assert_eq!(
        normalize::normalize_string("Gene[sub]A[/sub] (sup)2(/sup)"),
        "genea2".to_string()
    );
    assert_eq!(
        normalize::normalize_string("Subgroup analysis [sup]A[/sup]"),
        "subgroupanalysisa".to_string()
    );
    assert_eq!(
        normalize::normalize_string("The Immune Response"),
        "immuneresponse".to_string()
    );
    assert_eq!(
        normalize::normalize_string("A clinical pathway"),
        "clinicalpathway".to_string()
    );
    assert_eq!(
        normalize::normalize_string("An observational cohort"),
        "observationalcohort".to_string()
    );
}

#[test]
fn test_normalize_author_key_unicode_folding() {
    assert_eq!(
        normalize::normalize_author_key("Keyriläinen"),
        Some("keyrilainen".to_string())
    );
    assert_eq!(
        normalize::normalize_author_key("Keyrilainen"),
        Some("keyrilainen".to_string())
    );
}

#[test]
fn test_normalize_start_page_treats_placeholders_as_missing() {
    for placeholder in ["N.PAG", "N.PAG-N.PAG", "no pagination", "n/a", "na"] {
        assert_eq!(normalize::normalize_start_page(placeholder), None);
    }
}

#[test]
fn test_convert_unicode_string() {
    // Test basic conversion
    assert_eq!(
        normalize::convert_unicode_string("2<U+0391>-amino-4<U+0391>"),
        "2\u{0391}-amino-4\u{0391}",
        "Failed to convert basic Alpha Unicode sequences"
    );

    // Test multiple different Unicode sequences
    assert_eq!(
        normalize::convert_unicode_string("Hello <U+03A9>orld <U+03A3>cience"),
        "Hello \u{03A9}orld \u{03A3}cience",
        "Failed to convert multiple Unicode sequences"
    );

    // Test string with no Unicode sequences
    assert_eq!(
        normalize::convert_unicode_string("Normal String"),
        "Normal String",
        "Incorrectly modified string with no Unicode sequences"
    );

    // Test empty string
    assert_eq!(
        normalize::convert_unicode_string(""),
        "",
        "Failed to handle empty string"
    );

    // Test mixed content
    assert_eq!(
        normalize::convert_unicode_string("Mixed <U+0394> Unicode <U+03A9> Test"),
        "Mixed \u{0394} Unicode \u{03A9} Test",
        "Failed to handle mixed content with Unicode sequences"
    );

    // Test consecutive Unicode sequences
    assert_eq!(
        normalize::convert_unicode_string("<U+0391><U+0392><U+0393>"),
        "\u{0391}\u{0392}\u{0393}",
        "Failed to convert consecutive Unicode sequences"
    );
}

#[test]
fn test_normalize_volume() {
    assert_eq!(normalize::normalize_volume("61"), "61");
    assert_eq!(normalize::normalize_volume("61 (Supplement 1)"), "61");
    assert_eq!(normalize::normalize_volume("9 (8) (no pagination)"), "9");
    assert_eq!(normalize::normalize_volume("3)"), "3");
    assert_eq!(normalize::normalize_volume("Part A. 242"), "242");
    assert_eq!(normalize::normalize_volume("55 (10 SUPPL 1)"), "55");
    assert_eq!(normalize::normalize_volume("161A"), "161");
    assert_eq!(normalize::normalize_volume("74 Suppl 1"), "74");
    assert_eq!(normalize::normalize_volume("20 (2)"), "20");
    assert_eq!(normalize::normalize_volume("9 (FEB) (no pagination)"), "9");
}

#[test]
fn test_format_journal_name() {
    assert_eq!(
        normalize::format_journal_name(Some(
            "Heart. Conference: British Atherosclerosis Society BAS/British Society for Cardiovascular Research BSCR Annual Meeting"
        )),
        Some("heart".to_string())
    );
    assert_eq!(
        normalize::format_journal_name(Some("The FASEB Journal. Conference: Experimental Biology")),
        Some("fasebjournal".to_string())
    );
    assert_eq!(
        normalize::format_journal_name(Some(
            "Arteriosclerosis Thrombosis and Vascular Biology. Conference: American Heart Association's Arteriosclerosis Thrombosis and Vascular Biology"
        )),
        Some("arteriosclerosisthrombosisandvascularbiology".to_string())
    );
    assert_eq!(
        normalize::format_journal_name(Some("Journal of Sports Medicine & Physical Fitness")),
        Some("journalofsportsmedicineandphysicalfitness".to_string())
    );
    assert_eq!(
        normalize::format_journal_name(Some("The Journal of sports medicine and physical fitness")),
        Some("journalofsportsmedicineandphysicalfitness".to_string())
    );
    assert_eq!(
        normalize::format_journal_name(Some("Journal of sports medicine and physical fitness")),
        Some("journalofsportsmedicineandphysicalfitness".to_string())
    );
    assert_eq!(
        normalize::format_journal_name(Some("Journal of Sports Medicine &amp; Physical Fitness")),
        Some("journalofsportsmedicineandphysicalfitness".to_string())
    );
    assert_eq!(
        normalize::format_journal_name(Some("Thorax")),
        Some("thorax".to_string())
    );
    assert_eq!(normalize::format_journal_name(None), None);
    assert_eq!(
        normalize::format_journal_name(Some("")),
        Some("".to_string())
    );
    assert_eq!(
        normalize::format_journal_name(Some("Diabetologie und Stoffwechsel. Conference")),
        Some("diabetologieundstoffwechsel".to_string())
    );
}

#[test]
fn test_normalize_issue() {
    assert_eq!(normalize::normalize_issue("Issue 4"), "4".to_string());
    assert_eq!(normalize::normalize_issue("No. 4"), "4".to_string());
    assert_eq!(
        normalize::normalize_issue("(Suppl 2)"),
        "suppl2".to_string()
    );
    assert_eq!(normalize::normalize_issue("S-1"), "s1".to_string());
}

#[test]
fn test_match_issns_scenarios() {
    // Scenario 1: Matching lists
    let issns1 = vec!["1234-5678".to_string(), "8765-4321".to_string()];
    let issns2 = vec!["0000-0000".to_string(), "1234-5678".to_string()];
    assert!(
        Deduplicator::match_issns(&issns1, &issns2),
        "Should find a matching ISSN"
    );

    let non_match_issns2 = vec!["5555-6666".to_string(), "7777-8888".to_string()];
    assert!(
        !Deduplicator::match_issns(&issns1, &non_match_issns2),
        "Should not find a matching ISSN"
    );

    // Scenario 3: Empty lists
    let empty_issns1: Vec<String> = vec![];
    let empty_issns2: Vec<String> = vec![];
    assert!(
        !Deduplicator::match_issns(&empty_issns1, &empty_issns2),
        "Should return false for empty lists"
    );

    // Scenario 4: One empty list
    let partial_issns1 = vec!["1234-5678".to_string()];
    let partial_issns2: Vec<String> = vec![];
    assert!(
        !Deduplicator::match_issns(&partial_issns1, &partial_issns2),
        "Should return false when one list is empty"
    );
}

#[test]
fn test_format_issn() {
    assert_eq!(
        normalize::format_issn("1234-5678"),
        Some("1234-5678".to_string())
    );
    assert_eq!(
        normalize::format_issn("12345678"),
        Some("1234-5678".to_string())
    );
    assert_eq!(
        normalize::format_issn("1234-567X"),
        Some("1234-567X".to_string())
    );
    assert_eq!(
        normalize::format_issn("1234-567X (Electronic)"),
        Some("1234-567X".to_string())
    );
    assert_eq!(
        normalize::format_issn("1234-5678 (Print)"),
        Some("1234-5678".to_string())
    );
    assert_eq!(
        normalize::format_issn("1234-5678 (Linking)"),
        Some("1234-5678".to_string())
    );
    assert_eq!(normalize::format_issn("invalid"), None);
    assert_eq!(normalize::format_issn("1234-56789"), None);
    assert_eq!(normalize::format_issn("123-45678"), None);
}

#[test]
fn test_format_issn_rejects_x_in_non_final_position() {
    assert_eq!(normalize::format_issn("123X-5678"), None);
    assert_eq!(normalize::format_issn("X234-5678"), None);
    assert_eq!(normalize::format_issn("1234-5X78"), None);
    assert_eq!(
        normalize::format_issn("1234-567X"),
        Some("1234-567X".to_string())
    );
}

#[test]
fn test_year_tolerance_zero_disables_cross_year_no_doi_matches() {
    let citations = vec![
        Citation {
            title: "Title 1".to_string(),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            journal: Some("Journal 1".to_string()),
            volume: Some("24".to_string()),
            pages: Some("100-110".to_string()),
            doi: None,
            ..Default::default()
        },
        Citation {
            title: "Title 1".to_string(),
            date: Some(crate::Date {
                year: 2019,
                month: None,
                day: None,
            }),
            journal: Some("Journal 1".to_string()),
            volume: Some("24".to_string()),
            pages: Some("100-110".to_string()),
            doi: None,
            ..Default::default()
        },
    ];

    let deduplicator = Deduplicator::new();
    let duplicate_groups = deduplicator.find_duplicates(&citations);

    assert_eq!(duplicate_groups.len(), 1);
    assert_eq!(duplicate_groups[0].duplicates.len(), 1);

    let deduplicator = Deduplicator::builder().year_tolerance(0).build();
    let duplicate_groups = deduplicator.find_duplicates(&citations);

    assert_eq!(duplicate_groups.len(), 2);
    assert!(duplicate_groups.iter().all(|g| g.duplicates.is_empty()));
}

#[test]
fn test_unknown_year_matches_known_year_end_to_end() {
    let citations = vec![
        make_citation(
            "Yearless cross block",
            None,
            Some("Journal"),
            Some("6"),
            Some("70-80"),
            None,
        ),
        make_citation(
            "Yearless cross block",
            Some(2020),
            Some("Journal"),
            Some("6"),
            Some("70-80"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].unique, 0);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_source_preferences() {
    let citations = vec![
        Citation {
            title: "Title 1".to_string(),
            doi: Some("10.1234/abc".to_string()),
            journal: Some("Journal 1".to_string()),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            ..Default::default()
        },
        Citation {
            title: "Title 1".to_string(),
            doi: Some("10.1234/abc".to_string()),
            journal: Some("Journal 1".to_string()),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            ..Default::default()
        },
    ];

    let sources = vec!["source2", "source1"];

    let deduplicator = Deduplicator::builder()
        .source_preferences(["source1", "source2"])
        .build();
    let duplicate_groups = deduplicator.find_duplicates_with_sources(&citations, &sources);

    assert_eq!(duplicate_groups.len(), 1);
    // The second citation should be selected as unique because source1 (PubMed)
    // has higher priority than source2 (Embase) in our preferences
    assert_eq!(duplicate_groups[0].unique, 1);
    assert_eq!(duplicate_groups[0].duplicates.len(), 1);
}

#[test]
fn test_abstract_preference() {
    let citations = vec![
        Citation {
            title: "Title 1".to_string(),
            abstract_text: None,
            doi: Some("10.1234/abc".to_string()),
            journal: Some("Journal 1".to_string()),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            ..Default::default()
        },
        Citation {
            title: "Title 1".to_string(),
            abstract_text: Some("Abstract".to_string()),
            doi: Some("10.1234/abc".to_string()),
            journal: Some("Journal 1".to_string()),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            ..Default::default()
        },
    ];

    let deduplicator = Deduplicator::new();
    let duplicate_groups = deduplicator.find_duplicates(&citations);

    assert_eq!(duplicate_groups.len(), 1);
    // The citation with abstract should be selected as unique
    assert!(
        citations[duplicate_groups[0].unique]
            .abstract_text
            .is_some()
    );
    assert_eq!(duplicate_groups[0].duplicates.len(), 1);
}

#[test]
fn test_source_preferences_with_year_grouping() {
    // Create citations from different years to test year grouping with source preferences
    let citations = vec![
        Citation {
            title: "Test Article 2020".to_string(),
            doi: Some("10.1234/test2020".to_string()),
            journal: Some("Test Journal".to_string()),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            ..Default::default()
        },
        Citation {
            title: "Test Article 2020".to_string(), // Same as above but different source
            doi: Some("10.1234/test2020".to_string()),
            journal: Some("Test Journal".to_string()),
            date: Some(crate::Date {
                year: 2020,
                month: None,
                day: None,
            }),
            ..Default::default()
        },
        Citation {
            title: "Test Article 2021".to_string(),
            doi: Some("10.1234/test2021".to_string()),
            journal: Some("Test Journal".to_string()),
            date: Some(crate::Date {
                year: 2021,
                month: None,
                day: None,
            }),
            ..Default::default()
        },
        Citation {
            title: "Test Article 2021".to_string(), // Same as above but different source
            doi: Some("10.1234/test2021".to_string()),
            journal: Some("Test Journal".to_string()),
            date: Some(crate::Date {
                year: 2021,
                month: None,
                day: None,
            }),
            ..Default::default()
        },
    ];

    // Sources with PubMed having higher priority
    let sources = vec!["Embase", "PubMed", "Embase", "PubMed"];

    let deduplicator = Deduplicator::builder()
        .year_tolerance(0)
        .parallel(false)
        .source_preferences(["PubMed", "Embase"])
        .build();
    let duplicate_groups = deduplicator.find_duplicates_with_sources(&citations, &sources);

    // Should find 2 duplicate groups (one for each year)
    assert_eq!(duplicate_groups.len(), 2);

    // Both unique citations should be from PubMed (indices 1 and 3)
    // We can't directly check the source, but we can check that each group has the expected structure
    let unique_titles: Vec<&str> = duplicate_groups
        .iter()
        .map(|group| citations[group.unique].title.as_str())
        .collect();

    assert!(unique_titles.contains(&"Test Article 2020"));
    assert!(unique_titles.contains(&"Test Article 2021"));

    // Each group should have exactly one duplicate
    for group in &duplicate_groups {
        assert_eq!(group.duplicates.len(), 1);
    }
}

#[test]
fn test_empty_title_is_singleton_unless_doi_matched() {
    let citations = vec![
        make_citation(
            "",
            Some(2020),
            Some("Journal"),
            Some("1"),
            Some("10-20"),
            None,
        ),
        make_citation(
            "",
            Some(2020),
            Some("Journal"),
            Some("1"),
            Some("10-20"),
            Some("10.1000/empty-title"),
        ),
        make_citation(
            "Linked record",
            Some(2020),
            Some("Journal"),
            Some("1"),
            Some("10-20"),
            Some("https://doi.org/10.1000/empty-title"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().any(|group| group_members(group) == vec![0]));
    assert!(
        groups
            .iter()
            .any(|group| group_members(group) == vec![1, 2])
    );
}

#[test]
fn test_empty_journal_strings_do_not_match() {
    let citations = vec![
        make_citation("Shared title", Some(2020), Some(""), Some("12"), None, None),
        make_citation("Shared title", Some(2020), Some(""), Some("12"), None, None),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_doi_variants_are_deduplicated() {
    let citations = vec![
        make_citation(
            "Normalized DOI article",
            Some(2023),
            Some("Journal"),
            None,
            None,
            Some("https://doi.org/10.1000/X"),
        ),
        make_citation(
            "Normalized DOI article",
            Some(2023),
            Some("Journal"),
            None,
            None,
            Some("10.1000/x"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_truncated_pages_share_start_page_match() {
    let citations = vec![
        make_citation(
            "Paged article",
            Some(2020),
            Some("Journal"),
            None,
            Some("1234-45"),
            None,
        ),
        make_citation(
            "Paged article",
            Some(2020),
            Some("Journal"),
            None,
            Some("1234-1245"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_unicode_dash_pages_share_start_page_match() {
    let citations = vec![
        make_citation(
            "Paged article",
            Some(2020),
            Some("Journal"),
            None,
            Some("1417‐1422"),
            None,
        ),
        make_citation(
            "Paged article",
            Some(2020),
            Some("Journal"),
            None,
            Some("1417-1422"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_default_year_tolerance_matches_adjacent_years_without_doi() {
    let citations = vec![
        make_citation(
            "Cross-year match",
            Some(2020),
            Some("Journal"),
            Some("8"),
            Some("100-110"),
            None,
        ),
        make_citation(
            "Cross-year match",
            Some(2021),
            Some("Journal"),
            Some("8"),
            Some("100-110"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);

    let strict_groups = Deduplicator::builder()
        .year_tolerance(0)
        .build()
        .find_duplicates(&citations);
    assert_eq!(strict_groups.len(), 2);
}

#[test]
fn test_custom_year_tolerance_two_matches_two_year_gap() {
    let citations = vec![
        make_citation(
            "Two year gap",
            Some(2020),
            Some("Journal"),
            Some("9"),
            Some("200-210"),
            None,
        ),
        make_citation(
            "Two year gap",
            Some(2022),
            Some("Journal"),
            Some("9"),
            Some("200-210"),
            None,
        ),
    ];

    let groups = Deduplicator::builder()
        .year_tolerance(2)
        .build()
        .find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_missing_years_can_match_without_doi() {
    let citations = vec![
        make_citation(
            "Yearless article",
            None,
            Some("Journal"),
            Some("4"),
            Some("50-60"),
            None,
        ),
        make_citation(
            "Yearless article",
            None,
            Some("Journal"),
            Some("4"),
            Some("50-60"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_issue_can_help_match_when_volume_matches_and_page_is_missing() {
    let citations = vec![
        with_issue(
            make_citation(
                "Immune response study",
                Some(2020),
                Some("Journal"),
                Some("12"),
                None,
                None,
            ),
            Some("Issue 4"),
        ),
        with_issue(
            make_citation(
                "Immune response study",
                Some(2020),
                Some("Journal"),
                Some("12"),
                None,
                None,
            ),
            Some("4"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_issue_mismatch_does_not_help_when_volume_and_page_are_missing() {
    let citations = vec![
        with_issue(
            make_citation(
                "Immune response study",
                Some(2020),
                Some("Journal"),
                None,
                None,
                None,
            ),
            Some("4"),
        ),
        with_issue(
            make_citation(
                "Immune response study",
                Some(2020),
                Some("Journal"),
                None,
                None,
                None,
            ),
            Some("5"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_volume_only_fallback_requires_explicit_same_year() {
    let citations = vec![
        make_citation(
            "Immune response study",
            None,
            Some("Journal"),
            Some("12"),
            None,
            None,
        ),
        make_citation(
            "Immune response study",
            None,
            Some("Journal"),
            Some("12"),
            None,
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_no_doi_rejects_when_issue_and_page_conflict_despite_volume_match() {
    let citations = vec![
        with_issue(
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                Some("8"),
                Some("100-110"),
                None,
            ),
            Some("1"),
        ),
        with_issue(
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                Some("8"),
                Some("120-130"),
                None,
            ),
            Some("2"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_no_doi_volume_only_fallback_allows_missing_issue_and_page() {
    let citations = vec![
        with_issue(
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                Some("8"),
                None,
                None,
            ),
            Some("1"),
        ),
        make_citation(
            "Shared study title",
            Some(2020),
            Some("Journal"),
            Some("8"),
            None,
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_no_doi_matches_with_volume_and_issue_when_page_missing() {
    let citations = vec![
        with_issue(
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                Some("8"),
                None,
                None,
            ),
            Some("1"),
        ),
        with_issue(
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                Some("8"),
                None,
                None,
            ),
            Some("Issue 1"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_no_doi_rejects_when_pages_conflict() {
    let citations = vec![
        make_citation(
            "Shared study title",
            Some(2020),
            Some("Journal"),
            None,
            Some("100-110"),
            None,
        ),
        make_citation(
            "Shared study title",
            Some(2020),
            Some("Journal"),
            None,
            Some("120-130"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_no_doi_obvious_exact_duplicate_still_matches() {
    let citations = vec![
        with_issue(
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                Some("8"),
                Some("100-110"),
                None,
            ),
            Some("1"),
        ),
        with_issue(
            make_citation(
                "Shared study title",
                Some(2020),
                Some("Journal"),
                Some("8"),
                Some("100-110"),
                None,
            ),
            Some("1"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_metadata_precheck_preserves_exact_title_fallback_without_journal() {
    let citations = vec![
        make_citation(
            "Shared study title",
            Some(2020),
            None,
            Some("8"),
            Some("100-110"),
            None,
        ),
        make_citation(
            "Shared study title",
            Some(2020),
            None,
            Some("8"),
            Some("100-110"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_transitivity_groups_three_records() {
    let citations = vec![
        make_citation(
            "",
            Some(2020),
            Some("Journal"),
            Some("1"),
            Some("10-20"),
            Some("10.1000/transitive"),
        ),
        make_citation(
            "Linked transitive study",
            Some(2020),
            Some("Journal"),
            Some("1"),
            Some("10-20"),
            Some("10.1000/transitive"),
        ),
        make_citation(
            "Linked transitive study",
            Some(2020),
            Some("Journal"),
            Some("1"),
            Some("10-20"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(group_members(&groups[0]), vec![0, 1, 2]);
}

#[test]
fn test_series_guard_blocks_numeric_and_roman_suffixes() {
    let numeric = vec![
        make_citation(
            "Study part 1",
            Some(2020),
            Some("Journal"),
            Some("5"),
            Some("100-110"),
            None,
        ),
        make_citation(
            "Study part 2",
            Some(2020),
            Some("Journal"),
            Some("5"),
            Some("200-210"),
            None,
        ),
    ];
    let numeric_groups = Deduplicator::new().find_duplicates(&numeric);
    assert_eq!(numeric_groups.len(), 2);

    let roman = vec![
        make_citation(
            "Part ii",
            Some(2020),
            Some("Journal"),
            Some("5"),
            Some("100-110"),
            None,
        ),
        make_citation(
            "Part iii",
            Some(2020),
            Some("Journal"),
            Some("5"),
            Some("200-210"),
            None,
        ),
    ];
    let roman_groups = Deduplicator::new().find_duplicates(&roman);
    assert_eq!(roman_groups.len(), 2);
}

#[test]
fn test_series_guard_blocks_both_doi_matches_without_page_agreement() {
    let citations = vec![
        make_citation(
            "Study part 1",
            Some(2020),
            Some("Journal"),
            Some("5"),
            Some("100-110"),
            Some("10.1000/part1"),
        ),
        make_citation(
            "Study part 2",
            Some(2020),
            Some("Journal"),
            Some("5"),
            Some("200-210"),
            Some("10.1000/part2"),
        ),
    ];

    let sim = strsim::jaro(
        &normalize::normalize_string(&citations[0].title),
        &normalize::normalize_string(&citations[1].title),
    );
    assert!(sim < 0.99, "unexpected both-doi series sim {sim}");

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_series_guard_does_not_trigger_for_identical_trailing_digits() {
    let citations = vec![
        make_citation(
            "Baseline report 2024",
            Some(2024),
            Some("Journal"),
            Some("2"),
            Some("10-12"),
            None,
        ),
        make_citation(
            "Baseline report 2024",
            Some(2024),
            Some("Journal"),
            Some("2"),
            Some("10-12"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_author_guard_blocks_borderline_similarity() {
    let left_title = "integratedgenomicsforcancer";
    let right_title = "integratedgeneticsforcancer";
    let similarity = strsim::jaro_winkler(left_title, right_title);
    assert!(
        (0.93..AUTHOR_GUARD_THRESHOLD).contains(&similarity),
        "expected a borderline similarity, got {similarity}"
    );

    let citations = vec![
        make_citation_with_author(
            left_title,
            "Smith",
            Some(2020),
            Some("Journal"),
            Some("7"),
            Some("100-110"),
        ),
        make_citation_with_author(
            right_title,
            "Jones",
            Some(2020),
            Some("Journal"),
            Some("7"),
            Some("100-110"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
}

#[test]
fn test_boilerplate_prefix_aware_title_similarity_can_recover_match() {
    let citations = vec![
        make_citation(
            "Brief report. Kidney injury markers",
            Some(2020),
            Some("Journal"),
            Some("7"),
            None,
            None,
        ),
        make_citation(
            "Kidney injury markers",
            Some(2020),
            Some("Journal"),
            Some("7"),
            None,
            None,
        ),
    ];

    let deduplicator = Deduplicator::new();
    let records = deduplicator.preprocess_citations(&citations);
    let raw_similarity = strsim::jaro_winkler(&records[0].norm_title, &records[1].norm_title);
    let effective_similarity =
        Deduplicator::max_title_similarity(&records[0], &records[1], strsim::jaro_winkler);
    assert!(
        raw_similarity < NO_DOI_TITLE_SIMILARITY_THRESHOLD,
        "expected raw similarity below threshold, got {raw_similarity}"
    );
    assert!(
        effective_similarity >= NO_DOI_TITLE_SIMILARITY_THRESHOLD,
        "expected stripped-title similarity to recover the match, got {effective_similarity}"
    );

    let groups = deduplicator.find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_pass1_same_doi_without_other_metadata_still_matches() {
    let citations = vec![
        make_citation(
            "Renal biomarker observational study",
            Some(2020),
            None,
            None,
            None,
            Some("10.1000/pass1"),
        ),
        make_citation(
            "Renal biomarker observational studies",
            Some(2021),
            None,
            None,
            None,
            Some("10.1000/pass1"),
        ),
    ];

    let sim = strsim::jaro(
        &normalize::normalize_string(&citations[0].title),
        &normalize::normalize_string(&citations[1].title),
    );
    assert!(
        (PASS1_DOI_TITLE_SANITY..0.99).contains(&sim),
        "unexpected pass1 sim {sim}"
    );

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_pass1_same_doi_rescue_with_strong_corroboration() {
    let citations = vec![
        make_citation_with_author_and_doi(
            "Renal biomarker study in adults",
            "Smith",
            Some(2020),
            Some("Journal"),
            Some("12"),
            Some("100-110"),
            Some("10.1000/rescue"),
        ),
        make_citation_with_author_and_doi(
            "Observational study of renal biomarkers",
            "Smith",
            Some(2021),
            Some("Journal"),
            Some("12"),
            Some("100-118"),
            Some("10.1000/rescue"),
        ),
    ];

    let sim = strsim::jaro(
        &normalize::normalize_string(&citations[0].title),
        &normalize::normalize_string(&citations[1].title),
    );
    assert!(
        sim < PASS1_DOI_TITLE_SANITY,
        "expected rescue similarity below pass1 sanity threshold, got {sim}"
    );

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_pass1_same_doi_rescue_requires_no_author_conflict() {
    let citations = vec![
        make_citation_with_author_and_doi(
            "Renal biomarker study in adults",
            "Smith",
            Some(2020),
            Some("Journal"),
            Some("12"),
            Some("100-110"),
            Some("10.1000/rescue-author"),
        ),
        make_citation_with_author_and_doi(
            "Observational study of renal biomarkers",
            "Jones",
            Some(2021),
            Some("Journal"),
            Some("12"),
            Some("100-118"),
            Some("10.1000/rescue-author"),
        ),
    ];

    let sim = strsim::jaro(
        &normalize::normalize_string(&citations[0].title),
        &normalize::normalize_string(&citations[1].title),
    );
    assert!(
        sim < PASS1_DOI_TITLE_SANITY,
        "expected rescue similarity below pass1 sanity threshold, got {sim}"
    );

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_pass1_same_doi_rescue_requires_page_corroboration() {
    let citations = vec![
        make_citation_with_author_and_doi(
            "Renal biomarker study in adults",
            "Smith",
            Some(2020),
            Some("Journal"),
            Some("12"),
            None,
            Some("10.1000/rescue-metadata"),
        ),
        make_citation_with_author_and_doi(
            "Observational study of renal biomarkers",
            "Smith",
            Some(2021),
            Some("Journal"),
            Some("12"),
            None,
            Some("10.1000/rescue-metadata"),
        ),
    ];

    let sim = strsim::jaro(
        &normalize::normalize_string(&citations[0].title),
        &normalize::normalize_string(&citations[1].title),
    );
    assert!(
        sim < PASS1_DOI_TITLE_SANITY,
        "expected rescue similarity below pass1 sanity threshold, got {sim}"
    );

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_pass1_empty_titles_without_metadata_agreement_stay_separate() {
    let citations = vec![
        make_citation(
            "",
            Some(2020),
            Some("Journal A"),
            Some("1"),
            Some("10-20"),
            Some("10.1000/meta"),
        ),
        make_citation(
            "",
            Some(2021),
            Some("Journal B"),
            Some("2"),
            Some("30-40"),
            Some("10.1000/meta"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_conflicting_normalized_dois_are_not_duplicates() {
    let citations = vec![
        make_citation(
            "Renal biomarker observational study",
            Some(2020),
            Some("Journal"),
            Some("7"),
            Some("100-110"),
            Some("10.1000/left"),
        ),
        make_citation(
            "Observational renal biomarker study",
            Some(2020),
            Some("Journal"),
            Some("7"),
            Some("100-110"),
            Some("10.1000/right"),
        ),
    ];

    let sim = strsim::jaro(
        &normalize::normalize_string(&citations[0].title),
        &normalize::normalize_string(&citations[1].title),
    );
    assert!(
        (0.85..0.99).contains(&sim),
        "unexpected conflicting-doi sim {sim}"
    );

    assert!(
        Deduplicator::new()
            .find_duplicates(&citations)
            .iter()
            .all(|group| group.duplicates.is_empty())
    );
}

#[test]
fn test_bracketed_translated_title_can_match_with_strict_metadata() {
    let citations = vec![
        with_issue(
            with_issn(
                make_citation_with_author(
                    "[Translated title of the study]",
                    "Keyriläinen",
                    Some(2020),
                    Some("Journal"),
                    Some("5"),
                    Some("100-110"),
                ),
                &["1234-5678"],
            ),
            Some("2"),
        ),
        with_issue(
            with_issn(
                make_citation_with_author(
                    "Titulo original del estudio",
                    "Keyrilainen",
                    Some(2020),
                    Some("Journal"),
                    Some("5"),
                    Some("100-110"),
                ),
                &["1234-5678"],
            ),
            Some("2"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].duplicates, vec![1]);
}

#[test]
fn test_bracketed_translated_title_requires_full_metadata_agreement() {
    let citations = vec![
        with_issue(
            make_citation_with_author(
                "[Translated title of the study]",
                "Keyriläinen",
                Some(2020),
                Some("Journal"),
                Some("5"),
                Some("100-110"),
            ),
            Some("2"),
        ),
        with_issue(
            make_citation_with_author(
                "Titulo original del estudio",
                "Keyrilainen",
                Some(2020),
                Some("Journal"),
                Some("5"),
                Some("100-110"),
            ),
            Some("3"),
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}

#[test]
fn test_determinism_across_shuffle_and_repeated_runs() {
    let citations = vec![
        make_citation(
            "Alpha study",
            Some(2020),
            Some("J1"),
            Some("1"),
            Some("10-12"),
            None,
        ),
        make_citation(
            "Alpha study",
            Some(2020),
            Some("J1"),
            Some("1"),
            Some("10-12"),
            None,
        ),
        make_citation(
            "Beta study",
            Some(2021),
            Some("J2"),
            Some("2"),
            Some("20-22"),
            None,
        ),
        make_citation(
            "Gamma study",
            Some(2022),
            Some("J3"),
            Some("3"),
            Some("30-32"),
            None,
        ),
        make_citation(
            "Gamma study",
            Some(2022),
            Some("J3"),
            Some("3"),
            Some("30-32"),
            None,
        ),
    ];

    let deduplicator = Deduplicator::new();
    let first_run = deduplicator.find_duplicates(&citations);
    let second_run = deduplicator.find_duplicates(&citations);
    assert_eq!(first_run, second_run);

    let shuffled_order = vec![4, 1, 3, 0, 2];
    let shuffled_citations = shuffled_order
        .iter()
        .map(|&idx| citations[idx].clone())
        .collect::<Vec<_>>();
    let shuffled_groups = deduplicator.find_duplicates(&shuffled_citations);

    let identity_map = (0..citations.len()).collect::<Vec<_>>();
    assert_eq!(
        canonicalize_groups(&first_run, &identity_map),
        canonicalize_groups(&shuffled_groups, &shuffled_order)
    );
}

#[test]
fn test_parallel_matches_sequential_on_large_fixture() {
    let mut citations = Vec::new();
    for group in 0..40 {
        for duplicate in 0..3 {
            let mut citation = make_citation(
                &format!("Fixture title {group}"),
                if group % 5 == 0 {
                    None
                } else {
                    Some(2020 + (group % 3))
                },
                Some("Fixture Journal"),
                Some(&format!("{group}")),
                Some(&format!("{}-{}", 1000 + group, 1010 + group)),
                None,
            );
            if duplicate == 1 {
                citation.abstract_text = Some("Fixture abstract".to_string());
            }
            if duplicate == 2 {
                citation.journal_abbr = Some("Fixture J".to_string());
            }
            citations.push(citation);
        }
    }

    let sequential = Deduplicator::builder()
        .parallel(false)
        .build()
        .find_duplicates(&citations);
    let parallel = Deduplicator::builder()
        .parallel(true)
        .build()
        .find_duplicates(&citations);

    assert_eq!(sequential, parallel);
    assert!(sequential.len() >= 40);
}

#[test]
fn test_abstract_preference_ignores_empty_string() {
    let mut first = make_citation(
        "Abstract preference",
        Some(2020),
        Some("Journal"),
        Some("2"),
        Some("10-20"),
        Some("10.1000/abstract"),
    );
    first.abstract_text = Some("".to_string());

    let mut second = make_citation(
        "Abstract preference",
        Some(2020),
        Some("Journal"),
        Some("2"),
        Some("10-20"),
        Some("10.1000/abstract"),
    );
    second.abstract_text = Some("Real abstract".to_string());

    let groups = Deduplicator::new().find_duplicates(&[first, second.clone()]);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].unique, 1);
}

#[test]
fn test_sources_shorter_and_longer_are_tolerated() {
    let citations = vec![
        make_citation(
            "Source preference",
            Some(2020),
            Some("Journal"),
            Some("2"),
            Some("10-12"),
            Some("10.1000/source"),
        ),
        make_citation(
            "Source preference",
            Some(2020),
            Some("Journal"),
            Some("2"),
            Some("10-12"),
            Some("10.1000/source"),
        ),
    ];

    let deduplicator = Deduplicator::builder()
        .source_preferences(["PubMed", "Embase"])
        .build();

    let shorter = deduplicator.find_duplicates_with_sources(&citations, &["PubMed"]);
    assert_eq!(shorter.len(), 1);
    assert_eq!(shorter[0].unique, 0);

    let longer =
        deduplicator.find_duplicates_with_sources(&citations, &["Embase", "PubMed", "Extra"]);
    assert_eq!(longer.len(), 1);
    assert_eq!(longer[0].unique, 1);
}

#[test]
fn test_every_input_index_appears_exactly_once() {
    let citations = vec![
        make_citation(
            "One",
            Some(2020),
            Some("J1"),
            Some("1"),
            Some("10-11"),
            None,
        ),
        make_citation(
            "One",
            Some(2020),
            Some("J1"),
            Some("1"),
            Some("10-11"),
            None,
        ),
        make_citation(
            "Two",
            Some(2021),
            Some("J2"),
            Some("2"),
            Some("20-21"),
            None,
        ),
        make_citation(
            "Three",
            Some(2022),
            Some("J3"),
            Some("3"),
            Some("30-31"),
            None,
        ),
    ];

    let groups = Deduplicator::new().find_duplicates(&citations);

    assert_eq!(covered_indices(&groups), vec![0, 1, 2, 3]);
}

#[test]
#[should_panic(expected = "within (0.0, 1.0]")]
fn test_builder_panics_on_zero_threshold() {
    let _ = Deduplicator::builder().exact_title_threshold(0.0).build();
}

#[test]
#[should_panic(expected = "within (0.0, 1.0]")]
fn test_builder_panics_on_threshold_above_one() {
    let _ = Deduplicator::builder().no_doi_title_threshold(1.5).build();
}
