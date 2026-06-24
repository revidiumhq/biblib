use crate::csv::config::CsvConfig;
use crate::csv::parse::csv_parse_with_format;
use crate::csv::structure::RawCsvData;
use crate::error::{ParseError, SourceSpan, ValueError, fields};
use crate::{Citation, CitationFormat, CitationParser, Date};
use csv::ReaderBuilder;
use std::collections::HashMap;

/// Parser for ICTRP CSV exports.
#[derive(Debug, Clone, Default)]
pub struct IctrpCsvParser;

impl IctrpCsvParser {
    /// Creates a new ICTRP CSV parser.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn config() -> CsvConfig {
        let mut config = CsvConfig::new();
        config
            .set_header_mapping("accession_number", vec!["TrialID".to_string()])
            .set_header_mapping("scientific_title", vec!["Scientific title".to_string()])
            .set_header_mapping("date_registration", vec!["Date registration".to_string()])
            .set_header_mapping(
                "date_registration_compact",
                vec!["Date registration3".to_string()],
            )
            .set_header_mapping("publisher", vec!["Primary sponsor".to_string()])
            .set_header_mapping("type", vec!["Study type".to_string()])
            .set_header_mapping(
                "url",
                vec![
                    "web address".to_string(),
                    "results url link".to_string(),
                    "results url protocol".to_string(),
                ],
            );
        config
    }
}

impl CitationParser for IctrpCsvParser {
    fn parse(&self, input: &str) -> Result<Vec<Citation>, ParseError> {
        let config = Self::config();
        let raw_citations = csv_parse_with_format(input, &config, CitationFormat::IctrpCsv)?;

        raw_citations
            .into_iter()
            .map(RawCsvData::into_ictrp_citation)
            .collect()
    }
}

pub(crate) fn looks_like_ictrp_csv(content: &str) -> bool {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(content.as_bytes());

    let Ok(headers) = reader.headers() else {
        return false;
    };

    let header_names = headers
        .iter()
        .map(|header| header.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();

    let has_trial_id = header_names.iter().any(|header| header == "trialid");
    let has_source_register = header_names
        .iter()
        .any(|header| header == "source register");
    let has_title = header_names
        .iter()
        .any(|header| header == "scientific title" || header == "public title");
    let has_registration_date = header_names
        .iter()
        .any(|header| header == "date registration" || header == "date registration3");

    has_trial_id && has_source_register && has_title && has_registration_date
}

impl RawCsvData {
    pub(crate) fn into_ictrp_citation(mut self) -> Result<Citation, ParseError> {
        let accession_number = self
            .fields
            .remove("accession_number")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ParseError::at_line(
                    self.line_number,
                    CitationFormat::IctrpCsv,
                    ValueError::MissingValue {
                        field: fields::ACCESSION_NUMBER,
                        key: "TrialID",
                    },
                )
                .with_span(SourceSpan::new(self.byte_offset, self.byte_offset))
            })?;

        let scientific_title = self
            .fields
            .remove("scientific_title")
            .filter(|value| !value.trim().is_empty());
        let public_title = self.get_field("Public title").cloned();
        let title = scientific_title
            .clone()
            .or_else(|| {
                public_title
                    .clone()
                    .filter(|value| !value.trim().is_empty())
            })
            .ok_or_else(|| {
                ParseError::at_line(
                    self.line_number,
                    CitationFormat::IctrpCsv,
                    ValueError::MissingValue {
                        field: fields::TITLE,
                        key: "Scientific title/Public title",
                    },
                )
                .with_span(SourceSpan::new(self.byte_offset, self.byte_offset))
            })?;

        let date = self
            .fields
            .remove("date_registration_compact")
            .and_then(|value| parse_ictrp_compact_date(&value))
            .or_else(|| {
                self.fields
                    .remove("date_registration")
                    .and_then(|value| parse_ictrp_slash_date(&value))
            });

        let publisher = self.fields.remove("publisher");
        let mut citation_type = vec!["Clinical Trial".to_string()];
        if let Some(study_type) = self.fields.remove("type")
            && !study_type.trim().is_empty()
            && study_type != "Clinical Trial"
        {
            citation_type.push(study_type);
        }

        let mut extra_fields = HashMap::new();
        for (key, value) in self.fields {
            if value.trim().is_empty() {
                continue;
            }
            extra_fields.insert(key, vec![value]);
        }

        Ok(Citation {
            citation_type,
            title,
            authors: Vec::new(),
            journal: None,
            journal_abbr: None,
            date,
            volume: None,
            issue: None,
            pages: None,
            issn: Vec::new(),
            doi: None,
            accession_number: Some(accession_number),
            pmid: None,
            pmc_id: None,
            abstract_text: None,
            keywords: Vec::new(),
            urls: dedupe_urls(self.urls),
            language: None,
            mesh_terms: Vec::new(),
            publisher,
            extra_fields,
        })
    }
}

