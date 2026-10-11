//! The PubMed XML and `.nbib` parsers read the same records the same way.
//!
//! The fixtures are eight real records, fetched from E-utilities `efetch` as
//! XML and as MEDLINE text (`rettype=medline`); only the XML `<ReferenceList>`
//! elements, which neither parser reads, were removed. They cover cases that
//! once differed: superscripts, an "UNLABELLED" abstract label, affiliation
//! identifiers, a chapter with editors and an author suffix, a hyphenated
//! given name, a group author, supplementary concepts, and retraction links.
//!
//! MEDLINE text transliterates to ASCII, so text is compared only where the
//! XML value is ASCII too.

#![cfg(all(feature = "pubmed", feature = "xml"))]

use biblib::{Citation, CitationParser, PubMedParser, PubMedXmlParser};
use std::collections::HashMap;

fn by_pmid(citations: Vec<Citation>) -> HashMap<String, Citation> {
    citations
        .into_iter()
        .map(|c| (c.pmid.clone().unwrap(), c))
        .collect()
}

fn parse_both() -> (HashMap<String, Citation>, HashMap<String, Citation>) {
    let xml = PubMedXmlParser::new()
        .parse(include_str!("fixtures/pubmed/parity.xml"))
        .unwrap();
    let nbib = PubMedParser::new()
        .parse(include_str!("fixtures/pubmed/parity.nbib"))
        .unwrap();
    assert_eq!(xml.len(), 8);
    assert_eq!(nbib.len(), 8);
    (by_pmid(xml), by_pmid(nbib))
}

fn assert_same_if_ascii(pmid: &str, field: &str, xml: &str, nbib: &str) {
    if xml.is_ascii() {
        assert_eq!(xml, nbib, "{pmid} {field}");
    }
}

#[test]
fn standard_fields_match() {
    let (xml, nbib) = parse_both();
    for (pmid, n) in &nbib {
        let x = &xml[pmid];
        assert_eq!(x.citation_type, n.citation_type, "{pmid} type");
        assert_eq!(x.journal, n.journal, "{pmid} journal");
        assert_eq!(x.journal_abbr, n.journal_abbr, "{pmid} journal_abbr");
        assert_eq!(x.date, n.date, "{pmid} date");
        assert_eq!(x.volume, n.volume, "{pmid} volume");
        assert_eq!(x.issue, n.issue, "{pmid} issue");
        assert_eq!(x.pages, n.pages, "{pmid} pages");
        assert_eq!(x.doi, n.doi, "{pmid} doi");
        assert_eq!(x.accession_number, n.accession_number, "{pmid} accession");
        assert_eq!(x.pmc_id, n.pmc_id, "{pmid} pmc_id");
        assert_eq!(x.language, n.language, "{pmid} language");
        assert_eq!(x.mesh_terms, n.mesh_terms, "{pmid} mesh");
        assert_eq!(x.publisher, n.publisher, "{pmid} publisher");
        assert_same_if_ascii(pmid, "title", &x.title, &n.title);
        assert_same_if_ascii(
            pmid,
            "abstract",
            x.abstract_text.as_deref().unwrap_or_default(),
            n.abstract_text.as_deref().unwrap_or_default(),
        );
        assert_eq!(x.keywords.len(), n.keywords.len(), "{pmid} keywords");
        for (xk, nk) in x.keywords.iter().zip(&n.keywords) {
            assert_same_if_ascii(pmid, "keyword", xk, nk);
        }
    }
}

#[test]
fn authors_match() {
    let (xml, nbib) = parse_both();
    for (pmid, n) in &nbib {
        let x = &xml[pmid];
        assert_eq!(x.authors.len(), n.authors.len(), "{pmid} author count");
        for (xa, na) in x.authors.iter().zip(&n.authors) {
            assert_same_if_ascii(pmid, "author", &xa.name, &na.name);
            if let (Some(xg), Some(ng)) = (&xa.given_name, &na.given_name) {
                assert_same_if_ascii(pmid, "given name", xg, ng);
            }
            assert_eq!(xa.affiliations.len(), na.affiliations.len(), "{pmid}");
            for (xf, nf) in xa.affiliations.iter().zip(&na.affiliations) {
                assert_same_if_ascii(pmid, "affiliation", xf, nf);
            }
        }
    }
}

#[test]
fn extra_fields_match() {
    let (xml, nbib) = parse_both();
    let tags = [
        "BTI", "CRDT", "CTDT", "DCOM", "DEP", "DRDT", "ED", "EDAT", "FED", "GR", "JID", "LR",
        "MHDA", "OTO", "OWN", "PHST", "PL", "PMCR", "PST", "RN", "SB",
    ];
    for (pmid, n) in &nbib {
        let x = &xml[pmid];
        for tag in tags {
            let (xv, nv) = (x.extra_fields.get(tag), n.extra_fields.get(tag));
            if xv.is_some_and(|values| values.iter().all(|v| v.is_ascii())) || xv.is_none() {
                assert_eq!(xv, nv, "{pmid} {tag}");
            }
        }
    }
}
