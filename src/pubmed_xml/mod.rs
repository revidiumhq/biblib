//! PubMed XML format parser implementation.
//!
//! Parses the XML that PubMed exports ("Save" → "PubMed" as XML) and the
//! E-utilities `efetch` endpoint returns: a `<PubmedArticleSet>` of journal
//! articles (`<PubmedArticle>`) and NCBI Bookshelf books and chapters
//! (`<PubmedBookArticle>`).
//!
//! Field values follow the PubMed (`.nbib`) parser, so the XML and `.nbib`
//! exports of the same search give the same citations: publication types in
//! `citation_type`, MeSH headings as `Descriptor/*Qualifier`, ISSNs as
//! `1234-5678 (Electronic)`, and the MEDLINE journal abbreviation. Book editors
//! are listed with the authors.
//!
//! Only the predefined XML entities and character references are resolved;
//! entities declared in a DTD are never expanded.
//!
//! # Example
//!
//! ```
//! use biblib::{CitationParser, PubMedXmlParser};
//!
//! let xml = r#"<?xml version="1.0" ?>
//! <!DOCTYPE PubmedArticleSet PUBLIC "-//NLM//DTD PubMedArticle, 1st January 2025//EN" "https://dtd.nlm.nih.gov/ncbi/pubmed/out/pubmed_250101.dtd">
//! <PubmedArticleSet>
//!   <PubmedArticle>
//!     <MedlineCitation>
//!       <PMID Version="1">12345678</PMID>
//!       <Article>
//!         <Journal>
//!           <JournalIssue><Volume>15</Volume><PubDate><Year>2023</Year><Month>Mar</Month></PubDate></JournalIssue>
//!           <Title>Journal of Science</Title>
//!         </Journal>
//!         <ArticleTitle>Effect of <i>E. coli</i> on uptake.</ArticleTitle>
//!         <AuthorList>
//!           <Author><LastName>Doe</LastName><ForeName>John</ForeName></Author>
//!         </AuthorList>
//!       </Article>
//!     </MedlineCitation>
//!     <PubmedData>
//!       <ArticleIdList><ArticleId IdType="doi">10.1234/example</ArticleId></ArticleIdList>
//!     </PubmedData>
//!   </PubmedArticle>
//! </PubmedArticleSet>"#;
//!
//! let citations = PubMedXmlParser::new().parse(xml).unwrap();
//! assert_eq!(citations[0].title, "Effect of E. coli on uptake.");
//! assert_eq!(citations[0].pmid.as_deref(), Some("12345678"));
//! assert_eq!(citations[0].doi.as_deref(), Some("10.1234/example"));
//! assert_eq!(citations[0].date.as_ref().map(|d| d.month), Some(Some(3)));
//! ```

mod parse;

use crate::error::ParseError;
use crate::{Citation, CitationParser};
pub(crate) use parse::looks_like_pubmed_xml;
use parse::parse_pubmed_xml;

/// Parser for PubMed XML (`<PubmedArticleSet>`) exports.
#[derive(Debug, Clone, Default)]
pub struct PubMedXmlParser;

