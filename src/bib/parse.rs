use crate::error::{ParseError, SourceSpan, ValueError, fields as error_fields};
use crate::{Author, Citation, CitationFormat};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub(crate) struct MacroDef {
    expr: FieldExpr,
}

#[derive(Debug, Clone)]
pub(crate) struct RawBibEntry {
    entry_type: String,
    key: String,
    fields: Vec<RawField>,
    start_line: usize,
    span: SourceSpan,
}

#[derive(Debug, Clone)]
struct RawField {
    name: String,
    expr: FieldExpr,
    raw_value: String,
}

#[derive(Debug, Clone)]
enum FieldExpr {
    Literal(String),
    Ident(String),
    Concat(Vec<FieldExpr>),
}

#[derive(Debug, Clone)]
struct ParsedDocument {
    macros: HashMap<String, MacroDef>,
    entries: Vec<RawBibEntry>,
}

#[derive(Debug, Clone)]
struct ResolvedText {
    value: String,
    fully_resolved: bool,
}

#[derive(Debug, Clone)]
struct ResolvedField {
    value: String,
    fully_resolved: bool,
    raw: String,
}

#[derive(Debug, Clone)]
struct ResolvedEntry {
    entry_type: String,
    fields: HashMap<String, Vec<ResolvedField>>,
    start_line: usize,
    span: SourceSpan,
}

pub(crate) fn looks_like_bib(content: &str) -> bool {
    let trimmed = content.trim_start();
    if !trimmed.starts_with('@') {
        return false;
    }

    let after_at = &trimmed[1..];
    let ident_len = after_at
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        .map(char::len_utf8)
        .sum::<usize>();

    if ident_len == 0 {
        return false;
    }

    let remainder = after_at[ident_len..].trim_start();
    matches!(remainder.chars().next(), Some('{') | Some('('))
}

pub(crate) fn parse_bib(content: &str) -> Result<Vec<Citation>, ParseError> {
    let mut parser = Parser::new(content);
    let document = parser.parse_document()?;

    if document.entries.is_empty() {
        return Ok(Vec::new());
    }

    Resolver::new(content, document).into_citations()
}

