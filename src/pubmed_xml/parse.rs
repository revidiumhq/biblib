//! PubMed XML parsing implementation.
//!
//! Each top-level `<PubmedArticle>` or `<PubmedBookArticle>` is read into a
//! small element tree, then mapped to a [`Citation`] the way the PubMed
//! (`.nbib`) parser maps the same record: the same standard fields, and the
//! other data in `extra_fields` under the `.nbib` tag and in its format.

use crate::error::{ParseError, SourceSpan, ValueError};
use crate::utils::{
    buffer_position_to_line_number, format_page_numbers, parse_month_name,
    parse_pubmed_date_lenient, resolve_xml_reference, split_given_and_middle, utf8_bom_len,
    xml_reader_position,
};
use crate::{Author, Citation, CitationFormat, Date};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

/// Elements nest a few levels deep in real exports; this bounds the recursion
/// on hostile input.
const MAX_DEPTH: usize = 128;

/// Root elements of PubMed XML: `efetch` and PubMed's own export use
/// `<PubmedArticleSet>`; the DTD also defines `<PubmedBookArticleSet>`.
const ROOT_ELEMENTS: [&[u8]; 2] = [b"PubmedArticleSet", b"PubmedBookArticleSet"];

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

/// What [`Element::child`] returns for a missing element, so lookups chain.
static MISSING_ELEMENT: Element = Element {
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

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |element| element.name == name)
    }

    /// The `item` children of every `list` child: some lists, such as
    /// `<InvestigatorList>`, can repeat.
    fn list_items<'a>(&'a self, list: &'a str, item: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children_named(list)
            .flat_map(move |list| list.children_named(item))
    }

    /// The first child with this name, or an empty element.
    fn child(&self, name: &str) -> &Element {
        self.elements()
            .find(|element| element.name == name)
            .unwrap_or(&MISSING_ELEMENT)
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// All descendant text in reading order, so inline markup such as
    /// `<i>E. coli</i>` keeps its words in place, with whitespace collapsed.
    ///
    /// Superscripts and subscripts are written in parentheses, as MEDLINE
    /// does: `10<sup>7</sup>` is `10(7)`, not `107`. A superscript that is
    /// only a mark such as `®` is kept as is.
    fn text(&self) -> String {
        fn collect(element: &Element, out: &mut String) {
            for node in &element.children {
                match node {
                    Node::Text(text) => out.push_str(text),
                    Node::Element(child) if child.name == "sup" || child.name == "sub" => {
                        let inner = child.text();
                        if inner.chars().all(|c| "®™©*†‡§".contains(c)) {
                            out.push_str(&inner);
                        } else {
                            out.push('(');
                            out.push_str(&inner);
                            out.push(')');
                        }
                    }
                    Node::Element(child) => collect(child, out),
                }
            }
        }
        let mut raw = String::new();
        collect(self, &mut raw);
        // Only XML whitespace: a no-break space in the text is kept.
        raw.split([' ', '\t', '\n', '\r'])
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn text_opt(&self) -> Option<String> {
        Some(self.text()).filter(|text| !text.is_empty())
    }

    fn child_text(&self, name: &str) -> Option<String> {
        self.child(name).text_opt()
    }

    fn push_text(&mut self, text: &str) {
        // `&amp;` and the like arrive as separate events; keep one text node.
        if let Some(Node::Text(last)) = self.children.last_mut() {
            last.push_str(text);
        } else {
            self.children.push(Node::Text(text.to_string()));
        }
    }
}

fn syntax_error(content: &str, start: usize, end: usize, message: String) -> ParseError {
    ParseError::at_line(
        buffer_position_to_line_number(content, start),
        CitationFormat::PubMedXml,
        ValueError::Syntax(message),
    )
    .with_span(SourceSpan::new(start, end.max(start)))
}

fn element_name(start: &BytesStart) -> String {
    String::from_utf8_lossy(start.name().as_ref()).into_owned()
}

fn read_attributes(
    start: &BytesStart,
    reader: &Reader<&[u8]>,
    content: &str,
    start_pos: usize,
) -> Result<Vec<(String, String)>, ParseError> {
    let end = xml_reader_position(reader, content);
    let error = |message: String| syntax_error(content, start_pos, end, message);
    start
        .attributes()
        .map(|attribute| {
            let attribute = attribute.map_err(|e| error(format!("Invalid attribute: {}", e)))?;
            let value = attribute
                .decoded_and_normalized_value(XmlVersion::Explicit1_1, reader.decoder())
                .map_err(|e| error(format!("Invalid attribute value: {}", e)))?;
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
            xml_reader_position(reader, content),
            format!("Elements nested more than {} levels deep", MAX_DEPTH),
        ));
    }
    let mut element = Element {
        name: element_name(start),
        attributes: read_attributes(start, reader, content, start_pos)?,
        children: Vec::new(),
    };

    loop {
        let pos = xml_reader_position(reader, content);
        let event = reader.read_event();
        let end = xml_reader_position(reader, content);
        let text_error = |detail: String| {
            syntax_error(
                content,
                pos,
                end,
                format!("Invalid XML text content: {}", detail),
            )
        };
        match event {
            Ok(Event::Start(child)) => {
                let child = read_element(reader, &child, content, pos, depth + 1)?;
                element.children.push(Node::Element(child));
            }
            Ok(Event::Empty(child)) => {
                element.children.push(Node::Element(Element {
                    name: element_name(&child),
                    attributes: read_attributes(&child, reader, content, pos)?,
                    children: Vec::new(),
                }));
            }
            Ok(Event::Text(text)) => {
                let text = text
                    .xml_content(XmlVersion::Explicit1_1)
                    .map_err(|e| text_error(e.to_string()))?;
                element.push_text(&text);
            }
            Ok(Event::CData(text)) => {
                let text = text.decode().map_err(|e| text_error(e.to_string()))?;
                element.push_text(&text);
            }
            // Only the predefined entities are known: a DTD's entities are
            // never expanded, so a reference to one is an error.
            Ok(Event::GeneralRef(reference)) => {
                let ch = resolve_xml_reference(&reference).map_err(text_error)?;
                element.push_text(ch.encode_utf8(&mut [0; 4]));
            }
            Ok(Event::End(_)) => return Ok(element),
            Ok(Event::Eof) => {
                return Err(syntax_error(
                    content,
                    start_pos,
                    end,
                    format!(
                        "Unexpected EOF while looking for closing tag '{}'",
                        element.name
                    ),
                ));
            }
            Err(e) => {
                return Err(syntax_error(
                    content,
                    pos,
                    end,
                    format!("XML parsing error: {}", e),
                ));
            }
            _ => (),
        }
    }
}

