//! CSV format data structures.
//!
//! This module defines intermediate data structures used during CSV parsing.

use crate::csv::config::CsvConfig;
use crate::error::{ParseError, SourceSpan, ValueError, fields};
use crate::{Author, CitationFormat};
use csv::StringRecord;
use std::collections::HashMap;

/// Structured raw data from a CSV file.
#[derive(Debug, Clone)]
pub(crate) struct RawCsvData {
    /// Raw field data from the CSV record
    pub(crate) fields: HashMap<String, String>,
    /// Authors parsed from the authors field
    pub(crate) authors: Vec<Author>,
    /// Keywords parsed from the keywords field
    pub(crate) keywords: Vec<String>,
    /// URLs parsed from URL fields
    pub(crate) urls: Vec<String>,
    /// ISSN values parsed from ISSN fields
    pub(crate) issn: Vec<String>,
    /// Line number for error reporting
    pub(crate) line_number: usize,
    /// Byte offset of the record start in the source text.
    pub(crate) byte_offset: usize,
    /// Original record for debugging (optional for memory efficiency)
    pub(crate) original_record: Option<Vec<String>>,
}

impl RawCsvData {
    /// Create a new RawCsvData from a CSV record and headers.
    pub(crate) fn from_record(
        headers: &[String],
        record: &StringRecord,
        config: &CsvConfig,
        line_number: usize,
        byte_offset: usize,
    ) -> Result<Self, ParseError> {
        let mut fields = HashMap::new();
        let mut authors = Vec::new();
        let mut keywords = Vec::new();
        let mut urls = Vec::new();
        let mut issn = Vec::new();

        // Store original record for debugging if enabled
        let original_record = if config.store_original_record {
            Some(record.iter().map(|s| s.to_string()).collect())
        } else {
            None
        };

        for (i, value) in record.iter().enumerate() {
            if i >= headers.len() {
                if !config.flexible {
                    return Err(ParseError::at_line(
                        line_number,
                        CitationFormat::Csv,
                        ValueError::Syntax(format!(
                            "Record has more fields ({}) than headers ({})",
                            record.len(),
                            headers.len()
                        )),
                    ));
                }
                break;
            }

            let header = &headers[i];
            let value = if config.trim { value.trim() } else { value };

            if value.is_empty() {
                continue;
            }

            if let Some(field) = config.get_field_for_header(header) {
                match field {
                    "authors" => {
                        for author_str in value.split(';') {
                            let author_str = author_str.trim();
                            if !author_str.is_empty() {
                                let (family, given) = crate::utils::parse_author_name(author_str);
                                let (given_opt, middle_opt) = if given.is_empty() {
                                    (None, None)
                                } else {
                                    crate::utils::split_given_and_middle(&given)
                                };
                                authors.push(crate::Author {
                                    name: family,
                                    given_name: given_opt,
                                    middle_name: middle_opt,
                                    affiliations: Vec::new(),
                                });
                            }
                        }
                    }
                    "keywords" => {
                        keywords.extend(
                            value
                                .split(';')
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(String::from),
                        );
                    }
                    "url" => {
                        urls.push(value.to_string());
                    }
                    "issn" => {
                        issn.extend(crate::utils::split_issns(value));
                    }
                    _ => {
                        fields.insert(field.to_string(), value.to_string());
                    }
                }
            } else {
                // Store unknown fields as-is
                fields.insert(header.clone(), value.to_string());
            }
        }

        Ok(RawCsvData {
            fields,
            authors,
            keywords,
            urls,
            issn,
            line_number,
            byte_offset,
            original_record,
        })
    }

    /// Convert to Citation with proper extra fields handling
    pub(crate) fn into_citation_with_config(
        self,
        config: &CsvConfig,
    ) -> Result<crate::Citation, crate::error::CitationError> {
        let title = self.get_field("title").cloned().ok_or_else(|| {
            ParseError::at_line(
                self.line_number,
                CitationFormat::Csv,
                ValueError::MissingValue {
                    field: fields::TITLE,
                    key: "title",
                },
            )
            .with_span(SourceSpan::new(self.byte_offset, self.byte_offset))
        })?;

        let journal = self.get_field("journal").cloned();
        let journal_abbr = self.get_field("journal_abbr").cloned();

        // Parse date/year
        let date = self
            .get_field("year")
            .and_then(|year_str| crate::utils::parse_year_only(year_str));

        let volume = self.get_field("volume").cloned();
        let issue = self.get_field("issue").cloned();

        let pages = self
            .get_field("pages")
            .map(|p| crate::utils::format_page_numbers(p));

        let doi = self
            .get_field("doi")
            .and_then(|doi_str| crate::utils::format_doi(doi_str));

        let abstract_text = self.get_field("abstract").cloned();
        let language = self.get_field("language").cloned();
        let publisher = self.get_field("publisher").cloned();

        // Create citation type - default to "Journal Article" if not specified
        let citation_type = self
            .get_field("type")
            .map(|t| vec![t.clone()])
            .unwrap_or_else(|| vec!["Journal Article".to_string()]);

        // Properly extract extra fields using the config
        let extra_fields = self.get_extra_fields(config);

        Ok(crate::Citation {
            citation_type,
            title,
            authors: self.authors.clone(),
            journal,
            journal_abbr,
            date: date.clone(),
            volume,
            issue,
            pages,
            issn: self.issn.clone(),
            doi,
            pmid: self.get_field("pmid").cloned(),
            pmc_id: self.get_field("pmc_id").cloned(),
            abstract_text,
            keywords: self.keywords.clone(),
            urls: self.urls.clone(),
            language,
            mesh_terms: Vec::new(), // CSV typically doesn't have MeSH terms
            publisher,
            extra_fields,
        })
    }