struct Parser<'a> {
    source: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, pos: 0 }
    }

    fn parse_document(&mut self) -> Result<ParsedDocument, ParseError> {
        let mut macros = builtin_macros();
        let mut entries = Vec::new();

        self.skip_ws_and_comments();
        while !self.eof() {
            let at_pos = self.pos;
            self.expect_char('@')?;
            let directive = self.parse_name()?.to_ascii_lowercase();
            self.skip_ws_and_comments();

            let open = self
                .peek_char()
                .ok_or_else(|| self.syntax_error(at_pos, at_pos, "Unexpected end of input"))?;
            let close = match open {
                '{' => '}',
                '(' => ')',
                _ => {
                    return Err(self.syntax_error(
                        self.pos,
                        self.pos,
                        "Expected '{' or '(' after directive name",
                    ));
                }
            };
            self.bump_char();

            match directive.as_str() {
                "comment" | "preamble" => {
                    self.skip_balanced_block(open, close)?;
                }
                "string" => {
                    let (name, def) = self.parse_string_definition(close)?;
                    macros.insert(name, def);
                }
                _ => {
                    let entry = self.parse_entry(directive, close, at_pos)?;
                    entries.push(entry);
                }
            }

            self.skip_ws_and_comments();
        }

        Ok(ParsedDocument { macros, entries })
    }

    fn parse_string_definition(&mut self, close: char) -> Result<(String, MacroDef), ParseError> {
        self.skip_ws_and_comments();
        let name = self.parse_name()?.to_ascii_lowercase();
        self.skip_ws_and_comments();
        self.expect_char('=')?;
        self.skip_ws_and_comments();
        let value_start = self.pos;
        let expr = self.parse_value_expr()?;
        let value_end = self.pos;
        let raw_value = self.source[value_start..value_end].trim().to_string();

        self.skip_ws_and_comments();
        if self.peek_char() == Some(',') {
            self.bump_char();
            self.skip_ws_and_comments();
        }
        self.expect_char(close)?;

        Ok((
            name,
            MacroDef {
                expr: if raw_value.is_empty() {
                    FieldExpr::Literal(String::new())
                } else {
                    expr
                },
            },
        ))
    }

    fn parse_entry(
        &mut self,
        entry_type: String,
        close: char,
        start_pos: usize,
    ) -> Result<RawBibEntry, ParseError> {
        self.skip_ws_and_comments();
        let key_start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch == ',' || ch == close {
                break;
            }
            self.bump_char();
        }

        let key = self.source[key_start..self.pos].trim().to_string();
        if key.is_empty() {
            return Err(self.syntax_error(key_start, self.pos, "Bib entry is missing a citation key"));
        }

        let mut fields = Vec::new();
        self.skip_ws_and_comments();

        match self.peek_char() {
            Some(ch) if ch == close => {
                self.bump_char();
            }
            Some(',') => {
                self.bump_char();
                loop {
                    self.skip_ws_and_comments();
                    if self.peek_char() == Some(close) {
                        self.bump_char();
                        break;
                    }
                    let name = self.parse_name()?.to_ascii_lowercase();
                    self.skip_ws_and_comments();
                    self.expect_char('=')?;
                    self.skip_ws_and_comments();
                    let value_start = self.pos;
                    let expr = self.parse_value_expr()?;
                    let value_end = self.pos;
                    let raw_value = self.source[value_start..value_end].trim().to_string();
                    fields.push(RawField {
                        name,
                        expr,
                        raw_value,
                    });

                    self.skip_ws_and_comments();
                    match self.peek_char() {
                        Some(',') => {
                            self.bump_char();
                        }
                        Some(ch) if ch == close => {
                            self.bump_char();
                            break;
                        }
                        Some(_) => {
                            return Err(self.syntax_error(
                                self.pos,
                                self.pos,
                                "Expected ',' or closing delimiter after field value",
                            ));
                        }
                        None => {
                            return Err(self.syntax_error(
                                self.pos,
                                self.pos,
                                "Unexpected end of input while parsing entry",
                            ));
                        }
                    }
                }
            }
            Some(_) => {
                return Err(self.syntax_error(
                    self.pos,
                    self.pos,
                    "Expected ',' or closing delimiter after citation key",
                ));
            }
            None => {
                return Err(self.syntax_error(
                    self.pos,
                    self.pos,
                    "Unexpected end of input while parsing entry",
                ));
            }
        }

        let start_line = line_and_column_at(self.source, start_pos).0;
        Ok(RawBibEntry {
            entry_type,
            key,
            fields,
            start_line,
            span: SourceSpan::new(start_pos, self.pos),
        })
    }

    fn parse_value_expr(&mut self) -> Result<FieldExpr, ParseError> {
        let mut parts = vec![self.parse_value_atom()?];

        loop {
            self.skip_ws_and_comments();
            if self.peek_char() != Some('#') {
                break;
            }
            self.bump_char();
            self.skip_ws_and_comments();
            parts.push(self.parse_value_atom()?);
        }

        if parts.len() == 1 {
            Ok(parts.remove(0))
        } else {
            Ok(FieldExpr::Concat(parts))
        }
    }

    fn parse_value_atom(&mut self) -> Result<FieldExpr, ParseError> {
        match self.peek_char() {
            Some('{') => Ok(FieldExpr::Literal(self.parse_braced_string()?)),
            Some('"') => Ok(FieldExpr::Literal(self.parse_quoted_string()?)),
            Some(ch) if is_bare_token_start(ch) => {
                let token = self.parse_bare_token();
                if token.chars().all(|c| c.is_ascii_digit()) {
                    Ok(FieldExpr::Literal(token))
                } else {
                    Ok(FieldExpr::Ident(token))
                }
            }
            Some(_) => Err(self.syntax_error(
                self.pos,
                self.pos,
                "Expected a BibTeX/BibLaTeX value",
            )),
            None => Err(self.syntax_error(
                self.pos,
                self.pos,
                "Unexpected end of input while parsing value",
            )),
        }
    }

    fn parse_braced_string(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        self.expect_char('{')?;
        let mut depth = 1usize;
        let mut result = String::new();

        while let Some(ch) = self.peek_char() {
            match ch {
                '\\' => {
                    result.push(ch);
                    self.bump_char();
                    if let Some(next) = self.peek_char() {
                        result.push(next);
                        self.bump_char();
                    }
                }
                '{' => {
                    depth += 1;
                    result.push(ch);
                    self.bump_char();
                }
                '}' => {
                    depth -= 1;
                    self.bump_char();
                    if depth == 0 {
                        return Ok(result);
                    }
                    result.push('}');
                }
                _ => {
                    result.push(ch);
                    self.bump_char();
                }
            }
        }

        Err(self.syntax_error(
            start,
            self.pos,
            "Unterminated braced value in .bib input",
        ))
    }

    fn parse_quoted_string(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        self.expect_char('"')?;
        let mut result = String::new();

        while let Some(ch) = self.peek_char() {
            match ch {
                '\\' => {
                    result.push(ch);
                    self.bump_char();
                    if let Some(next) = self.peek_char() {
                        result.push(next);
                        self.bump_char();
                    }
                }
                '"' => {
                    self.bump_char();
                    return Ok(result);
                }
                _ => {
                    result.push(ch);
                    self.bump_char();
                }
            }
        }

        Err(self.syntax_error(
            start,
            self.pos,
            "Unterminated quoted value in .bib input",
        ))
    }

    fn skip_balanced_block(&mut self, open: char, close: char) -> Result<(), ParseError> {
        let start = self.pos.saturating_sub(open.len_utf8());
        let mut depth = 1usize;

        while let Some(ch) = self.peek_char() {
            match ch {
                '\\' => {
                    self.bump_char();
                    if !self.eof() {
                        self.bump_char();
                    }
                }
                '"' => {
                    let _ = self.parse_quoted_string()?;
                }
                '{' if open != '{' => {
                    let _ = self.parse_braced_string()?;
                }
                _ if ch == open => {
                    depth += 1;
                    self.bump_char();
                }
                _ if ch == close => {
                    depth -= 1;
                    self.bump_char();
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {
                    self.bump_char();
                }
            }
        }

        Err(self.syntax_error(
            start,
            self.pos,
            "Unterminated top-level BibTeX/BibLaTeX block",
        ))
    }

    fn parse_name(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        let mut bytes = 0usize;
        for ch in self.source[self.pos..].chars() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                bytes += ch.len_utf8();
            } else {
                break;
            }
        }

        if bytes == 0 {
            return Err(self.syntax_error(start, start, "Expected an identifier"));
        }

        self.pos += bytes;
        Ok(self.source[start..self.pos].to_string())
    }

    fn parse_bare_token(&mut self) -> String {
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() || matches!(ch, '#' | ',' | '}' | ')' | '=' | '"') {
                break;
            }
            self.bump_char();
        }
        self.source[start..self.pos].trim().to_string()
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            let before = self.pos;
            while let Some(ch) = self.peek_char() {
                if ch.is_whitespace() {
                    self.bump_char();
                } else {
                    break;
                }
            }

            if self.peek_char() == Some('%') {
                while let Some(ch) = self.peek_char() {
                    self.bump_char();
                    if ch == '\n' {
                        break;
                    }
                }
            }

            if self.pos == before {
                break;
            }
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), ParseError> {
        match self.peek_char() {
            Some(ch) if ch == expected => {
                self.bump_char();
                Ok(())
            }
            Some(_) => Err(self.syntax_error(
                self.pos,
                self.pos,
                &format!("Expected '{}'", expected),
            )),
            None => Err(self.syntax_error(
                self.pos,
                self.pos,
                &format!("Expected '{}' but reached end of input", expected),
            )),
        }
    }

    fn syntax_error(&self, start: usize, end: usize, message: &str) -> ParseError {
        let (line, column) = line_and_column_at(self.source, start);
        let span_end = if end > start {
            end
        } else {
            next_boundary(self.source, start)
        };

        ParseError::at_position(
            line,
            column,
            CitationFormat::Bib,
            ValueError::Syntax(message.to_string()),
        )
        .with_span(SourceSpan::new(start, span_end))
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn bump_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn eof(&self) -> bool {
        self.pos >= self.source.len()
    }
}

