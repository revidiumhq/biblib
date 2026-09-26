//! Text and metadata normalization applied to citations before matching.

use crate::regex::Regex;
use crate::utils::format_page_numbers;
use std::sync::LazyLock;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

static UNICODE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<U\+([0-9A-Fa-f]+)>").unwrap());

static HTML_ENTITY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&(#x[0-9a-fA-F]+|#\d+|[A-Za-z]+);").unwrap());

const HTML_TAG_REPLACEMENTS: [(&str, &str); 8] = [
    ("<sup>", ""),
    ("</sup>", ""),
    ("<sub>", ""),
    ("</sub>", ""),
    ("<inf>", ""),
    ("</inf>", ""),
    ("<i>", ""),
    ("</i>", ""),
];

const TITLE_MARKUP_TOKEN_REPLACEMENTS: [(&str, &str); 8] = [
    ("[sub]", ""),
    ("[/sub]", ""),
    ("[sup]", ""),
    ("[/sup]", ""),
    ("(sub)", ""),
    ("(/sub)", ""),
    ("(sup)", ""),
    ("(/sup)", ""),
];

const BOILERPLATE_TITLE_PREFIXES: [&str; 5] = [
    "brief report",
    "clinical note",
    "case report",
    "short communication",
    "letter",
];

pub(super) fn convert_unicode_string(input: &str) -> String {
    UNICODE_REGEX
        .replace_all(input, |caps: &crate::regex::Captures| {
            u32::from_str_radix(&caps[1], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .to_string()
}

pub(super) fn normalize_string(string: &str) -> String {
    if string.is_empty() {
        return String::new();
    }

    let mut normalized = convert_unicode_string(string);
    normalized = decode_html_entities(&normalized);
    normalized = normalized.trim().to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(stripped) = normalized.strip_prefix(article) {
            normalized = stripped.to_string();
            break;
        }
    }
    for (needle, replacement) in HTML_TAG_REPLACEMENTS {
        normalized = normalized.replace(needle, replacement);
    }
    for (needle, replacement) in TITLE_MARKUP_TOKEN_REPLACEMENTS {
        normalized = normalized.replace(needle, replacement);
    }
    normalized = expand_greek_characters(&normalized);

    normalized
        .nfkd()
        .filter(|ch| !is_combining_mark(*ch))
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

pub(super) fn strip_boilerplate_title_prefix(title: &str) -> Option<&str> {
    let trimmed = title.trim();
    let lower = trimmed.to_lowercase();

    for prefix in BOILERPLATE_TITLE_PREFIXES {
        if let Some(remainder) = lower.strip_prefix(prefix) {
            if remainder.is_empty()
                || !remainder
                    .chars()
                    .next()
                    .is_some_and(is_boilerplate_separator)
            {
                continue;
            }

            let original_remainder = &trimmed[prefix.len()..];
            let stripped = original_remainder
                .trim_start_matches(is_boilerplate_separator)
                .trim();
            if !stripped.is_empty() {
                return Some(stripped);
            }
        }
    }

    None
}

pub(super) fn is_boilerplate_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '.' | ':' | ';' | ',' | '-' | '–' | '—')
}

pub(super) fn decode_html_entities(input: &str) -> String {
    HTML_ENTITY_REGEX
        .replace_all(input, |caps: &crate::regex::Captures| {
            decode_html_entity(&caps[1]).unwrap_or_else(|| caps[0].to_string())
        })
        .to_string()
}

pub(super) fn decode_html_entity(entity: &str) -> Option<String> {
    if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        return u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .map(|ch| ch.to_string());
    }

    if let Some(decimal) = entity.strip_prefix('#') {
        return decimal
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|ch| ch.to_string());
    }

    match entity.to_ascii_lowercase().as_str() {
        "amp" => Some("&".to_string()),
        "quot" => Some("\"".to_string()),
        "apos" => Some("'".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "nbsp" => Some(" ".to_string()),
        "alpha" => Some("\u{03B1}".to_string()),
        "beta" => Some("\u{03B2}".to_string()),
        "gamma" => Some("\u{03B3}".to_string()),
        "delta" => Some("\u{03B4}".to_string()),
        "epsilon" => Some("\u{03B5}".to_string()),
        "kappa" => Some("\u{03BA}".to_string()),
        "lambda" => Some("\u{03BB}".to_string()),
        "mu" => Some("\u{03BC}".to_string()),
        "omega" => Some("\u{03C9}".to_string()),
        _ => None,
    }
}

pub(super) fn expand_greek_characters(input: &str) -> String {
    let mut expanded = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\u{00DF}' | '\u{03B2}' => expanded.push_str("beta"),
            '\u{03B1}' => expanded.push_str("alpha"),
            '\u{03B3}' => expanded.push_str("gamma"),
            '\u{03B4}' => expanded.push_str("delta"),
            '\u{03B5}' => expanded.push_str("epsilon"),
            '\u{03B6}' => expanded.push_str("zeta"),
            '\u{03B7}' => expanded.push_str("eta"),
            '\u{03B8}' => expanded.push_str("theta"),
            '\u{03B9}' => expanded.push_str("iota"),
            '\u{03BA}' => expanded.push_str("kappa"),
            '\u{03BB}' => expanded.push_str("lambda"),
            '\u{03BC}' | '\u{00B5}' => expanded.push_str("mu"),
            '\u{03BD}' => expanded.push_str("nu"),
            '\u{03BE}' => expanded.push_str("xi"),
            '\u{03BF}' => expanded.push_str("omicron"),
            '\u{03C0}' => expanded.push_str("pi"),
            '\u{03C1}' => expanded.push_str("rho"),
            '\u{03C3}' | '\u{03C2}' => expanded.push_str("sigma"),
            '\u{03C4}' => expanded.push_str("tau"),
            '\u{03C5}' => expanded.push_str("upsilon"),
            '\u{03C6}' => expanded.push_str("phi"),
            '\u{03C7}' => expanded.push_str("chi"),
            '\u{03C8}' => expanded.push_str("psi"),
            '\u{03C9}' => expanded.push_str("omega"),
            _ => expanded.push(ch),
        }
    }

    expanded
}

