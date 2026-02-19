//! CSV format parsing implementation.
//!
//! This module handles the low-level parsing of CSV formatted text.

use crate::CitationFormat;
use crate::csv::config::CsvConfig;
use crate::csv::structure::RawCsvData;
use crate::error::{ParseError, ValueError};
use csv::ReaderBuilder;

/// Parse the content of a CSV formatted file, returning structured data.
pub fn csv_parse<S: AsRef<str>>(
    csv_text: S,
    config: &CsvConfig,
) -> Result<Vec<RawCsvData>, ParseError> {
    let text = csv_text.as_ref();

    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    // Validate configuration
    config.validate().map_err(|msg| {
        ParseError::without_position(
            CitationFormat::Csv,
            ValueError::Syntax(format!("Invalid CSV configuration: {}", msg)),
        )
    })?;

    let mut reader = ReaderBuilder::new()
        .delimiter(config.delimiter)
        .has_headers(config.has_header)
        .quote(config.quote)
        .trim(if config.trim {
            csv::Trim::All
        } else {
            csv::Trim::None
        })
        .flexible(config.flexible)
        .from_reader(text.as_bytes());

    let headers: Vec<String> = if config.has_header {
        reader
            .headers()
            .map_err(|e| {
                ParseError::without_position(
                    CitationFormat::Csv,
                    ValueError::Syntax(format!("Header parsing error: {}", e)),
                )
            })?
            .iter()
            .map(String::from)
            .collect()
    } else {
        // Use column numbers as headers if no headers present
        let first_record = reader.headers().map_err(|e| {
            ParseError::without_position(
                CitationFormat::Csv,
                ValueError::Syntax(format!("Failed to read first record: {}", e)),
            )
        })?;
        (0..first_record.len())
            .map(|i| format!("Column{}", i + 1))
            .collect()
    };

    if headers.is_empty() {
        return Err(ParseError::without_position(
            CitationFormat::Csv,
            ValueError::Syntax("No headers found in CSV".to_string()),
        ));
    }

    let mut raw_citations = Vec::new();
    let mut line_number = if config.has_header { 2 } else { 1 }; // Start counting from data lines

    for result in reader.records() {
        let record = result.map_err(|e| {
            // Extract position information from csv::Error if available
            if let Some(position) = e.position() {
                ParseError::at_line(
                    position.line() as usize,
                    CitationFormat::Csv,
                    ValueError::Syntax(format!("CSV parsing error: {}", e)),
                )
            } else {
                ParseError::at_line(
                    line_number,
                    CitationFormat::Csv,
                    ValueError::Syntax(format!("CSV parsing error: {}", e)),
                )
            }
        })?;

        if record.is_empty() {
            line_number += 1;
            continue;
        }

        let byte_offset = record.position().map(|p| p.byte() as usize).unwrap_or(0);

        let raw_citation = RawCsvData::from_record(&headers, &record, config, line_number, byte_offset)?;

        if raw_citation.has_content() {
            raw_citations.push(raw_citation);
        } else if !config.flexible {
            return Err(ParseError::at_line(
                line_number,
                CitationFormat::Csv,
                ValueError::Syntax("Record contains no meaningful content".to_string()),
            ));
        }

        line_number += 1;
    }

    if raw_citations.is_empty() {
        return Ok(Vec::new());
    }

    Ok(raw_citations)
}

/// Detect CSV delimiter by analyzing the content.
pub fn detect_csv_delimiter(content: &str) -> u8 {
    let delimiters = [b',', b';', b'\t', b'|'];
    let sample_lines: Vec<&str> = content.lines().take(5).collect();

    if sample_lines.is_empty() {
        return b','; // Default to comma
    }

    let mut best_delimiter = b',';
    let mut best_score = 0;

    for &delimiter in &delimiters {
        let mut score = 0;
        let mut consistent = true;
        let mut expected_fields = None;

        for line in &sample_lines {
            let field_count = line.split(delimiter as char).count();

            if let Some(expected) = expected_fields {
                if field_count != expected {
                    consistent = false;
                    break;
                }
            } else {
                expected_fields = Some(field_count);
            }

            score += field_count;
        }

        if consistent && score > best_score {
            best_score = score;
            best_delimiter = delimiter;
        }
    }

    best_delimiter
}