struct Resolver {
    macros: HashMap<String, MacroDef>,
    entries: Vec<RawBibEntry>,
    entry_lookup: HashMap<String, usize>,
    macro_cache: HashMap<String, ResolvedText>,
    entry_cache: HashMap<usize, ResolvedEntry>,
}

impl Resolver {
    fn new(_source: &str, document: ParsedDocument) -> Self {
        let mut entry_lookup = HashMap::new();
        for (index, entry) in document.entries.iter().enumerate() {
            entry_lookup
                .entry(entry.key.to_ascii_lowercase())
                .or_insert(index);
        }

        Self {
            macros: document.macros,
            entries: document.entries,
            entry_lookup,
            macro_cache: HashMap::new(),
            entry_cache: HashMap::new(),
        }
    }

    fn into_citations(mut self) -> Result<Vec<Citation>, ParseError> {
        let mut citations = Vec::with_capacity(self.entries.len());
        for index in 0..self.entries.len() {
            if self.entries[index].entry_type.eq_ignore_ascii_case("xdata") {
                continue;
            }
            let resolved = self.resolve_entry(index, &mut Vec::new());
            citations.push(self.into_citation(resolved)?);
        }
        Ok(citations)
    }

    fn resolve_entry(&mut self, index: usize, stack: &mut Vec<usize>) -> ResolvedEntry {
        if let Some(cached) = self.entry_cache.get(&index) {
            return cached.clone();
        }

        let raw_entry = self.entries[index].clone();
        stack.push(index);

        let mut fields = self.resolve_local_fields(&raw_entry.fields);

        for key in collect_reference_keys(fields.get("xdata")) {
            if let Some(parent_index) = self.entry_lookup.get(&key.to_ascii_lowercase()).copied() {
                if !stack.contains(&parent_index) {
                    let parent = self.resolve_entry(parent_index, stack);
                    inherit_fields(&mut fields, &parent.fields);
                }
            }
        }

        if let Some(crossref) = fields
            .get("crossref")
            .and_then(|values| values.first())
            .map(|field| field.value.trim().to_string())
            .filter(|value| !value.is_empty())
            && let Some(parent_index) = self.entry_lookup.get(&crossref.to_ascii_lowercase()).copied()
            && !stack.contains(&parent_index)
        {
            let parent = self.resolve_entry(parent_index, stack);
            inherit_fields(&mut fields, &parent.fields);
        }

        stack.pop();

        let resolved = ResolvedEntry {
            entry_type: raw_entry.entry_type,
            fields,
            start_line: raw_entry.start_line,
            span: raw_entry.span,
        };
        self.entry_cache.insert(index, resolved.clone());
        resolved
    }