pub(super) fn normalize_volume(volume: &str) -> String {
    if volume.is_empty() {
        return String::new();
    }

    let numbers: String = volume
        .chars()
        .skip_while(|c| !c.is_numeric())
        .take_while(|c| c.is_numeric())
        .collect();

    if numbers.is_empty() {
        String::new()
    } else {
        numbers
    }
}

pub(super) fn normalize_issue(issue: &str) -> String {
    if issue.is_empty() {
        return String::new();
    }

    let mut normalized = issue.trim().to_lowercase();
    normalized = normalized
        .trim_matches(|ch: char| matches!(ch, '(' | ')' | '[' | ']'))
        .to_string();

    for prefix in ["issue", "no.", "no", "number", "nr.", "nr"] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped
                .trim_start_matches(|ch: char| {
                    ch.is_whitespace() || matches!(ch, '.' | ':' | '-' | '/' | '#')
                })
                .trim()
                .to_string();
            break;
        }
    }

    normalized
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

pub(super) fn normalize_start_page(pages: &str) -> Option<String> {
    let formatted = format_page_numbers(pages);
    let start_segment = formatted.split('-').next().unwrap_or("");
    let canonical_start = start_segment
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();

    if matches!(canonical_start.as_str(), "npag" | "nopagination" | "na") {
        return None;
    }

    let start_page = start_segment
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();

    (!start_page.is_empty()).then_some(start_page)
}

pub(super) fn normalize_author_key(author_name: &str) -> Option<String> {
    let normalized = author_name
        .trim()
        .to_lowercase()
        .nfkd()
        .filter(|ch| !is_combining_mark(*ch))
        .filter(|ch| ch.is_alphanumeric())
        .collect::<String>();

    (!normalized.is_empty()).then_some(normalized)
}

pub(super) fn format_journal_name(full_name: Option<&str>) -> Option<String> {
    full_name.map(|name| {
        let normalized = name
            .split(". Conference")
            .next()
            .unwrap_or(name)
            .trim()
            .to_lowercase();
        let normalized = normalized.strip_prefix("the ").unwrap_or(&normalized);
        normalized
            .replace("&amp;", " and ")
            .replace('&', " and ")
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .collect::<String>()
    })
}

pub(super) fn format_issn(issn_str: &str) -> Option<String> {
    let clean_issn = issn_str
        .trim()
        .to_uppercase()
        .replace("(ELECTRONIC)", "")
        .replace("(LINKING)", "")
        .replace("(PRINT)", "")
        .replace(
            |ch: char| !ch.is_ascii_digit() && ch != '-' && ch != 'X',
            "",
        )
        .trim()
        .to_string();

    let digits: String = clean_issn
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == 'X')
        .collect();

    if digits[..digits.len().saturating_sub(1)].contains('X') {
        return None;
    }

    match (clean_issn.len(), digits.len()) {
        (9, 8) if clean_issn.chars().nth(4) == Some('-') => Some(clean_issn),
        (8, 8) => Some(format!("{}-{}", &digits[..4], &digits[4..])),
        _ => None,
    }
}