/// Whether `content` is a PubMed XML export, judged by its root element.
pub(crate) fn looks_like_pubmed_xml(content: &str) -> bool {
    let mut reader = Reader::from_str(&content[utf8_bom_len(content)..]);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                return ROOT_ELEMENTS.contains(&e.name().as_ref());
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
    let mut in_root = false;
    loop {
        let pos = xml_reader_position(&reader, content);
        let event = reader.read_event();
        let end = xml_reader_position(&reader, content);
        match event {
            Ok(Event::Start(e) | Event::Empty(e)) if !in_root => {
                if !ROOT_ELEMENTS.contains(&e.name().as_ref()) {
                    return Err(syntax_error(
                        content,
                        pos,
                        end,
                        format!(
                            "Not a PubMed XML document: the root element is <{}>, not <PubmedArticleSet>",
                            element_name(&e)
                        ),
                    ));
                }
                in_root = true;
            }
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
                    end,
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

/// An `<AffiliationInfo>` as `.nbib` writes `AD`: the affiliation followed
/// by its identifiers, as `Univ X, Leeds, UK. ROR: https://ror.org/024mrxd33`.
fn affiliation(info: &Element) -> Option<String> {
    let mut text = info.child_text("Affiliation")?;
    let ids: Vec<String> = info
        .children_named("Identifier")
        .filter_map(|id| Some(format!("{}: {}", id.attribute("Source")?, id.text_opt()?)))
        .collect();
    if !ids.is_empty() {
        text.push(' ');
        text.push_str(&ids.join(". "));
    }
    Some(text)
}

fn affiliations(person: &Element) -> Vec<String> {
    person
        .children_named("AffiliationInfo")
        .filter_map(affiliation)
        .collect()
}

/// Every `<Author>` in these lists, in order: a person, or a group
/// (`<CollectiveName>`) as one name with no given name, as the `.nbib` parser
/// reads `CN`. A suffix such as "Jr" follows the given names, as in `FAU`.
fn authors<'a>(lists: impl Iterator<Item = &'a Element>) -> Vec<Author> {
    lists
        .flat_map(|list| list.children_named("Author"))
        .filter_map(|author| {
            let affiliations = affiliations(author);
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
                .or_else(|| author.child_text("Initials"))
                .map(|given| match author.child_text("Suffix") {
                    Some(suffix) => format!("{} {}", given, suffix),
                    None => given,
                });
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

/// A person as `.nbib` writes them in full (`FAU`, `FED`, `FIR`, `FPS`:
/// "Smith, Jane A") and short (`AU`, `ED`, `IR`, `PS`: "Smith JA"). A group
/// is its name in both.
fn nbib_names(person: &Element) -> Option<(String, String)> {
    if let Some(group) = person.child_text("CollectiveName") {
        return Some((group.clone(), group));
    }
    let last = person.child_text("LastName")?;
    let suffix = person
        .child_text("Suffix")
        .map(|suffix| format!(" {}", suffix))
        .unwrap_or_default();
    let full = match person.child_text("ForeName") {
        Some(fore) => format!("{}, {}{}", last, fore, suffix),
        None => format!("{}{}", last, suffix),
    };
    let short = match person.child_text("Initials") {
        Some(initials) => format!("{} {}{}", last, initials, suffix),
        None => format!("{}{}", last, suffix),
    };
    Some((full, short))
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
    parse_pubmed_date_lenient(&element.child_text("MedlineDate")?)
}

/// Structured abstracts keep their labels, as in `.nbib` `AB` lines; MEDLINE
/// leaves out the placeholder label "UNLABELLED".
fn abstract_text(abstract_element: &Element) -> Option<String> {
    let parts: Vec<String> = abstract_element
        .children_named("AbstractText")
        .filter_map(|part| {
            let text = part.text_opt()?;
            Some(match part.attribute("Label").map(str::trim) {
                Some(label) if !label.is_empty() && !label.eq_ignore_ascii_case("UNLABELLED") => {
                    format!("{}: {}", label, text)
                }
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
        .list_items("MeshHeadingList", "MeshHeading")
        .filter_map(|heading| {
            let descriptor = heading.child("DescriptorName");
            let mut term = format!("{}{}", major(descriptor), descriptor.text_opt()?);
            for qualifier in heading.children_named("QualifierName") {
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
        .children_named("ELocationID")
        .find(|id| id.attribute("EIdType") == Some("doi"))
        .and_then(Element::text_opt)
}

/// `<MedlinePgn>`, or `<StartPage>`-`<EndPage>` when it is missing, with an
/// abbreviated range ("940-5") expanded as in the other parsers.
fn pages(pagination: &Element) -> Option<String> {
    let pages = pagination.child_text("MedlinePgn").or_else(|| {
        let start = pagination.child_text("StartPage")?;
        Some(match pagination.child_text("EndPage") {
            Some(end) => format!("{}-{}", start, end),
            None => start,
        })
    })?;
    Some(format_page_numbers(&pages))
}

/// Multiple values are joined like the `.nbib` parser joins repeated tags.
fn joined(values: Vec<String>) -> Option<String> {
    (!values.is_empty()).then(|| values.join(" AND "))
}

/// Left-pad to two characters with `0`, as `.nbib` dates are written.
fn two_digits(date: &Element, name: &str) -> Option<String> {
    date.child_text(name).map(|value| format!("{:0>2}", value))
}

/// A date element as `.nbib` writes `DCOM`, `LR` or `DEP`: `20201210`, with
/// day `01` when there is none.
fn compact_date(date: &Element) -> Option<String> {
    Some(format!(
        "{}{}{}",
        date.child_text("Year")?,
        two_digits(date, "Month")?,
        two_digits(date, "Day").unwrap_or_else(|| "01".to_string())
    ))
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

/// `extra_fields` under `.nbib` tags, in the order values are added.
#[derive(Default)]
struct ExtraFields(HashMap<String, Vec<String>>);

impl ExtraFields {
    fn push(&mut self, tag: &str, value: Option<String>) {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            self.0.entry(tag.to_string()).or_default().push(value);
        }
    }

    fn extend(&mut self, tag: &str, values: impl IntoIterator<Item = String>) {
        for value in values {
            self.push(tag, Some(value));
        }
    }

    /// History dates: `EDAT` (added to PubMed) and `CRDT` (record created),
    /// and for articles also `MHDA` (MeSH added), `PMCR` (PMC release) and
    /// every date as `PHST`, e.g. `2020/12/11 06:00 [pubmed]`. `.nbib` files
    /// give books only `EDAT` and `CRDT`.
    fn history(&mut self, history: &Element, is_article: bool) {
        for date in history.children_named("PubMedPubDate") {
            let Some(stamp) = history_stamp(date) else {
                continue;
            };
            let status = date.attribute("PubStatus").unwrap_or_default();
            let tag = match status {
                "pubmed" => Some("EDAT"),
                "entrez" => Some("CRDT"),
                "medline" if is_article => Some("MHDA"),
                "pmc-release" if is_article => {
                    self.push("PMCR", stamp.get(..10).map(str::to_string));
                    None
                }
                _ => None,
            };
            if let Some(tag) = tag
                && !self.0.contains_key(tag)
            {
                self.push(tag, Some(stamp.clone()));
            }
            if is_article {
                self.push("PHST", Some(format!("{} [{}]", stamp, status)));
            }
        }
    }

    /// `AID`: identifiers other than the PMID, PMCID and manuscript ID, as
    /// `10.1056/NEJMoa2021436 [doi]`; `MID`: manuscript IDs.
    fn article_ids(&mut self, ids: &[&Element]) {
        for id in ids {
            let (Some(kind), Some(value)) = (id.attribute("IdType"), id.text_opt()) else {
                continue;
            };
            match kind {
                "pubmed" | "pmc" | "pmcid" => (),
                "mid" => self.push("MID", Some(value)),
                _ => self.push("AID", Some(format!("{} [{}]", value, kind))),
            }
        }
    }

    /// `AUID`: author identifiers, as `ORCID: 0000-0002-1825-0097`.
    fn author_ids<'a>(&mut self, lists: impl Iterator<Item = &'a Element>) {
        for author in lists.flat_map(|list| list.children_named("Author")) {
            for id in author.children_named("Identifier") {
                if let (Some(source), Some(value)) = (id.attribute("Source"), id.text_opt()) {
                    self.push("AUID", Some(format!("{}: {}", source, value)));
                }
            }
        }
    }

    /// `FED` and `ED`: editors listed beside authors.
    fn editors<'a>(&mut self, lists: impl Iterator<Item = &'a Element>) {
        for (full, short) in lists
            .flat_map(|list| list.children_named("Author"))
            .filter_map(nbib_names)
        {
            self.push("FED", Some(full));
            self.push("ED", Some(short));
        }
    }

    /// `OAB` and `OABL`: abstracts in other languages or for lay readers.
    fn other_abstracts(&mut self, parent: &Element) {
        for other in parent.children_named("OtherAbstract") {
            self.push("OAB", abstract_text(other));
            self.push("OABL", other.attribute("Language").map(str::to_string));
        }
    }

    /// `CIN`, `EIN`, `RIN` and the other links to related records, as
    /// `N Engl J Med. 2021;384(8):755-757. PMID: 32678528`.
    fn comments_corrections(&mut self, medline: &Element) {
        for link in medline.list_items("CommentsCorrectionsList", "CommentsCorrections") {
            let Some(tag) = link.attribute("RefType").and_then(comments_corrections_tag) else {
                continue;
            };
            let source = link.child_text("RefSource");
            let value = match (source, link.child_text("PMID")) {
                (Some(source), Some(pmid)) => format!("{} PMID: {}", source, pmid),
                (Some(source), None) => source,
                (None, Some(pmid)) => format!("PMID: {}", pmid),
                (None, None) => continue,
            };
            self.push(tag, Some(value));
        }
    }

    fn into_map(self) -> HashMap<String, Vec<String>> {
        self.0
    }
}

/// The `.nbib` tag for a `<CommentsCorrections RefType>`.
fn comments_corrections_tag(ref_type: &str) -> Option<&'static str> {
    Some(match ref_type {
        "CommentIn" => "CIN",
        "CommentOn" => "CON",
        "CorrectedandRepublishedIn" => "CRI",
        "CorrectedandRepublishedFrom" => "CRF",
        "DatasetDescribedIn" => "DDIN",
        "DatasetUseReportedIn" => "DRIN",
        "ErratumIn" => "EIN",
        "ErratumFor" => "EFR",
        "ExpressionOfConcernIn" => "ECI",
        "ExpressionOfConcernFor" => "ECF",
        "OriginalReportIn" => "ORI",
        "RepublishedIn" => "RPI",
        "RepublishedFrom" => "RPF",
        "RetractedandRepublishedIn" => "RRI",
        "RetractedandRepublishedFrom" => "RRF",
        "RetractionIn" => "RIN",
        "RetractionOf" => "ROF",
        "SummaryForPatientsIn" => "SPIN",
        "UpdateIn" => "UIN",
        "UpdateOf" => "UOF",
        _ => return None,
    })
}

/// Publication types in alphabetical order, as `.nbib` lists `PT`.
fn publication_types(parent: &Element) -> Vec<String> {
    let mut types = texts(parent.children_named("PublicationType"));
    types.sort();
    types
}

/// Keywords from every `<KeywordList>`, and `OTO`, their owners.
fn keywords(parent: &Element, extra: &mut ExtraFields) -> Vec<String> {
    let lists: Vec<&Element> = parent.children_named("KeywordList").collect();
    for list in &lists {
        extra.push("OTO", list.attribute("Owner").map(str::to_string));
    }
    texts(lists.iter().flat_map(|list| list.children_named("Keyword")))
}

fn article_to_citation(root: &Element) -> Citation {
    let medline = root.child("MedlineCitation");
    let article = medline.child("Article");
    let journal = article.child("Journal");
    let issue = journal.child("JournalIssue");
    let journal_info = medline.child("MedlineJournalInfo");
    let pubmed_data = root.child("PubmedData");
    let ids: Vec<&Element> = pubmed_data
        .child("ArticleIdList")
        .children_named("ArticleId")
        .collect();

    let mut issn: Vec<String> = journal
        .children_named("ISSN")
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

    let mut extra = ExtraFields::default();
    extra.push("OWN", medline.attribute("Owner").map(str::to_string));
    extra.push("STAT", medline.attribute("Status").map(str::to_string));
    extra.push("DCOM", compact_date(medline.child("DateCompleted")));
    extra.push("LR", compact_date(medline.child("DateRevised")));
    extra.push(
        "CI",
        article.child("Abstract").child_text("CopyrightInformation"),
    );
    extra.other_abstracts(medline);
    extra.author_ids(article.children_named("AuthorList"));
    extra.extend(
        "SI",
        article
            .list_items("DataBankList", "DataBank")
            .flat_map(|bank| {
                let name = bank.child_text("DataBankName").unwrap_or_default();
                texts(
                    bank.child("AccessionNumberList")
                        .children_named("AccessionNumber"),
                )
                .into_iter()
                .map(move |number| format!("{}/{}", name, number))
            }),
    );
    extra.extend(
        "GR",
        article.list_items("GrantList", "Grant").map(|grant| {
            // A part that is present but empty, such as `<Country/>`,
            // still gets its slash.
            ["GrantID", "Acronym", "Agency", "Country"]
                .iter()
                .filter(|name| grant.children_named(name).next().is_some())
                .map(|name| grant.child(name).text())
                .collect::<Vec<_>>()
                .join("/")
        }),
    );
    extra.push(
        "DEP",
        article
            .children_named("ArticleDate")
            .find(|date| date.attribute("DateType") == Some("Electronic"))
            .and_then(compact_date),
    );
    extra.push("PL", journal_info.child_text("Country"));
    extra.push("JID", journal_info.child_text("NlmUniqueID"));
    // Chemicals as `7S5I7G3JQL (Dexamethasone)`, then supplementary concepts.
    extra.extend(
        "RN",
        medline
            .list_items("ChemicalList", "Chemical")
            .filter_map(|chemical| {
                let substance = chemical.child_text("NameOfSubstance")?;
                Some(match chemical.child_text("RegistryNumber") {
                    Some(number) => format!("{} ({})", number, substance),
                    None => substance,
                })
            }),
    );
    extra.extend(
        "RN",
        texts(medline.list_items("SupplMeshList", "SupplMeshName")),
    );
    extra.extend("SB", texts(medline.children_named("CitationSubset")));
    extra.comments_corrections(medline);
    extra.extend("GN", texts(medline.children_named("GeneralNote")));
    for subject in medline.list_items("PersonalNameSubjectList", "PersonalNameSubject") {
        if let Some((full, short)) = nbib_names(subject) {
            extra.push("FPS", Some(full));
            extra.push("PS", Some(short));
        }
    }
    for investigator in medline.list_items("InvestigatorList", "Investigator") {
        if let Some((full, short)) = nbib_names(investigator) {
            extra.push("FIR", Some(full));
            extra.push("IR", Some(short));
        }
        extra.extend("IRAD", affiliations(investigator));
    }
    extra.extend(
        "OID",
        medline.children_named("OtherID").filter_map(|id| {
            let value = id.text_opt()?;
            Some(match id.attribute("Source") {
                Some(source) => format!("{}: {}", source, value),
                None => value,
            })
        }),
    );
    extra.push("RF", medline.child_text("NumberOfReferences"));
    extra.push("COI", medline.child_text("CoiStatement"));
    extra.push("TT", article.child_text("VernacularTitle"));
    extra.history(pubmed_data.child("History"), true);
    extra.push("PST", pubmed_data.child_text("PublicationStatus"));
    extra.article_ids(&ids);
    let keywords = keywords(medline, &mut extra);

    Citation {
        citation_type: publication_types(article.child("PublicationTypeList")),
        title: article
            .child_text("ArticleTitle")
            .or_else(|| article.child_text("VernacularTitle"))
            .unwrap_or_default(),
        authors: authors(article.children_named("AuthorList")),
        journal: journal.child_text("Title"),
        journal_abbr: journal_info
            .child_text("MedlineTA")
            .or_else(|| journal.child_text("ISOAbbreviation")),
        date: date(issue.child("PubDate")),
        volume: issue.child_text("Volume"),
        issue: issue.child_text("Issue"),
        pages: pages(article.child("Pagination")),
        issn,
        doi: elocation_doi(article).or_else(|| id_of(&ids, "doi")),
        accession_number: None,
        pmid: medline.child_text("PMID"),
        pmc_id: id_of(&ids, "pmc"),
        abstract_text: abstract_text(article.child("Abstract")),
        keywords,
        urls: Vec::new(),
        language: joined(texts(article.children_named("Language"))),
        mesh_terms: mesh_terms(medline),
        publisher: None,
        extra_fields: extra.into_map(),
    }
}

/// An NCBI Bookshelf book, or a chapter when it has its own `ArticleTitle`.
/// As in `.nbib` files, a chapter's book title is kept as `BTI`, and the
/// type gains "Book Chapter" or "Book".
fn book_to_citation(root: &Element) -> Citation {
    let document = root.child("BookDocument");
    let book = document.child("Book");
    let book_data = root.child("PubmedBookData");
    let ids: Vec<&Element> = document
        .child("ArticleIdList")
        .children_named("ArticleId")
        .chain(book_data.child("ArticleIdList").children_named("ArticleId"))
        .collect();
    // `.nbib` files end a chapter title with a period, like an article title.
    let chapter_title = document
        .child_text("ArticleTitle")
        .or_else(|| document.child_text("VernacularTitle"))
        .map(|mut title| {
            if !title.ends_with(['.', '?', '!']) {
                title.push('.');
            }
            title
        });
    let book_title = book.child_text("BookTitle");
    let is_chapter = chapter_title.is_some();

    // Chapter authors, then the book's; editors stand in when there are none.
    let author_lists = || {
        document
            .children_named("AuthorList")
            .chain(book.children_named("AuthorList"))
    };
    let (author_lists, editor_lists): (Vec<&Element>, Vec<&Element>) =
        author_lists().partition(|list| list.attribute("Type") != Some("editors"));
    let mut authors = authors(author_lists.iter().copied());

    let mut extra = ExtraFields::default();
    if authors.is_empty() {
        authors = self::authors(editor_lists.iter().copied());
    } else {
        extra.editors(editor_lists.iter().copied());
    }
    extra.author_ids(author_lists.iter().copied());
    if is_chapter {
        extra.push("BTI", book_title.clone());
    }
    extra.push("CTI", book.child_text("CollectionTitle"));
    extra.push("VTI", book.child_text("VolumeTitle"));
    extra.push("EN", book.child_text("Edition"));
    extra.push(
        "PL",
        book.child("Publisher").child_text("PublisherLocation"),
    );
    extra.push(
        "CP",
        document
            .children_named("LocationLabel")
            .find_map(Element::text_opt),
    );
    extra.push(
        "CI",
        document
            .child("Abstract")
            .child_text("CopyrightInformation"),
    );
    extra.push("CTDT", compact_date(document.child("ContributionDate")));
    extra.push("DRDT", compact_date(document.child("DateRevised")));
    extra.history(book_data.child("History"), false);
    extra.article_ids(&ids);
    let keywords = keywords(document, &mut extra);

    let mut citation_type = publication_types(document);
    citation_type.push(if is_chapter { "Book Chapter" } else { "Book" }.to_string());

    Citation {
        citation_type,
        title: chapter_title.or(book_title).unwrap_or_default(),
        authors,
        journal: None,
        journal_abbr: None,
        date: date(book.child("PubDate")),
        volume: book.child_text("Volume"),
        issue: None,
        pages: pages(document.child("Pagination")),
        issn: texts(book.children_named("Isbn")),
        doi: elocation_doi(book)
            .or_else(|| elocation_doi(document))
            .or_else(|| id_of(&ids, "doi")),
        accession_number: id_of(&ids, "bookaccession"),
        pmid: document.child_text("PMID"),
        pmc_id: id_of(&ids, "pmc"),
        abstract_text: abstract_text(document.child("Abstract")),
        keywords,
        urls: Vec::new(),
        language: joined(texts(document.children_named("Language"))),
        mesh_terms: Vec::new(),
        publisher: book.child("Publisher").child_text("PublisherName"),
        extra_fields: extra.into_map(),
    }
}