    fn resolve_local_fields(&mut self, fields: &[RawField]) -> HashMap<String, Vec<ResolvedField>> {
        let mut resolved = HashMap::new();
        for field in fields {
            let text = self.resolve_expr(&field.expr, &mut Vec::new());
            resolved
                .entry(field.name.clone())
                .or_insert_with(Vec::new)
                .push(ResolvedField {
                    value: text.value,
                    fully_resolved: text.fully_resolved,
                    raw: field.raw_value.clone(),
                });
        }
        resolved
    }

    fn resolve_expr(&mut self, expr: &FieldExpr, macro_stack: &mut Vec<String>) -> ResolvedText {
        match expr {
            FieldExpr::Literal(value) => ResolvedText {
                value: value.clone(),
                fully_resolved: true,
            },
            FieldExpr::Ident(name) => self.resolve_ident(name, macro_stack),
            FieldExpr::Concat(parts) => {
                let mut value = String::new();
                let mut fully_resolved = true;
                for part in parts {
                    let resolved = self.resolve_expr(part, macro_stack);
                    value.push_str(&resolved.value);
                    fully_resolved &= resolved.fully_resolved;
                }
                ResolvedText {
                    value,
                    fully_resolved,
                }
            }
        }
    }

    fn resolve_ident(&mut self, name: &str, macro_stack: &mut Vec<String>) -> ResolvedText {
        let key = name.to_ascii_lowercase();
        if let Some(cached) = self.macro_cache.get(&key) {
            return cached.clone();
        }

        if macro_stack.contains(&key) {
            return ResolvedText {
                value: name.to_string(),
                fully_resolved: false,
            };
        }

        if let Some(definition) = self.macros.get(&key).cloned() {
            macro_stack.push(key.clone());
            let resolved = self.resolve_expr(&definition.expr, macro_stack);
            macro_stack.pop();
            self.macro_cache.insert(key, resolved.clone());
            resolved
        } else {
            ResolvedText {
                value: name.to_string(),
                fully_resolved: false,
            }
        }
    }

