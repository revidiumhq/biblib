use crate::pubmed::author::{ConsecutiveTag, resolve_authors};
use crate::pubmed::split::BlankLineSplit;
use crate::pubmed::structure::RawPubmedData;
use crate::pubmed::tags::PubmedTag;
use crate::pubmed::whole_lines::WholeLinesIter;
use crate::error::SourceSpan;
use crate::utils::newline_delimiter_of;
use either::{Either, Left, Right};
use itertools::Itertools;
use std::collections::HashMap;

/// Parse the content of a PubMed formatted .nbib file, returning its key-value pairs
/// in a [HashMap] (with the order of duplicate values preserved in the [Vec] values)
/// alongside any unparsable lines.
pub fn pubmed_parse<S: AsRef<str>>(nbib_text: S) -> Vec<RawPubmedData> {
    let text = nbib_text.as_ref();
    let text_ptr = text.as_ptr() as usize;
    let line_break = newline_delimiter_of(text);
    BlankLineSplit::new(text, line_break)
        .map(|(line_number, chunk)| {
            let chunk_start = chunk.as_ptr() as usize - text_ptr;
            pubmed_parse_one(chunk, line_break, line_number, chunk_start)
        })
        .collect() // TODO do not collect, return an Iterator instead
}

fn pubmed_parse_one(text: &str, line_break: &str, start_line: usize, start_byte: usize) -> RawPubmedData {
    let (mut ignored_lines, pairs): (Vec<_>, Vec<_>) =
        WholeLinesIter::new(text.split(line_break)).partition_map(parse_complete_entry);
    let (data, others) = separate_stateless_entries(pairs);
    let (authors, leading_affiliations) = resolve_authors(others);
    ignored_lines.extend(
        leading_affiliations
            .into_iter()
            .map(|s| format!("AD - {s}")),
    );
    RawPubmedData {
        data,
        authors,
        ignored_lines,
        start_line,
        record_span: SourceSpan::new(start_byte, start_byte + text.len()),
    }
}

/// Collect the data: tags which can be parsed statelessly are stored in a [HashMap],
/// with duplicates kept in a [Vec] with order preserved, while other tags that require
/// context to parse are stored in a vec with order preserved.
#[allow(clippy::type_complexity)]
fn separate_stateless_entries<V>(
    v: Vec<(PubmedTag, V)>,
) -> (HashMap<PubmedTag, Vec<V>>, Vec<(ConsecutiveTag, V)>) {
    let mut map = HashMap::with_capacity(v.len());
    let mut other = Vec::with_capacity(v.len());
    for (k, v) in v {
        if let Some(tag) = ConsecutiveTag::from_tag(k) {
            other.push((tag, v))
        } else {
            let bucket = map.entry(k).or_insert_with(Vec::new);
            bucket.push(v);
        }
    }
    (map, other)
}

/// Parse the string as a key-value pair from a PubMed formatted .nbib file.
fn parse_complete_entry(line: String) -> Either<String, (PubmedTag, String)> {
    split_on_dash(&line)
        .and_then(|(k, v)| match_pubmed_key(k, v))
        .map(|(k, v)| Right((k, v.to_string())))
        .unwrap_or_else(|| Left(line))
}

/// Match `key` with a known [PubmedTag].
fn match_pubmed_key<S: AsRef<str>, V>(key: S, value: V) -> Option<(PubmedTag, V)> {
    PubmedTag::from_tag(key.as_ref()).map(|tag| (tag, value))
}

/// Split on the first `-` character and remove the whitespace surrounding the removed `-`.
fn split_on_dash(line: &str) -> Option<(&str, &str)> {
    line.split_once('-')
        .map(|(l, r)| (l.trim_end(), r.trim_start()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[rstest]
    #[case("", Left(""))]
    #[case("DNE - tag does not exist", Left("DNE - tag does not exist"))]
    #[case("AU - Albert Einstein", Right((PubmedTag::Author, "Albert Einstein")))]
    #[case("AU- Albert Einstein", Right((PubmedTag::Author, "Albert Einstein")))]
    #[case("AU -Albert Einstein", Right((PubmedTag::Author, "Albert Einstein")))]
    #[case("AU  - Albert Einstein", Right((PubmedTag::Author, "Albert Einstein")))]
    fn test_parse_complete_entry(
        #[case] line: &str,
        #[case] expected: Either<&str, (PubmedTag, &str)>,
    ) {
        let actual = parse_complete_entry(line.to_string());
        assert_eq!(
            actual
                .as_ref()
                .map_either(|s| s.as_str(), |(t, s)| (t.clone(), s.as_str())),
            expected
        )
    }
}
