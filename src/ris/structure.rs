//! RIS format data structures.
//!
//! This module defines intermediate data structures used during RIS parsing.
//!
//! # Design Decision
//!
//! ## Field Processing Strategy
//! - **Priority-based**: Journal names/abbreviations use documented priority systems
//! - **First-wins**: Simple fields like title use the first valid value found
//! - **Two-pass**: DOI extraction checks dedicated fields first, then URLs
//! - **Validation**: Date parsing includes error logging for invalid formats

use crate::Author;
use crate::ris::tags::RisTag;
use std::collections::HashMap;

/// Structured raw data from a RIS formatted file.
#[derive(Debug, Clone)]
pub(crate) struct RawRisData {
    /// Key-value pair data from the RIS file data.
    pub(crate) data: HashMap<RisTag, Vec<String>>,
    /// Authors of the cited work.
    pub(crate) authors: Vec<Author>,
    /// Invalid lines found in the RIS file data with line number context for error reporting.
    pub(crate) ignored_lines: Vec<(usize, String)>,
}

impl RawRisData {
    /// Create a new empty RawRisData.
    pub(crate) fn new() -> Self {
        Self {
            data: HashMap::new(),
            authors: Vec::new(),
            ignored_lines: Vec::new(),
        }
    }

    /// Add a tag-value pair to the data.
    pub(crate) fn add_data(&mut self, tag: RisTag, value: String) {
        self.data.entry(tag).or_default().push(value);
    }

    /// Add an author to the authors list.
    pub(crate) fn add_author(&mut self, author: Author) {
        self.authors.push(author);
    }

    /// Add an ignored line with context.
    pub(crate) fn add_ignored_line(&mut self, line_number: usize, line: String) {
        self.ignored_lines.push((line_number, line));
    }

    /// Get the first value for a tag, if it exists.
    pub(crate) fn get_first(&self, tag: &RisTag) -> Option<&String> {
        self.data.get(tag).and_then(|values| values.first())
    }

    /// Remove and return all values for a tag.
    pub(crate) fn remove(&mut self, tag: &RisTag) -> Option<Vec<String>> {
        self.data.remove(tag)
    }

    /// Check if the data contains any content (not just metadata).
    pub(crate) fn has_content(&self) -> bool {
        !self.data.is_empty() || !self.authors.is_empty()
    }

    /// Generic helper method to select the best value based on tag priority.
    ///
    /// # Arguments
    /// * `priority_fn` - A function that extracts the priority from a RisTag, returning None if the tag is not relevant
    fn get_best_value_by_priority<F>(&self, priority_fn: F) -> Option<String>
    where
        F: Fn(&RisTag) -> Option<u8>,
    {
        let mut best_value = None;
        let mut best_priority = u8::MAX;

        for (tag, values) in &self.data {
            if let Some(priority) = priority_fn(tag)
                && priority < best_priority && !values.is_empty()
                    && let Some(first_value) = values.first()
                        && !first_value.trim().is_empty() {
                            best_priority = priority;
                            best_value = Some(first_value.clone());
                        }
        }

        best_value
    }

    /// Get the best journal name based on tag priority.
    pub(crate) fn get_best_journal(&self) -> Option<String> {
        self.get_best_value_by_priority(|tag| tag.journal_priority())
    }

    /// Get the best journal abbreviation based on tag priority.
    pub(crate) fn get_best_journal_abbr(&self) -> Option<String> {
        self.get_best_value_by_priority(|tag| tag.journal_abbr_priority())
    }
}

impl TryFrom<RawRisData> for crate::Citation {
    type Error = crate::error::ParseError;

    fn try_from(mut raw: RawRisData) -> Result<Self, Self::Error> {
        let citation_type = raw.remove(&RisTag::Type).unwrap_or_default();
        let title = Self::extract_title(&mut raw)?;
        let (journal, journal_abbr) = Self::extract_journal_info(&mut raw);
        let date = Self::extract_date(&mut raw);
        let (volume, issue, pages) = Self::extract_publication_details(&mut raw);
        let (doi, urls) = Self::extract_doi_and_urls(&mut raw);
        let (pmid, pmc_id) = Self::extract_identifiers(&mut raw);
        let abstract_text = Self::extract_abstract(&mut raw);
        let keywords = raw.remove(&RisTag::Keywords).unwrap_or_default();
        let issn = raw.remove(&RisTag::SerialNumber).unwrap_or_default();
        let (language, publisher) = Self::extract_metadata(&mut raw);
        let extra_fields = Self::extract_extra_fields(&mut raw);

        Ok(crate::Citation {
            citation_type,
            title,
            authors: raw.authors,
            journal,
            journal_abbr,
            date: date.clone(),
            volume,
            issue,
            pages,
            issn,
            doi,
            pmid,
            pmc_id,
            abstract_text,
            keywords,
            urls,
            language,
            mesh_terms: Vec::new(), // RIS doesn't typically have MeSH terms
            publisher,
            extra_fields,
        })
    }
}