    fn into_citation(&self, resolved: ResolvedEntry) -> Result<Citation, ParseError> {
        let ResolvedEntry {
            entry_type,
            mut fields,
            start_line,
            span,
        } = resolved;

        let title = take_title(&mut fields);
        let authors = take_authors(&mut fields);
        let journal = take_preferred_value(&mut fields, &["journaltitle", "journal", "booktitle"]);
        let journal_abbr = take_preferred_value(&mut fields, &["shortjournal", "journalabbr"]);
        let date = take_date(&mut fields);
        let volume = take_first_value(&mut fields, "volume");
        let issue = take_preferred_value(&mut fields, &["number", "issue"]);
        let pages = take_first_value(&mut fields, "pages").map(|pages| crate::utils::format_page_numbers(&pages));
        let publisher = take_first_value(&mut fields, "publisher");
        let language = take_preferred_value(&mut fields, &["language", "langid"]);
        let abstract_text = take_joined_value(&mut fields, "abstract");
        let keywords = take_keywords(&mut fields);
        let pmid = take_preferred_value(&mut fields, &["pmid", "pubmed"]);
        let pmc_id = take_preferred_value(&mut fields, &["pmcid", "pmc"]);
        let mut accession_number =
            take_preferred_value(&mut fields, &["accessionnumber", "eid", "ids"]);
        if accession_number.is_none() {
            accession_number = pmid.clone().or_else(|| pmc_id.clone());
        }

        let mut doi = None;
        if let Some(candidate) = take_first_value(&mut fields, "doi")
            && let Some(formatted) = crate::utils::format_doi(&candidate)
        {
            doi = Some(formatted);
        }

        let urls = take_all_values(&mut fields, "url");
        if doi.is_none() {
            for url in &urls {
                if let Some(found) = crate::utils::format_doi(url) {
                    doi = Some(found);
                    break;
                }
            }
        }

        let mut issn = take_identifier_values(&mut fields, "issn");
        issn.extend(take_identifier_values(&mut fields, "isbn"));

        let has_identity = !title.trim().is_empty()
            || !authors.is_empty()
            || doi.is_some()
            || !urls.is_empty()
            || accession_number.is_some()
            || pmid.is_some()
            || pmc_id.is_some()
            || has_non_empty_field(&fields, "eprint");

        if !has_identity {
            let err = ParseError::new(
                Some(start_line),
                None,
                CitationFormat::Bib,
                ValueError::MissingValue {
                    field: error_fields::TITLE,
                    key: "title/author/identifier",
                },
            );
            return Err(err.with_span(span));
        }

        Ok(Citation {
            citation_type: vec![entry_type.to_ascii_lowercase()],
            title,
            authors,
            journal,
            journal_abbr,
            date,
            volume,
            issue,
            pages,
            issn,
            doi,
            accession_number,
            pmid,
            pmc_id,
            abstract_text,
            keywords,
            urls: dedupe_preserve_order(urls),
            language,
            mesh_terms: Vec::new(),
            publisher,
            extra_fields: remaining_extra_fields(fields),
        })
    }
}

fn builtin_macros() -> HashMap<String, MacroDef> {
    let mut macros = HashMap::new();
    for month in [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ] {
        macros.insert(
            month.to_string(),
            MacroDef {
                expr: FieldExpr::Literal(month.to_string()),
            },
        );
    }
    macros
}

fn inherit_fields(
    child: &mut HashMap<String, Vec<ResolvedField>>,
    parent: &HashMap<String, Vec<ResolvedField>>,
) {
    for (key, values) in parent {
        if matches!(key.as_str(), "xdata" | "crossref") || child.contains_key(key) {
            continue;
        }
        child.insert(key.clone(), values.clone());
    }
}

fn collect_reference_keys(values: Option<&Vec<ResolvedField>>) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(values) = values {
        for field in values {
            for part in field.value.split(',') {
                let trimmed = part.trim();
                if !trimmed.is_empty() {
                    keys.push(trimmed.to_string());
                }
            }
        }
    }
    keys
}

fn take_title(fields: &mut HashMap<String, Vec<ResolvedField>>) -> String {
    let mut title = take_first_value(fields, "title").unwrap_or_default();
    if let Some(subtitle) = take_first_value(fields, "subtitle") {
        if title.trim().is_empty() {
            title = subtitle;
        } else {
            title.push_str(": ");
            title.push_str(&subtitle);
        }
    }
    title
}

