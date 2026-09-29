//! PubMed XML parsing implementation.
//!
//! Each top-level `<PubmedArticle>` or `<PubmedBookArticle>` is read into a
//! small element tree, then mapped to a [`Citation`]. Values follow the
//! PubMed (`.nbib`) parser's conventions, so both exports of the same search
//! give the same fields: `PT` → `citation_type`, `MH`-style MeSH headings,
//! `IS`-style ISSNs, and the MEDLINE journal abbreviation.

use crate::error::{ParseError, SourceSpan, ValueError};
use crate::utils::{
    buffer_position_to_line_number, parse_month_name, parse_pubmed_date, split_given_and_middle,
    utf8_bom_len, xml_reader_position,
};
use crate::{Author, Citation, CitationFormat, Date};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

/// Elements nest a few levels deep in real exports; this bounds the recursion
/// on hostile input.
const MAX_DEPTH: usize = 128;

#[derive(Debug, Default)]
struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Node>,
}

#[derive(Debug)]
enum Node {
    Element(Element),
    Text(String),
}

static EMPTY: Element = Element {
    name: String::new(),
    attributes: Vec::new(),
    children: Vec::new(),
};

impl Element {
    fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |element| element.name == name)
    }

    /// The first child with this name, or an empty element.
    fn child(&self, name: &str) -> &Element {
        self.elements()
            .find(|element| element.name == name)
            .unwrap_or(&EMPTY)
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// All descendant text in reading order, so inline markup such as
    /// `<i>E. coli</i>` keeps its words in place, with whitespace collapsed.
    fn text(&self) -> String {
        fn collect(element: &Element, out: &mut String) {
            for node in &element.children {
                match node {
                    Node::Text(text) => out.push_str(text),
                    Node::Element(child) => collect(child, out),
                }
            }
        }
        let mut raw = String::new();
        collect(self, &mut raw);
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn text_opt(&self) -> Option<String> {
        Some(self.text()).filter(|text| !text.is_empty())
    }

    fn child_text(&self, name: &str) -> Option<String> {
        self.child(name).text_opt()
    }
}

fn syntax_error(content: &str, pos: usize, message: String) -> ParseError {
    ParseError::at_line(
        buffer_position_to_line_number(content, pos),
        CitationFormat::PubMedXml,
        ValueError::Syntax(message),
    )
}

fn read_attributes(
    start: &BytesStart,
    reader: &Reader<&[u8]>,
    content: &str,
) -> Result<Vec<(String, String)>, ParseError> {
    let pos = xml_reader_position(reader, content);
    start
        .attributes()
        .map(|attribute| {
            let attribute = attribute
                .map_err(|e| syntax_error(content, pos, format!("Invalid attribute: {}", e)))?;
            let value = attribute
                .decoded_and_normalized_value(XmlVersion::Explicit1_1, reader.decoder())
                .map_err(|e| {
                    syntax_error(content, pos, format!("Invalid attribute value: {}", e))
                })?;
            Ok((
                String::from_utf8_lossy(attribute.key.as_ref()).into_owned(),
                value.into_owned(),
            ))
        })
        .collect()
}

/// Read the element opened by `start` up to its end tag.
fn read_element(
    reader: &mut Reader<&[u8]>,
    start: &BytesStart,
    content: &str,
    start_pos: usize,
    depth: usize,
) -> Result<Element, ParseError> {
    if depth > MAX_DEPTH {
        return Err(syntax_error(
            content,
            start_pos,
            format!("Elements nested more than {} levels deep", MAX_DEPTH),
        ));
    }
    let mut element = Element {
        name: String::from_utf8_lossy(start.name().as_ref()).into_owned(),
        attributes: read_attributes(start, reader, content)?,
        children: Vec::new(),
    };

    loop {
        let pos = xml_reader_position(reader, content);
        let text_error = |detail: String| {
            syntax_error(
                content,
                pos,
                format!("Invalid XML text content: {}", detail),
            )
        };
        match reader.read_event() {
            Ok(Event::Start(child)) => {
                let child = read_element(reader, &child, content, pos, depth + 1)?;
                element.children.push(Node::Element(child));
            }
            Ok(Event::Empty(child)) => {
                element.children.push(Node::Element(Element {
                    name: String::from_utf8_lossy(child.name().as_ref()).into_owned(),
                    attributes: read_attributes(&child, reader, content)?,
                    children: Vec::new(),
                }));
            }
            Ok(Event::Text(text)) => {
                let text = text
                    .xml_content(XmlVersion::Explicit1_1)
                    .map_err(|e| text_error(e.to_string()))?;
                element.children.push(Node::Text(text.into_owned()));
            }
            Ok(Event::CData(text)) => {
                let text = text.decode().map_err(|e| text_error(e.to_string()))?;
                element.children.push(Node::Text(text.into_owned()));
            }
            // `&amp;`, `&#x3B1;` etc. arrive as separate events. Only the
            // predefined entities are known: a DTD's entities are never
            // expanded, so a reference to one is an error.
            Ok(Event::GeneralRef(reference)) => {
                let ch = match reference
                    .resolve_char_ref()
                    .map_err(|e| text_error(e.to_string()))?
                {
                    Some(ch) => ch,
                    None => {
                        let name = reference.decode().map_err(|e| text_error(e.to_string()))?;
                        match name.as_ref() {
                            "lt" => '<',
                            "gt" => '>',
                            "amp" => '&',
                            "apos" => '\'',
                            "quot" => '"',
                            other => {
                                return Err(text_error(format!(
                                    "unsupported entity reference &{};",
                                    other
                                )));
                            }
                        }
                    }
                };
                element.children.push(Node::Text(ch.to_string()));
            }
            Ok(Event::End(_)) => return Ok(element),
            Ok(Event::Eof) => {
                return Err(syntax_error(
                    content,
                    start_pos,
                    format!(
                        "Unexpected EOF while looking for closing tag '{}'",
                        element.name
                    ),
                )
                .with_span(SourceSpan::new(
                    start_pos,
                    xml_reader_position(reader, content),
                )));
            }
            Err(e) => {
                return Err(syntax_error(
                    content,
                    pos,
                    format!("XML parsing error: {}", e),
                ));
            }
            _ => (),
        }
    }
}

/// Whether `content` is a PubMed XML export: its root element is
/// `<PubmedArticleSet>`.
pub(crate) fn looks_like_pubmed_xml(content: &str) -> bool {
    let mut reader = Reader::from_str(&content[utf8_bom_len(content)..]);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                return e.name().as_ref() == b"PubmedArticleSet";
            }
            Ok(Event::Decl(_) | Event::DocType(_) | Event::Comment(_) | Event::PI(_)) => (),
            Ok(Event::Text(text)) if text.iter().all(u8::is_ascii_whitespace) => (),
            _ => return false,
        }
    }
}