impl crate::Citation {
    /// Extract title from RIS data, trying primary title first, then alternative.
    fn extract_title(raw: &mut RawRisData) -> Result<String, crate::error::ParseError> {
        let title = raw
            .get_first(&RisTag::Title)
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                raw.get_first(&RisTag::TitleAlternative)
                    .filter(|s| !s.trim().is_empty())
            })
            .cloned()
            .ok_or_else(|| {
                crate::error::ParseError::without_position(
                    crate::CitationFormat::Ris,
                    crate::error::ValueError::MissingValue {
                        field: crate::error::fields::TITLE,
                        key: "TI",
                    },
                )
            })?;

        // Remove title data after extraction
        raw.remove(&RisTag::Title);
        raw.remove(&RisTag::TitleAlternative);

        Ok(title)
    }

    /// Extract journal information using priority-based selection.
    fn extract_journal_info(raw: &mut RawRisData) -> (Option<String>, Option<String>) {
        let journal = raw.get_best_journal();
        let journal_abbr = raw.get_best_journal_abbr();

        // Clean up journal data
        raw.remove(&RisTag::JournalFull);
        raw.remove(&RisTag::JournalFullAlternative);
        raw.remove(&RisTag::JournalAbbreviation);
        raw.remove(&RisTag::JournalAbbreviationAlternative);
        raw.remove(&RisTag::SecondaryTitle);

        (journal, journal_abbr)
    }

    /// Extract date from RIS data with validation.
    fn extract_date(raw: &mut RawRisData) -> Option<crate::Date> {
        // Parse date from available date fields with validation
        let date = raw
            .get_first(&RisTag::PublicationYear)
            .or_else(|| raw.get_first(&RisTag::DatePrimary))
            .and_then(|date_str| {
                crate::utils::parse_ris_date(date_str)
                // Note: Invalid dates are silently ignored to avoid breaking parsing
                // TODO: Collect warnings
            });

        raw.remove(&RisTag::PublicationYear);
        raw.remove(&RisTag::DatePrimary);
        raw.remove(&RisTag::DateAccess);

        date
    }

    /// Extract publication details: volume, issue, and formatted pages.
    fn extract_publication_details(
        raw: &mut RawRisData,
    ) -> (Option<String>, Option<String>, Option<String>) {
        let volume = raw
            .remove(&RisTag::Volume)
            .and_then(|v| v.into_iter().next());
        let issue = raw
            .remove(&RisTag::Issue)
            .and_then(|v| v.into_iter().next());

        // Handle pages
        let start_page = raw
            .remove(&RisTag::StartPage)
            .and_then(|v| v.into_iter().next());
        let end_page = raw
            .remove(&RisTag::EndPage)
            .and_then(|v| v.into_iter().next());
        let pages = match (start_page, end_page) {
            (Some(start), Some(end)) => Some(crate::utils::format_page_numbers(&format!(
                "{}-{}",
                start, end
            ))),
            (Some(start), None) => Some(crate::utils::format_page_numbers(&start)),
            (None, Some(end)) => Some(end),
            (None, None) => None,
        };

        (volume, issue, pages)
    }

    /// Extract DOI and URLs with two-pass DOI extraction strategy.
    fn extract_doi_and_urls(raw: &mut RawRisData) -> (Option<String>, Vec<String>) {
        // First pass: Extract DOI from dedicated DOI field
        let mut doi = raw
            .remove(&RisTag::Doi)
            .and_then(|v| v.into_iter().next())
            .and_then(|doi_str| crate::utils::format_doi(&doi_str));

        // Collect URLs from various link fields and extract DOI if not already found
        let mut urls = Vec::new();
        for tag in [
            RisTag::LinkPdf,
            RisTag::LinkFullText,
            RisTag::LinkRelated,
            RisTag::LinkImages,
            RisTag::Url,
            RisTag::Link,
        ] {
            if let Some(mut tag_urls) = raw.remove(&tag) {
                // Second pass: Extract DOI from URL fields if not already found
                if doi.is_none() {
                    for url in &tag_urls {
                        if url.contains("doi.org")
                            && let Some(extracted_doi) = crate::utils::format_doi(url) {
                                doi = Some(extracted_doi);
                                break;
                            }
                    }
                }
                urls.append(&mut tag_urls);
            }
        }

        (doi, urls)
    }

    /// Extract PMID and PMC ID identifiers.
    fn extract_identifiers(raw: &mut RawRisData) -> (Option<String>, Option<String>) {
        let pmid = raw
            .remove(&RisTag::ReferenceId)
            .and_then(|v| v.into_iter().next());

        let pmc_id = raw
            .remove(&RisTag::PmcId)
            .and_then(|v| v.into_iter().next())
            .filter(|s| s.contains("PMC"));

        (pmid, pmc_id)
    }

    /// Extract abstract text from primary or alternative abstract fields.
    fn extract_abstract(raw: &mut RawRisData) -> Option<String> {
        let abstract_text = raw
            .get_first(&RisTag::Abstract)
            .or_else(|| raw.get_first(&RisTag::AbstractAlternative))
            .cloned();

        raw.remove(&RisTag::Abstract);
        raw.remove(&RisTag::AbstractAlternative);

        abstract_text
    }

    /// Extract language and publisher metadata.
    fn extract_metadata(raw: &mut RawRisData) -> (Option<String>, Option<String>) {
        let language = raw
            .remove(&RisTag::Language)
            .and_then(|v| v.into_iter().next());
        let publisher = raw
            .remove(&RisTag::Publisher)
            .and_then(|v| v.into_iter().next());

        (language, publisher)
    }

    /// Extract remaining fields as extra_fields after removing end-of-reference marker.
    fn extract_extra_fields(raw: &mut RawRisData) -> HashMap<String, Vec<String>> {
        // Remove end-of-reference marker
        raw.remove(&RisTag::EndOfReference);

        // Collect remaining fields as extra_fields
        raw.data
            .drain()
            .map(|(tag, values)| (tag.as_tag().to_string(), values))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ris::tags::RisTag;

    #[test]
    fn test_raw_ris_data_new() {
        let raw = RawRisData::new();
        assert!(raw.data.is_empty());
        assert!(raw.authors.is_empty());
        assert!(raw.ignored_lines.is_empty());
        assert!(!raw.has_content());
    }

    #[test]
    fn test_add_data() {
        let mut raw = RawRisData::new();
        raw.add_data(RisTag::Title, "Test Title".to_string());
        raw.add_data(RisTag::Title, "Another Title".to_string());

        assert_eq!(
            raw.get_first(&RisTag::Title),
            Some(&"Test Title".to_string())
        );
        assert!(raw.has_content());
    }

    #[test]
    fn test_journal_priority() {
        let mut raw = RawRisData::new();
        raw.add_data(RisTag::JournalFullAlternative, "Alt Journal".to_string());
        raw.add_data(RisTag::JournalFull, "Main Journal".to_string());
        raw.add_data(RisTag::SecondaryTitle, "Secondary".to_string());

        assert_eq!(raw.get_best_journal(), Some("Main Journal".to_string()));
    }

    #[test]
    fn test_conversion_to_citation() {
        let mut raw = RawRisData::new();
        raw.add_data(RisTag::Type, "JOUR".to_string());
        raw.add_data(RisTag::Title, "Test Article".to_string());
        raw.add_author(Author {
            name: "Smith".to_string(),
            given_name: Some("John".to_string()),
            middle_name: None,
            affiliations: Vec::new(),
        });

        let citation: crate::Citation = raw.try_into().unwrap();
        assert_eq!(citation.title, "Test Article");
        assert_eq!(citation.citation_type, vec!["JOUR"]);
        assert_eq!(citation.authors.len(), 1);
    }

    #[test]
    fn test_missing_title_error() {
        let raw = RawRisData::new();
        let result: Result<crate::Citation, _> = raw.try_into();
        assert!(matches!(result, Err(_parse_err)));
    }

    #[test]
    fn test_doi_extraction_from_urls() {
        let mut raw = RawRisData::new();
        raw.add_data(RisTag::Type, "JOUR".to_string());
        raw.add_data(RisTag::Title, "Test Article".to_string());
        // Add a DOI URL without a dedicated DOI field
        raw.add_data(RisTag::Url, "https://doi.org/10.1234/example".to_string());
        raw.add_data(RisTag::LinkPdf, "https://example.com/pdf".to_string());

        let citation: crate::Citation = raw.try_into().unwrap();
        assert_eq!(citation.doi, Some("10.1234/example".to_string()));
        assert_eq!(citation.urls.len(), 2);
        assert!(
            citation
                .urls
                .contains(&"https://doi.org/10.1234/example".to_string())
        );
    }

    #[test]
    fn test_doi_extraction_prioritizes_doi_field() {
        let mut raw = RawRisData::new();
        raw.add_data(RisTag::Type, "JOUR".to_string());
        raw.add_data(RisTag::Title, "Test Article".to_string());
        // Add both dedicated DOI field and DOI URL
        raw.add_data(RisTag::Doi, "10.5678/primary".to_string());
        raw.add_data(RisTag::Url, "https://doi.org/10.1234/secondary".to_string());

        let citation: crate::Citation = raw.try_into().unwrap();
        // Should prioritize the dedicated DOI field
        assert_eq!(citation.doi, Some("10.5678/primary".to_string()));
    }

    #[test]
    fn test_title_extraction_edge_cases() {
        // Test that empty title falls back to alternative
        let mut raw = RawRisData::new();
        raw.add_data(RisTag::Type, "JOUR".to_string());
        raw.add_data(RisTag::Title, "".to_string());
        raw.add_data(RisTag::TitleAlternative, "Fallback Title".to_string());

        let citation: crate::Citation = raw.try_into().unwrap();
        assert_eq!(citation.title, "Fallback Title");

        // Test fallback works when primary title is completely missing
        let mut raw2 = RawRisData::new();
        raw2.add_data(RisTag::Type, "JOUR".to_string());
        raw2.add_data(RisTag::TitleAlternative, "Fallback Title".to_string());

        let citation2: crate::Citation = raw2.try_into().unwrap();
        assert_eq!(citation2.title, "Fallback Title");

        // Test that whitespace-only title also falls back
        let mut raw3 = RawRisData::new();
        raw3.add_data(RisTag::Type, "JOUR".to_string());
        raw3.add_data(RisTag::Title, "   ".to_string());
        raw3.add_data(RisTag::TitleAlternative, "Fallback Title".to_string());

        let citation3: crate::Citation = raw3.try_into().unwrap();
        assert_eq!(citation3.title, "Fallback Title");
    }

    #[test]
    fn test_complex_doi_extraction_scenarios() {
        let mut raw = RawRisData::new();
        raw.add_data(RisTag::Type, "JOUR".to_string());
        raw.add_data(RisTag::Title, "Test Article".to_string());

        // Test malformed DOI URL handling - should not extract DOI from malformed URLs
        raw.add_data(RisTag::Url, "https://malformed-doi-url".to_string());
        raw.add_data(RisTag::LinkPdf, "https://doi.org/malformed".to_string());

        let citation: crate::Citation = raw.try_into().unwrap();
        // Should handle malformed DOIs gracefully - no DOI should be extracted
        assert_eq!(
            citation.doi, None,
            "Should not extract DOI from malformed URLs"
        );
        assert_eq!(citation.urls.len(), 2, "Should still preserve all URLs");
        assert!(
            citation
                .urls
                .contains(&"https://malformed-doi-url".to_string())
        );
        assert!(
            citation
                .urls
                .contains(&"https://doi.org/malformed".to_string())
        );
    }

    #[test]
    fn test_journal_priority_with_empty_values() {
        let mut raw = RawRisData::new();
        raw.add_data(RisTag::JournalFull, "".to_string()); // Empty primary
        raw.add_data(RisTag::SecondaryTitle, "Secondary Journal".to_string());
        raw.add_data(RisTag::JournalFullAlternative, "Alt Journal".to_string());

        // Should skip empty values and pick the next priority
        assert_eq!(
            raw.get_best_journal(),
            Some("Secondary Journal".to_string())
        );
    }
}