fn take_authors(fields: &mut HashMap<String, Vec<ResolvedField>>) -> Vec<Author> {
    if let Some(author_text) = take_first_value(fields, "author") {
        return parse_people_list(&author_text);
    }

    fields
        .get("editor")
        .map(|values| join_field_text(values).as_str().to_string())
        .map(|text| parse_people_list(&text))
        .unwrap_or_default()
}

fn take_date(fields: &mut HashMap<String, Vec<ResolvedField>>) -> Option<crate::Date> {
    if let Some(values) = fields.get("date")
        && let Some(value) = values
            .iter()
            .map(ResolvedField::canonical_text)
            .find(|value| !value.trim().is_empty())
        && let Some(date) = crate::utils::parse_bib_date(&value)
    {
        fields.remove("date");
        return Some(date);
    }

    if let Some(year) = fields
        .get("year")
        .and_then(|values| values.iter().map(ResolvedField::canonical_text).find(|value| !value.trim().is_empty()))
    {
        let month_value = fields.get("month").and_then(|values| {
            values
                .iter()
                .map(ResolvedField::canonical_text)
                .find(|value| !value.trim().is_empty())
        });

        if let Some(month) = month_value.as_deref()
            && let Some(date) = crate::utils::parse_bib_year_month(&year, month)
        {
            fields.remove("year");
            fields.remove("month");
            return Some(date);
        }

        if let Some(date) = crate::utils::parse_year_only(&year) {
            fields.remove("year");
            return Some(date);
        }
    }

    None
}

fn take_keywords(fields: &mut HashMap<String, Vec<ResolvedField>>) -> Vec<String> {
    let Some(values) = fields.remove("keywords") else {
        return Vec::new();
    };

    let mut keywords = Vec::new();
    for value in values {
        let text = value.canonical_text();
        let separators = if text.contains(';') {
            [';', '\n']
        } else if text.contains(',') {
            [',', '\n']
        } else {
            ['\n', '\n']
        };

        for chunk in text.split(&separators[..]) {
            let trimmed = chunk.trim();
            if !trimmed.is_empty() {
                keywords.push(trimmed.to_string());
            }
        }
    }
    dedupe_preserve_order(keywords)
}

fn take_identifier_values(
    fields: &mut HashMap<String, Vec<ResolvedField>>,
    key: &str,
) -> Vec<String> {
    let Some(values) = fields.remove(key) else {
        return Vec::new();
    };

    let mut identifiers = Vec::new();
    for value in values {
        let text = value.canonical_text();
        if text.trim().is_empty() {
            continue;
        }
        if key == "issn" {
            let split = crate::utils::split_issns(&text);
            if split.is_empty() {
                identifiers.push(text);
            } else {
                identifiers.extend(split);
            }
        } else {
            identifiers.push(text);
        }
    }
    dedupe_preserve_order(identifiers)
}

fn take_joined_value(
    fields: &mut HashMap<String, Vec<ResolvedField>>,
    key: &str,
) -> Option<String> {
    let values = fields.remove(key)?;
    let joined = values
        .into_iter()
        .map(|value| value.canonical_text())
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!joined.is_empty()).then_some(joined)
}

fn take_first_value(
    fields: &mut HashMap<String, Vec<ResolvedField>>,
    key: &str,
) -> Option<String> {
    let values = fields.remove(key)?;
    values
        .into_iter()
        .map(|value| value.canonical_text())
        .find(|value| !value.trim().is_empty())
}

fn take_preferred_value(
    fields: &mut HashMap<String, Vec<ResolvedField>>,
    keys: &[&str],
) -> Option<String> {
    for key in keys {
        if let Some(value) = fields
            .get(*key)
            .and_then(|values| values.iter().map(ResolvedField::canonical_text).find(|value| !value.trim().is_empty()))
        {
            fields.remove(*key);
            return Some(value);
        }
    }
    None
}

fn take_all_values(fields: &mut HashMap<String, Vec<ResolvedField>>, key: &str) -> Vec<String> {
    let Some(values) = fields.remove(key) else {
        return Vec::new();
    };
    values
        .into_iter()
        .map(|value| value.canonical_text())
        .filter(|value| !value.trim().is_empty())
        .collect()
}