/// Parse PubMed XML content into citations, in document order.
pub(crate) fn parse_pubmed_xml(content: &str) -> Result<Vec<Citation>, ParseError> {
    let mut reader = Reader::from_str(&content[utf8_bom_len(content)..]);
    // Not `trim_text(true)`: it trims each text event, which drops the spaces
    // around inline markup and entity references. `Element::text` collapses
    // whitespace once per field instead.
    reader.config_mut().trim_text(false);

    let mut citations = Vec::new();
    loop {
        let pos = xml_reader_position(&reader, content);
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"PubmedArticle" => {
                    let element = read_element(&mut reader, &e, content, pos, 0)?;
                    citations.push(article_to_citation(&element));
                }
                b"PubmedBookArticle" => {
                    let element = read_element(&mut reader, &e, content, pos, 0)?;
                    citations.push(book_to_citation(&element));
                }
                _ => (),
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(syntax_error(
                    content,
                    pos,
                    format!("XML parsing error: {}", e),
                ));
            }
            _ => (),
        }
    }
    Ok(citations)
}

fn texts<'a>(elements: impl Iterator<Item = &'a Element>) -> Vec<String> {
    elements.filter_map(Element::text_opt).collect()
}

/// Every author list, including book editors (`Type="editors"`), in order.
fn authors(lists: &[&Element]) -> Vec<Author> {
    lists
        .iter()
        .flat_map(|element| element.all("AuthorList"))
        .flat_map(|list| list.all("Author"))
        .filter_map(|author| {
            let affiliations = texts(
                author
                    .all("AffiliationInfo")
                    .map(|info| info.child("Affiliation")),
            );
            if let Some(name) = author.child_text("CollectiveName") {
                return Some(Author {
                    name,
                    given_name: None,
                    middle_name: None,
                    affiliations,
                });
            }
            let name = author.child_text("LastName")?;
            let given = author
                .child_text("ForeName")
                .or_else(|| author.child_text("Initials"));
            let (given_name, middle_name) = given
                .as_deref()
                .map(split_given_and_middle)
                .unwrap_or((None, None));
            Some(Author {
                name,
                given_name,
                middle_name,
                affiliations,
            })
        })
        .collect()
}