    /// Get a field value by name.
    pub(crate) fn get_field(&self, field: &str) -> Option<&String> {
        self.fields.get(field)
    }

    /// Check if the record has any meaningful content.
    pub(crate) fn has_content(&self) -> bool {
        !self.fields.is_empty() || !self.authors.is_empty()
    }

    /// Get all extra fields (those not mapped to standard citation fields).
    pub(crate) fn get_extra_fields(&self, config: &CsvConfig) -> HashMap<String, Vec<String>> {
        let mut extra_fields = HashMap::new();

        // Find fields that aren't mapped to standard citation fields
        for (field_name, value) in &self.fields {
            if !is_standard_field(field_name, config) {
                extra_fields.insert(field_name.clone(), vec![value.clone()]);
            }
        }

        extra_fields
    }
}

/// Check if a field name corresponds to a standard citation field.
fn is_standard_field(field_name: &str, config: &CsvConfig) -> bool {
    const STANDARD_FIELDS: &[&str] = &[
        "title",
        "authors",
        "journal",
        "journal_abbr",
        "year",
        "volume",
        "issue",
        "pages",
        "doi",
        "pmid",
        "pmc_id",
        "abstract",
        "keywords",
        "issn",
        "language",
        "publisher",
        "type",
        "url",
    ];

    STANDARD_FIELDS
        .iter()
        .any(|&standard| config.get_field_for_header(field_name) == Some(standard))
}

impl TryFrom<RawCsvData> for crate::Citation {
    type Error = crate::error::CitationError;

    fn try_from(raw: RawCsvData) -> Result<Self, Self::Error> {
        // Use default config for backward compatibility
        // Note: This uses default field mappings; use into_citation_with_config for custom mappings
        let config = CsvConfig::new();
        raw.into_citation_with_config(&config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv::StringRecord;

    fn create_test_record(fields: &[&str]) -> StringRecord {
        let mut record = StringRecord::new();
        for field in fields {
            record.push_field(field);
        }
        record
    }

    #[test]
    fn test_from_record_basic() {
        let headers = vec!["Title".to_string(), "Author".to_string()];
        let record = create_test_record(&["Test Article", "Smith, John"]);
        let config = CsvConfig::new();

        let raw = RawCsvData::from_record(&headers, &record, &config, 1, 0).unwrap();

        assert_eq!(raw.get_field("title"), Some(&"Test Article".to_string()));
        assert_eq!(raw.authors.len(), 1);
        assert_eq!(raw.authors[0].name, "Smith");
        assert!(raw.has_content());
    }

    #[test]
    fn test_from_record_multiple_authors() {
        let headers = vec!["Authors".to_string()];
        let record = create_test_record(&["Smith, John; Doe, Jane"]);
        let config = CsvConfig::new();

        let raw = RawCsvData::from_record(&headers, &record, &config, 1, 0).unwrap();

        assert_eq!(raw.authors.len(), 2);
        assert_eq!(raw.authors[0].name, "Smith");
        assert_eq!(raw.authors[1].name, "Doe");
    }

    #[test]
    fn test_from_record_keywords() {
        let headers = vec!["Keywords".to_string()];
        let record = create_test_record(&["keyword1; keyword2; keyword3"]);
        let config = CsvConfig::new();

        let raw = RawCsvData::from_record(&headers, &record, &config, 1, 0).unwrap();

        assert_eq!(raw.keywords.len(), 3);
        assert!(raw.keywords.contains(&"keyword1".to_string()));
    }

    #[test]
    fn test_from_record_too_many_fields_strict() {
        let headers = vec!["Title".to_string()];
        let record = create_test_record(&["Test Article", "Extra Field"]);
        let config = CsvConfig::new(); // flexible = false by default

        let result = RawCsvData::from_record(&headers, &record, &config, 1, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_record_too_many_fields_flexible() {
        let headers = vec!["Title".to_string()];
        let record = create_test_record(&["Test Article", "Extra Field"]);
        let mut config = CsvConfig::new();
        config.set_flexible(true);

        let raw = RawCsvData::from_record(&headers, &record, &config, 1, 0).unwrap();
        assert_eq!(raw.get_field("title"), Some(&"Test Article".to_string()));
    }

    #[test]
    fn test_conversion_to_citation() {
        let headers = vec![
            "Title".to_string(),
            "Author".to_string(),
            "Year".to_string(),
        ];
        let record = create_test_record(&["Test Article", "Smith, John", "2023"]);
        let config = CsvConfig::new();

        let raw = RawCsvData::from_record(&headers, &record, &config, 1, 0).unwrap();
        let citation: crate::Citation = raw.try_into().unwrap();

        assert_eq!(citation.title, "Test Article");
        assert_eq!(citation.authors.len(), 1);
        assert_eq!(citation.date.as_ref().unwrap().year, 2023);
    }

    #[test]
    fn test_missing_title_error() {
        let headers = vec!["Author".to_string()];
        let record = create_test_record(&["Smith, John"]);
        let config = CsvConfig::new();

        let raw = RawCsvData::from_record(&headers, &record, &config, 1, 0).unwrap();
        let result: Result<crate::Citation, _> = raw.try_into();

        // The error is now converted through the legacy bridge
        assert!(result.is_err());
    }
}