/// Detect if CSV has headers by analyzing the first few lines.
pub fn detect_csv_headers(content: &str, delimiter: u8) -> bool {
    let lines: Vec<&str> = content.lines().take(3).collect();

    if lines.len() < 2 {
        return true; // Assume headers if we can't analyze
    }

    let first_line_fields: Vec<&str> = lines[0].split(delimiter as char).collect();
    let second_line_fields: Vec<&str> = lines[1].split(delimiter as char).collect();

    // Check if first line contains typical header patterns
    for field in &first_line_fields {
        let field_lower = field.to_lowercase();
        if field_lower.contains("title")
            || field_lower.contains("author")
            || field_lower.contains("year")
            || field_lower.contains("journal")
            || field_lower.contains("doi")
            || field_lower.contains("volume")
            || field_lower.contains("issue")
            || field_lower.contains("page")
            || field_lower.contains("abstract")
            || field_lower.contains("keyword")
        {
            return true;
        }
    }

    // Improved heuristic: Check if first line looks more like headers than data
    // Headers typically contain more text and fewer pure numbers
    let first_line_text_ratio = first_line_fields
        .iter()
        .filter(|f| !f.trim().is_empty())
        .filter(|f| f.parse::<f64>().is_err() && f.len() > 3)
        .count() as f64
        / first_line_fields.len().max(1) as f64;

    let second_line_numeric_ratio = second_line_fields
        .iter()
        .filter(|f| !f.trim().is_empty())
        .filter(|f| f.parse::<f64>().is_ok() || f.len() <= 3)
        .count() as f64
        / second_line_fields.len().max(1) as f64;

    // If first line has more text-like fields and second line has more data-like fields,
    // likely has headers
    first_line_text_ratio > 0.5 && second_line_numeric_ratio > 0.3
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[test]
    fn test_csv_parse_basic() {
        let input = "Title,Author,Year\nTest Article,Smith J,2023";
        let config = CsvConfig::new();

        let result = csv_parse(input, &config).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].get_field("title"),
            Some(&"Test Article".to_string())
        );
        assert_eq!(result[0].authors.len(), 1);
    }

    #[test]
    fn test_csv_parse_no_headers() {
        let input = "Test Article,Smith J,2023";
        let mut config = CsvConfig::new();
        config.set_has_header(false);

        let result = csv_parse(input, &config).unwrap();
        assert_eq!(result.len(), 1);
        // With no headers, fields are stored by column names
        assert!(result[0].get_field("Column1").is_some());
    }

    #[test]
    fn test_csv_parse_custom_delimiter() {
        let input = "Title;Author;Year\nTest Article;Smith J;2023";
        let mut config = CsvConfig::new();
        config.set_delimiter(b';');

        let result = csv_parse(input, &config).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].get_field("title"),
            Some(&"Test Article".to_string())
        );
    }

    #[test]
    fn test_csv_parse_empty_input() {
        let config = CsvConfig::new();
        let result = csv_parse("", &config);
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_csv_parse_no_valid_citations() {
        let input = "Title,Author,Year\n,,\n  ,  ,  ";
        let config = CsvConfig::new();

        let result = csv_parse(input, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_csv_parse_flexible_mode() {
        let input = "Title,Author\nTest Article,Smith J,Extra Field";
        let mut config = CsvConfig::new();
        config.set_flexible(true);

        let result = csv_parse(input, &config).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_csv_parse_malformed_strict() {
        let input = "Title,Author\nTest Article,Smith J,Extra Field";
        let config = CsvConfig::new(); // flexible = false by default

        let result = csv_parse(input, &config);
        // Should fail due to extra field in strict mode
        assert!(result.is_err());
    }

    #[rstest]
    #[case("a,b,c\n1,2,3", b',')]
    #[case("a;b;c\n1;2;3", b';')]
    #[case("a\tb\tc\n1\t2\t3", b'\t')]
    #[case("a|b|c\n1|2|3", b'|')]
    #[case("a,b;c\n1,2;3", b',')] // Comma appears more consistently
    fn test_detect_csv_delimiter(#[case] input: &str, #[case] expected: u8) {
        assert_eq!(detect_csv_delimiter(input), expected);
    }

    #[rstest]
    #[case("Title,Author\nTest,Smith", true)]
    #[case("Test Article,Smith\nAnother,Jones", false)]
    #[case("title,author\nTest,Smith", true)]
    #[case("Year,Volume\n2023,10", true)]
    #[case("123,456\n789,012", false)]
    fn test_detect_csv_headers(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(detect_csv_headers(input, b','), expected);
    }

    #[test]
    fn test_csv_parse_with_quotes() {
        let input = r#"Title,Author,Year
"Test Article with, comma","Smith, John",2023"#;
        let config = CsvConfig::new();

        let result = csv_parse(input, &config).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].get_field("title"),
            Some(&"Test Article with, comma".to_string())
        );
    assert_eq!(result[0].authors[0].name, "Smith");
    }

    #[test]
    fn test_csv_parse_line_numbers_in_errors() {
        let input = "Title,Author\nTest Article"; // Missing field
        let config = CsvConfig::new();

        match csv_parse(input, &config) {
            Err(parse_err) if parse_err.line.is_some() => {
                assert_eq!(parse_err.line.unwrap(), 2); // Second line (first data line)
                assert_eq!(parse_err.format, CitationFormat::Csv);
            }
            _ => panic!("Expected ParseError with line number"),
        }
    }
}