/// `<PubDate>`: Year / Month / Day, or a free-text `<MedlineDate>` such as
/// "1998 Dec-1999 Jan".
fn date(element: &Element) -> Option<Date> {
    if let Some(year) = element
        .child_text("Year")
        .and_then(|y| y.parse::<i32>().ok())
    {
        let month = element
            .child_text("Month")
            .and_then(|month| match month.parse::<u8>() {
                Ok(number) => (1..=12).contains(&number).then_some(number),
                Err(_) => parse_month_name(&month),
            });
        let day = element
            .child_text("Day")
            .and_then(|day| day.parse::<u8>().ok())
            .filter(|day| (1..=31).contains(day));
        return Some(Date { year, month, day });
    }
    let medline = element.child_text("MedlineDate")?;
    parse_pubmed_date(&medline).or_else(|| {
        medline
            .split(|c: char| !c.is_ascii_digit())
            .find(|token| token.len() == 4)
            .and_then(|token| token.parse().ok())
            .map(|year| Date {
                year,
                month: None,
                day: None,
            })
    })
}

/// Structured abstracts keep their labels, as in `.nbib` `AB` lines.
fn abstract_text(element: &Element) -> Option<String> {
    let parts: Vec<String> = element
        .child("Abstract")
        .all("AbstractText")
        .filter_map(|part| {
            let text = part.text_opt()?;
            Some(match part.attribute("Label").map(str::trim) {
                Some(label) if !label.is_empty() => format!("{}: {}", label, text),
                _ => text,
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// `MH`-style headings: "Descriptor/Qualifier", `*` marking a major topic.
fn mesh_terms(citation: &Element) -> Vec<String> {
    let major = |element: &Element| {
        if element.attribute("MajorTopicYN") == Some("Y") {
            "*"
        } else {
            ""
        }
    };
    citation
        .child("MeshHeadingList")
        .all("MeshHeading")
        .filter_map(|heading| {
            let descriptor = heading.child("DescriptorName");
            let mut term = format!("{}{}", major(descriptor), descriptor.text_opt()?);
            for qualifier in heading.all("QualifierName") {
                if let Some(text) = qualifier.text_opt() {
                    term.push('/');
                    term.push_str(major(qualifier));
                    term.push_str(&text);
                }
            }
            Some(term)
        })
        .collect()
}

fn id_of(ids: &[&Element], id_type: &str) -> Option<String> {
    ids.iter()
        .find(|id| id.attribute("IdType") == Some(id_type))
        .and_then(|id| id.text_opt())
}

fn elocation_doi(element: &Element) -> Option<String> {
    element
        .all("ELocationID")
        .find(|id| id.attribute("EIdType") == Some("doi"))
        .and_then(Element::text_opt)
}

/// Multiple values are joined like the `.nbib` parser joins repeated tags.
fn joined(values: Vec<String>) -> Option<String> {
    (!values.is_empty()).then(|| values.join(" AND "))
}

/// Left-pad to two characters with `0`, as `.nbib` dates are written.
fn two_digits(date: &Element, name: &str) -> Option<String> {
    date.child_text(name).map(|value| format!("{:0>2}", value))
}

/// A `<PubMedPubDate>` as `.nbib` writes it: `2020/12/11 06:00`, with
/// `00:00` when the time is missing.
fn history_stamp(date: &Element) -> Option<String> {
    Some(format!(
        "{}/{}/{} {}:{}",
        date.child_text("Year")?,
        two_digits(date, "Month")?,
        two_digits(date, "Day")?,
        two_digits(date, "Hour").unwrap_or_else(|| "00".to_string()),
        two_digits(date, "Minute").unwrap_or_else(|| "00".to_string())
    ))
}

/// History dates under their `.nbib` tags: `EDAT` (added to PubMed), `MHDA`
/// (MeSH added) and `CRDT` (record created), and every date as `PHST`, e.g.
/// `2020/12/11 06:00 [pubmed]`.
fn history_fields(data: &Element, fields: &mut HashMap<String, Vec<String>>) {
    for date in data.child("History").all("PubMedPubDate") {
        let Some(stamp) = history_stamp(date) else {
            continue;
        };
        let status = date.attribute("PubStatus").unwrap_or_default();
        let tag = match status {
            "pubmed" => Some("EDAT"),
            "medline" => Some("MHDA"),
            "entrez" => Some("CRDT"),
            _ => None,
        };
        if let Some(tag) = tag {
            fields
                .entry(tag.to_string())
                .or_insert_with(|| vec![stamp.clone()]);
        }
        fields
            .entry("PHST".to_string())
            .or_default()
            .push(format!("{} [{}]", stamp, status));
    }
}

fn article_to_citation(root: &Element) -> Citation {
    let medline = root.child("MedlineCitation");
    let article = medline.child("Article");
    let journal = article.child("Journal");
    let issue = journal.child("JournalIssue");
    let journal_info = medline.child("MedlineJournalInfo");
    let ids: Vec<&Element> = root
        .child("PubmedData")
        .child("ArticleIdList")
        .all("ArticleId")
        .collect();

    let mut issn: Vec<String> = journal
        .all("ISSN")
        .filter_map(|element| {
            let value = element.text_opt()?;
            Some(match element.attribute("IssnType") {
                Some(kind) => format!("{} ({})", value, kind),
                None => value,
            })
        })
        .collect();
    if let Some(linking) = journal_info.child_text("ISSNLinking") {
        issn.push(format!("{} (Linking)", linking));
    }

    let mut extra_fields = HashMap::new();
    history_fields(root.child("PubmedData"), &mut extra_fields);
    // `DEP`: the electronic publication date, as `20201210`.
    if let Some(date) = article
        .all("ArticleDate")
        .find(|date| date.attribute("DateType") == Some("Electronic"))
        && let (Some(year), Some(month), Some(day)) = (
            date.child_text("Year"),
            two_digits(date, "Month"),
            two_digits(date, "Day"),
        )
    {
        extra_fields.insert("DEP".to_string(), vec![format!("{}{}{}", year, month, day)]);
    }

    Citation {
        citation_type: texts(article.child("PublicationTypeList").all("PublicationType")),
        title: article
            .child_text("ArticleTitle")
            .or_else(|| article.child_text("VernacularTitle"))
            .unwrap_or_default(),
        authors: authors(&[article]),
        journal: journal.child_text("Title"),
        journal_abbr: journal_info
            .child_text("MedlineTA")
            .or_else(|| journal.child_text("ISOAbbreviation")),
        date: date(issue.child("PubDate")),
        volume: issue.child_text("Volume"),
        issue: issue.child_text("Issue"),
        pages: article.child("Pagination").child_text("MedlinePgn"),
        issn,
        doi: elocation_doi(article).or_else(|| id_of(&ids, "doi")),
        accession_number: None,
        pmid: medline.child_text("PMID"),
        pmc_id: id_of(&ids, "pmc"),
        abstract_text: abstract_text(article),
        keywords: texts(
            medline
                .all("KeywordList")
                .flat_map(|list| list.all("Keyword")),
        ),
        urls: Vec::new(),
        language: joined(texts(article.all("Language"))),
        mesh_terms: mesh_terms(medline),
        publisher: None,
        extra_fields,
    }
}

/// An NCBI Bookshelf book, or a chapter when it has its own `ArticleTitle`.
/// As in `.nbib` files, a chapter's book title is kept as `BTI`.
fn book_to_citation(root: &Element) -> Citation {
    let document = root.child("BookDocument");
    let book = document.child("Book");
    let ids: Vec<&Element> = document
        .child("ArticleIdList")
        .all("ArticleId")
        .chain(
            root.child("PubmedBookData")
                .child("ArticleIdList")
                .all("ArticleId"),
        )
        .collect();
    let chapter_title = document
        .child_text("ArticleTitle")
        .or_else(|| document.child_text("VernacularTitle"));
    let book_title = book.child_text("BookTitle");

    let mut extra_fields = HashMap::new();
    if chapter_title.is_some()
        && let Some(title) = &book_title
    {
        extra_fields.insert("BTI".to_string(), vec![title.clone()]);
    }
    if let Some(collection) = book.child_text("CollectionTitle") {
        extra_fields.insert("CTI".to_string(), vec![collection]);
    }
    history_fields(root.child("PubmedBookData"), &mut extra_fields);

    Citation {
        citation_type: texts(document.all("PublicationType")),
        title: chapter_title.or(book_title).unwrap_or_default(),
        authors: authors(&[document, book]),
        journal: None,
        journal_abbr: None,
        date: date(book.child("PubDate")),
        volume: book.child_text("Volume"),
        issue: None,
        pages: document.child("Pagination").child_text("MedlinePgn"),
        issn: texts(book.all("Isbn")),
        doi: elocation_doi(book)
            .or_else(|| elocation_doi(document))
            .or_else(|| id_of(&ids, "doi")),
        accession_number: id_of(&ids, "bookaccession"),
        pmid: document.child_text("PMID"),
        pmc_id: id_of(&ids, "pmc"),
        abstract_text: abstract_text(document),
        keywords: texts(
            document
                .all("KeywordList")
                .flat_map(|list| list.all("Keyword")),
        ),
        urls: Vec::new(),
        language: joined(texts(document.all("Language"))),
        mesh_terms: Vec::new(),
        publisher: book.child("Publisher").child_text("PublisherName"),
        extra_fields,
    }
}