fn dedupe_urls(urls: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for url in urls {
        if !url.trim().is_empty() && !unique.contains(&url) {
            unique.push(url);
        }
    }
    unique
}

fn parse_ictrp_compact_date(value: &str) -> Option<Date> {
    let trimmed = value.trim();
    if trimmed.len() != 8 {
        return None;
    }

    let year = trimmed[0..4].parse().ok()?;
    let month = trimmed[4..6].parse().ok()?;
    let day = trimmed[6..8].parse().ok()?;

    Some(Date {
        year,
        month: Some(month),
        day: Some(day),
    })
}

fn parse_ictrp_slash_date(value: &str) -> Option<Date> {
    let parts = value.trim().split('/').map(str::trim).collect::<Vec<_>>();

    if parts.len() != 3 {
        return None;
    }

    let (year, month, day) = if parts[0].len() == 4 {
        (
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        )
    } else {
        (
            parts[2].parse().ok()?,
            parts[1].parse().ok()?,
            parts[0].parse().ok()?,
        )
    };

    Some(Date {
        year,
        month: Some(month),
        day: Some(day),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_looks_like_ictrp_csv() {
        let input = concat!(
            "TrialID,Public title,Scientific title,Date registration,Source Register\n",
            "NCT00000001,Public,Scientific,01/05/2026,ClinicalTrials.gov\n"
        );

        assert!(looks_like_ictrp_csv(input));
    }

    #[test]
    fn test_parse_ictrp_csv() {
        let input = concat!(
            "TrialID,Public title,Scientific title,Primary sponsor,Date registration,Date registration3,Study type,web address,results url link,Secondary ID,Source Register\n",
            "NCT00000001,Public title,Scientific title,Sponsor,01/05/2026,20260501,Interventional,https://example.test/study,https://example.test/results,ABC-123,ClinicalTrials.gov\n"
        );

        let citation = IctrpCsvParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.accession_number.as_deref(), Some("NCT00000001"));
        assert_eq!(citation.title, "Scientific title");
        assert_eq!(citation.publisher.as_deref(), Some("Sponsor"));
        assert_eq!(
            citation.citation_type,
            vec!["Clinical Trial", "Interventional"]
        );
        assert_eq!(
            citation.date,
            Some(Date {
                year: 2026,
                month: Some(5),
                day: Some(1)
            })
        );
        assert_eq!(
            citation.extra_fields.get("Public title").unwrap(),
            &vec!["Public title".to_string()]
        );
        assert_eq!(
            citation.extra_fields.get("Secondary ID").unwrap(),
            &vec!["ABC-123".to_string()]
        );
        assert_eq!(citation.urls.len(), 2);
    }

    #[test]
    fn test_parse_ictrp_public_title_fallback() {
        let input = concat!(
            "TrialID,Public title,Scientific title,Date registration,Source Register\n",
            "NCT00000002,Public title,,01/05/2026,ClinicalTrials.gov\n"
        );

        let citation = IctrpCsvParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.title, "Public title");
        assert_eq!(citation.citation_type, vec!["Clinical Trial"]);
    }

    #[test]
    fn test_parse_ictrp_does_not_duplicate_clinical_trial_type() {
        let input = concat!(
            "TrialID,Public title,Scientific title,Study type,Date registration,Source Register\n",
            "NCT00000003,Public title,Scientific title,Clinical Trial,01/05/2026,ClinicalTrials.gov\n"
        );

        let citation = IctrpCsvParser::new().parse(input).unwrap().remove(0);
        assert_eq!(citation.citation_type, vec!["Clinical Trial"]);
    }
}