impl PubMedXmlParser {
    /// Creates a new PubMed XML parser instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use biblib::PubMedXmlParser;
    /// let parser = PubMedXmlParser::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CitationParser for PubMedXmlParser {
    /// Parses PubMed XML content into citations, in document order.
    ///
    /// # Errors
    ///
    /// Returns `ParseError` for malformed XML, input truncated inside a
    /// record, or a reference to an entity other than the predefined ones.
    fn parse(&self, input: &str) -> Result<Vec<Citation>, ParseError> {
        parse_pubmed_xml(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ValueError;
    use crate::{CitationFormat, Date};

    fn set(records: &str) -> String {
        format!(
            "<?xml version=\"1.0\" ?>\n<!DOCTYPE PubmedArticleSet PUBLIC \"-//NLM//DTD PubMedArticle, 1st January 2025//EN\" \"https://dtd.nlm.nih.gov/ncbi/pubmed/out/pubmed_250101.dtd\">\n<PubmedArticleSet>{}</PubmedArticleSet>",
            records
        )
    }

    fn parse(records: &str) -> Vec<Citation> {
        PubMedXmlParser::new().parse(&set(records)).unwrap()
    }

    const ARTICLE: &str = r#"<PubmedArticle><MedlineCitation Status="MEDLINE" Owner="NLM"><PMID Version="1">31000001</PMID>
<Article PubModel="Print-Electronic"><Journal><ISSN IssnType="Electronic">1533-4406</ISSN>
<JournalIssue CitedMedium="Internet"><Volume>380</Volume><Issue>12</Issue><PubDate><Year>2019</Year><Month>Mar</Month><Day>21</Day></PubDate></JournalIssue>
<Title>The New England journal of medicine</Title><ISOAbbreviation>N. Engl. J. Med.</ISOAbbreviation></Journal>
<ArticleTitle>Effect of <i>E. coli</i> on <sup>13</sup>C uptake &amp; more.</ArticleTitle>
<Pagination><MedlinePgn>1101-1110</MedlinePgn></Pagination>
<ELocationID EIdType="doi" ValidYN="Y">10.1056/NEJMoa1813219</ELocationID>
<Abstract><AbstractText Label="BACKGROUND" NlmCategory="BACKGROUND">Why &#x2013; and how.</AbstractText><AbstractText Label="RESULTS">It worked.</AbstractText></Abstract>
<AuthorList CompleteYN="Y"><Author ValidYN="Y"><LastName>Smith</LastName><ForeName>Jane A</ForeName><Initials>JA</Initials>
<AffiliationInfo><Affiliation>Dept A, Univ B.</Affiliation></AffiliationInfo></Author>
<Author ValidYN="Y"><LastName>Wang</LastName><Initials>L</Initials></Author>
<Author ValidYN="Y"><LastName>Wang</LastName><Initials>L</Initials></Author>
<Author ValidYN="Y"><CollectiveName>Example Trial Group</CollectiveName></Author></AuthorList>
<Language>eng</Language>
<PublicationTypeList><PublicationType UI="D016428">Journal Article</PublicationType><PublicationType UI="D016449">Randomized Controlled Trial</PublicationType></PublicationTypeList>
</Article>
<MedlineJournalInfo><Country>United States</Country><MedlineTA>N Engl J Med</MedlineTA><NlmUniqueID>0255562</NlmUniqueID><ISSNLinking>0028-4793</ISSNLinking></MedlineJournalInfo>
<MeshHeadingList><MeshHeading><DescriptorName UI="D001241" MajorTopicYN="N">Aspirin</DescriptorName><QualifierName UI="Q000008" MajorTopicYN="N">administration &amp; dosage</QualifierName><QualifierName UI="Q000627" MajorTopicYN="Y">therapeutic use</QualifierName></MeshHeading>
<MeshHeading><DescriptorName UI="D006801" MajorTopicYN="Y">Humans</DescriptorName></MeshHeading></MeshHeadingList>
<KeywordList Owner="NOTNLM"><Keyword MajorTopicYN="N">prevention</Keyword><Keyword MajorTopicYN="N">aspirin</Keyword></KeywordList>
</MedlineCitation>
<PubmedData><History><PubMedPubDate PubStatus="pubmed"><Year>2019</Year><Month>3</Month><Day>22</Day></PubMedPubDate></History>
<ArticleIdList><ArticleId IdType="pubmed">31000001</ArticleId><ArticleId IdType="doi">10.1056/NEJMoa1813219</ArticleId><ArticleId IdType="pmc">PMC6000001</ArticleId></ArticleIdList>
<ReferenceList><Reference><Citation>Cited work.</Citation><ArticleIdList><ArticleId IdType="doi">10.1000/cited</ArticleId></ArticleIdList></Reference></ReferenceList>
</PubmedData></PubmedArticle>"#;

    #[test]
    fn parses_a_journal_article() {
        let citations = parse(ARTICLE);
        assert_eq!(citations.len(), 1);
        let c = &citations[0];
        assert_eq!(c.title, "Effect of E. coli on 13C uptake & more.");
        assert_eq!(c.pmid.as_deref(), Some("31000001"));
        assert_eq!(c.pmc_id.as_deref(), Some("PMC6000001"));
        assert_eq!(c.doi.as_deref(), Some("10.1056/NEJMoa1813219"));
        assert_eq!(
            c.journal.as_deref(),
            Some("The New England journal of medicine")
        );
        assert_eq!(c.journal_abbr.as_deref(), Some("N Engl J Med"));
        assert_eq!(
            c.date,
            Some(Date {
                year: 2019,
                month: Some(3),
                day: Some(21)
            })
        );
        assert_eq!(c.volume.as_deref(), Some("380"));
        assert_eq!(c.issue.as_deref(), Some("12"));
        assert_eq!(c.pages.as_deref(), Some("1101-1110"));
        assert_eq!(
            c.issn,
            vec!["1533-4406 (Electronic)", "0028-4793 (Linking)"]
        );
        assert_eq!(
            c.abstract_text.as_deref(),
            Some("BACKGROUND: Why \u{2013} and how. RESULTS: It worked.")
        );
        assert_eq!(c.language.as_deref(), Some("eng"));
        assert_eq!(
            c.citation_type,
            vec!["Journal Article", "Randomized Controlled Trial"]
        );
        assert_eq!(
            c.mesh_terms,
            vec![
                "Aspirin/administration & dosage/*therapeutic use",
                "*Humans"
            ]
        );
        assert_eq!(c.keywords, vec!["prevention", "aspirin"]);
    }

    #[test]
    fn keeps_every_author_including_same_names_and_groups() {
        let c = &parse(ARTICLE)[0];
        let names: Vec<_> = c.authors.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Smith", "Wang", "Wang", "Example Trial Group"]);
        assert_eq!(c.authors[0].given_name.as_deref(), Some("Jane"));
        assert_eq!(c.authors[0].middle_name.as_deref(), Some("A"));
        assert_eq!(c.authors[0].affiliations, vec!["Dept A, Univ B."]);
        assert_eq!(c.authors[1].given_name.as_deref(), Some("L"));
        assert_eq!(c.authors[3].given_name, None);
    }

    #[test]
    fn ignores_reference_list_identifiers() {
        let c = &parse(&ARTICLE.replace(
            "<ELocationID EIdType=\"doi\" ValidYN=\"Y\">10.1056/NEJMoa1813219</ELocationID>",
            "",
        ))[0];
        // From the article's own ArticleIdList, not the cited work's.
        assert_eq!(c.doi.as_deref(), Some("10.1056/NEJMoa1813219"));
    }

    const CHAPTER: &str = r#"<PubmedBookArticle><BookDocument><PMID Version="1">20301295</PMID>
<ArticleIdList><ArticleId IdType="bookaccession">NBK1116</ArticleId></ArticleIdList>
<Book><Publisher><PublisherName>University of Washington, Seattle</PublisherName><PublisherLocation>Seattle (WA)</PublisherLocation></Publisher>
<BookTitle book="gene">GeneReviews<sup>®</sup></BookTitle><PubDate><Year>1993</Year></PubDate>
<AuthorList Type="editors"><Author><LastName>Adam</LastName><ForeName>Margaret P</ForeName></Author></AuthorList>
<CollectionTitle book="gene">GeneReviews</CollectionTitle><Medium>Internet</Medium></Book>
<LocationLabel Type="chapter">Chapter 1</LocationLabel>
<ArticleTitle book="gene" part="cf">Cystic Fibrosis</ArticleTitle><Language>eng</Language>
<AuthorList Type="authors"><Author><LastName>Author</LastName><ForeName>Ann</ForeName></Author></AuthorList>
<PublicationType UI="D016454">Review</PublicationType>
<Abstract><AbstractText Label="CLINICAL CHARACTERISTICS">Text.</AbstractText></Abstract>
</BookDocument><PubmedBookData><History><PubMedPubDate PubStatus="pubmed"><Year>2010</Year><Month>3</Month><Day>20</Day></PubMedPubDate></History>
<PublicationStatus>ppublish</PublicationStatus><ArticleIdList><ArticleId IdType="pubmed">20301295</ArticleId></ArticleIdList></PubmedBookData></PubmedBookArticle>"#;

    #[test]
    fn parses_bookshelf_chapters_and_books_in_document_order() {
        let whole_book = r#"<PubmedBookArticle><BookDocument><PMID>30000001</PMID><Book><BookTitle book="x">A Whole Book</BookTitle>
<PubDate><Year>2018</Year><Month>01</Month></PubDate><AuthorList Type="authors"><Author><LastName>Writer</LastName><Initials>W</Initials></Author></AuthorList>
<Isbn>9780000000001</Isbn><ELocationID EIdType="doi">10.1000/book</ELocationID></Book></BookDocument></PubmedBookArticle>"#;
        let citations = parse(&format!("{CHAPTER}{ARTICLE}{whole_book}"));
        let pmids: Vec<_> = citations.iter().map(|c| c.pmid.as_deref()).collect();
        assert_eq!(
            pmids,
            vec![Some("20301295"), Some("31000001"), Some("30000001")]
        );

        let chapter = &citations[0];
        assert_eq!(chapter.title, "Cystic Fibrosis");
        assert_eq!(chapter.extra_fields["BTI"], vec!["GeneReviews®"]);
        assert_eq!(chapter.extra_fields["CTI"], vec!["GeneReviews"]);
        assert_eq!(chapter.journal, None);
        // Chapter authors first, then the book's editors.
        let names: Vec<_> = chapter.authors.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Author", "Adam"]);
        assert_eq!(chapter.accession_number.as_deref(), Some("NBK1116"));
        assert_eq!(
            chapter.publisher.as_deref(),
            Some("University of Washington, Seattle")
        );
        assert_eq!(chapter.date.as_ref().map(|d| d.year), Some(1993));
        assert_eq!(chapter.citation_type, vec!["Review"]);
        assert_eq!(
            chapter.abstract_text.as_deref(),
            Some("CLINICAL CHARACTERISTICS: Text.")
        );

        let book = &citations[2];
        assert_eq!(book.title, "A Whole Book");
        assert!(!book.extra_fields.contains_key("BTI"));
        assert_eq!(book.authors[0].name, "Writer");
        assert_eq!(book.issn, vec!["9780000000001"]);
        assert_eq!(book.doi.as_deref(), Some("10.1000/book"));
        assert_eq!(
            book.date,
            Some(Date {
                year: 2018,
                month: Some(1),
                day: None
            })
        );
    }

    #[test]
    fn reads_medline_dates() {
        let with_date = |pub_date: &str| {
            let xml = ARTICLE.replace(
                "<PubDate><Year>2019</Year><Month>Mar</Month><Day>21</Day></PubDate>",
                pub_date,
            );
            parse(&xml)[0].date.clone()
        };
        assert_eq!(
            with_date("<PubDate><MedlineDate>1998 Dec-1999 Jan</MedlineDate></PubDate>")
                .map(|d| d.year),
            Some(1998)
        );
        assert_eq!(
            with_date("<PubDate><MedlineDate>Winter 2018</MedlineDate></PubDate>").map(|d| d.year),
            Some(2018)
        );
        assert_eq!(with_date("<PubDate></PubDate>"), None);
    }

    #[test]
    fn empty_set_parses_to_no_citations() {
        assert!(parse("").is_empty());
        assert!(PubMedXmlParser::new().parse("").unwrap().is_empty());
    }

    #[test]
    fn rejects_dtd_entities() {
        let xml = r#"<?xml version="1.0"?><!DOCTYPE PubmedArticleSet [<!ENTITY e SYSTEM "file:///etc/passwd">]>
<PubmedArticleSet><PubmedArticle><MedlineCitation><PMID>1</PMID><Article><ArticleTitle>&e;</ArticleTitle></Article></MedlineCitation></PubmedArticle></PubmedArticleSet>"#;
        let err = PubMedXmlParser::new().parse(xml).unwrap_err();
        assert_eq!(err.format, CitationFormat::PubMedXml);
        assert_eq!(err.line, Some(2));
        assert!(
            matches!(&err.error, ValueError::Syntax(message) if message.contains("unsupported entity reference &e;")),
            "{err:?}"
        );
    }

    #[test]
    fn rejects_truncated_and_mismatched_input() {
        let truncated = set(ARTICLE);
        let truncated = &truncated[..truncated.find("</PubmedData>").unwrap()];
        let err = PubMedXmlParser::new().parse(truncated).unwrap_err();
        assert!(
            matches!(&err.error, ValueError::Syntax(m) if m.contains("Unexpected EOF")),
            "{err:?}"
        );

        let mismatched =
            set("<PubmedArticle><MedlineCitation><PMID>1</PMID></Article></PubmedArticle>");
        assert!(PubMedXmlParser::new().parse(&mismatched).is_err());
    }

    #[test]
    fn rejects_hostile_nesting_without_overflowing() {
        let deep = format!(
            "<PubmedArticle>{}{}</PubmedArticle>",
            "<a>".repeat(10_000),
            "</a>".repeat(10_000)
        );
        let err = PubMedXmlParser::new().parse(&set(&deep)).unwrap_err();
        assert!(
            matches!(&err.error, ValueError::Syntax(m) if m.contains("nested")),
            "{err:?}"
        );
    }

    #[test]
    fn detects_pubmed_xml_by_root_element() {
        assert!(looks_like_pubmed_xml(&set("")));
        assert!(looks_like_pubmed_xml("\u{feff}  <PubmedArticleSet/>"));
        assert!(looks_like_pubmed_xml("<!-- c --><PubmedArticleSet>"));
        assert!(!looks_like_pubmed_xml(
            "<?xml version=\"1.0\"?><xml><records><record/></records></xml>"
        ));
        assert!(!looks_like_pubmed_xml(
            "TY  - JOUR\nTI  - <PubmedArticleSet>\nER  -"
        ));
        assert!(!looks_like_pubmed_xml(""));
    }
}