fn has_non_empty_field(fields: &HashMap<String, Vec<ResolvedField>>, key: &str) -> bool {
    fields.get(key).is_some_and(|values| {
        values
            .iter()
            .map(ResolvedField::canonical_text)
            .any(|value| !value.trim().is_empty())
    })
}

fn remaining_extra_fields(
    fields: HashMap<String, Vec<ResolvedField>>,
) -> HashMap<String, Vec<String>> {
    let mut extra = HashMap::new();
    for (key, values) in fields {
        let collected = values
            .into_iter()
            .map(|value| value.extra_text())
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>();
        if !collected.is_empty() {
            extra.insert(key, collected);
        }
    }
    extra
}

impl ResolvedField {
    fn canonical_text(&self) -> String {
        self.value.trim().to_string()
    }

    fn extra_text(&self) -> String {
        if self.fully_resolved {
            self.value.trim().to_string()
        } else {
            self.raw.trim().to_string()
        }
    }
}

fn parse_people_list(value: &str) -> Vec<Author> {
    split_top_level_and(value)
        .into_iter()
        .filter_map(|person| parse_person(&person))
        .collect()
}

fn split_top_level_and(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut pos = 0usize;

    while pos < value.len() {
        if depth == 0 && value[pos..].starts_with(" and ") {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
            current.clear();
            pos += 5;
            continue;
        }

        let ch = value[pos..].chars().next().unwrap();
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        current.push(ch);
        pos += ch.len_utf8();
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
    parts
}

fn parse_person(person: &str) -> Option<Author> {
    let trimmed = person.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(name) = strip_wrapping_braces(trimmed) {
        return Some(Author {
            name,
            given_name: None,
            middle_name: None,
            affiliations: Vec::new(),
        });
    }

    let comma_parts = trimmed
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    let (family, given) = match comma_parts.len() {
        0 => return None,
        1 => parse_unstructured_name(comma_parts[0]),
        2 => (comma_parts[0].to_string(), comma_parts[1].to_string()),
        _ => (
            comma_parts[0].to_string(),
            format!("{} {}", comma_parts[2], comma_parts[1]).trim().to_string(),
        ),
    };

    let family = family.trim().to_string();
    let given = given.trim().to_string();
    let (given_name, middle_name) = if given.is_empty() {
        (None, None)
    } else {
        crate::utils::split_given_and_middle(&given)
    };

    Some(Author {
        name: family,
        given_name,
        middle_name,
        affiliations: Vec::new(),
    })
}

fn parse_unstructured_name(name: &str) -> (String, String) {
    let tokens = name.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return (String::new(), String::new());
    }
    if tokens.len() == 1 {
        return (tokens[0].to_string(), String::new());
    }

    let particles = [
        "von", "van", "de", "del", "der", "den", "da", "dos", "la", "le", "du",
    ];

    let mut family_start = tokens.len() - 1;
    while family_start > 0 {
        let previous = tokens[family_start - 1];
        let starts_lowercase = previous
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase());
        if starts_lowercase || particles.contains(&previous.to_ascii_lowercase().as_str()) {
            family_start -= 1;
        } else {
            break;
        }
    }

    let family = tokens[family_start..].join(" ");
    let given = tokens[..family_start].join(" ");
    (family, given)
}

fn strip_wrapping_braces(value: &str) -> Option<String> {
    if !value.starts_with('{') || !value.ends_with('}') {
        return None;
    }

    let mut depth = 0usize;
    for (index, ch) in value.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index + ch.len_utf8() != value.len() {
                    return None;
                }
            }
            _ => {}
        }
    }

    Some(value[1..value.len() - 1].trim().to_string())
}

fn join_field_text(values: &[ResolvedField]) -> String {
    values
        .iter()
        .map(ResolvedField::canonical_text)
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" and ")
}

fn line_and_column_at(source: &str, pos: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for ch in source[..pos.min(source.len())].chars() {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn next_boundary(source: &str, pos: usize) -> usize {
    if pos >= source.len() {
        return source.len();
    }
    let ch = source[pos..].chars().next().unwrap();
    pos + ch.len_utf8()
}

fn is_bare_token_start(ch: char) -> bool {
    !ch.is_whitespace() && !matches!(ch, '#' | ',' | '}' | ')' | '=' | '"' | '{')
}

fn dedupe_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}
