use crate::Date;
use crate::regex::Regex;
use std::sync::LazyLock;

static DOI_URL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^https?://(?:dx\.)?doi\.org/(.+)$").unwrap());

static ISSN_SPLIT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{4}-\d{3}[\dX](?:\s*\([^)]+\))?").unwrap());

/// Formats page numbers consistently, handling partial end page numbers
///
/// # Arguments
///
/// * `page_str` - The page string to format
pub fn format_page_numbers(page_range: &str) -> String {
    // Handle non-hyphenated or empty input
    if !page_range.contains('-') {
        return page_range.to_string();
    }

    // Split the range into from and to parts
    let parts: Vec<&str> = page_range.split('-').collect();
    if parts.len() != 2 {
        return page_range.to_string();
    }

    let (from, to) = (parts[0], parts[1]);

    // Detect prefix (alphanumeric characters at the start)
    let (from_prefix, from_num) = split_prefix_and_number(from);
    let (to_prefix, to_num) = split_prefix_and_number(to);

    // Check if prefixes match or are empty
    if from_prefix != to_prefix && !from_prefix.is_empty() && !to_prefix.is_empty() {
        return page_range.to_string();
    }

    // If to part doesn't have a number, return original
    let to_num = match to_num {
        Some(num) => num,
        None => return page_range.to_string(),
    };

    // If from_num is None but to_num is Some, return original
    let from_num = match from_num {
        Some(num) => num,
        None => return page_range.to_string(),
    };

    // If to number is shorter, use from's prefix/digits
    let completed_to = if to_num.len() < from_num.len() {
        format!("{}{}", &from_num[..from_num.len() - to_num.len()], to_num)
    } else {
        to_num.to_string()
    };

    // If both numbers are the same after completion, return just one number
    if from_num == completed_to {
        return format!("{}{}", from_prefix, from_num);
    }

    // Reconstruct the page range
    format!(
        "{}{}-{}{}",
        from_prefix, from_num, from_prefix, completed_to
    )
}

/// Helper function to split a page number into prefix and numeric part
fn split_prefix_and_number(input: &str) -> (String, Option<String>) {
    // Find the first numeric character
    match input.find(|c: char| c.is_ascii_digit()) {
        Some(index) => {
            let prefix = input[..index].to_string();
            let number = input[index..].to_string();
            (prefix, Some(number))
        }
        None => {
            // If no numeric part, return the whole input as prefix
            (input.to_string(), None)
        }
    }
}

/// Formats a DOI string by removing URL prefixes and [doi] suffixes
///
/// # Arguments
///
/// * `doi_str` - The DOI string to format
pub fn format_doi(doi_str: &str) -> Option<String> {
    if doi_str.is_empty() {
        return None;
    }
    let doi = doi_str
        .trim()
        .trim_end_matches("[doi]")
        .trim()
        .replace(|c: char| c.is_whitespace(), "") // Remove all whitespace
        .to_lowercase();

    // Find the first occurrence of "10." which typically starts a DOI
    if let Some(pos) = doi.find("10.") {
        let doi = &doi[pos..];
        if let Some(captures) = DOI_URL_REGEX.captures(doi) {
            Some(captures[1].to_string())
        } else {
            Some(doi.to_string())
        }
    } else {
        None
    }
}

/// Splits a string containing multiple ISSNs into a vector of individual ISSNs
///
/// # Arguments
///
/// * `issns` - String containing one or more ISSNs, possibly separated by newlines
pub fn split_issns(issns: &str) -> Vec<String> {
    let normalized = issns
        .replace("\\r\\n", "\n")
        .replace("\\r", "\n")
        .replace("\\n", "\n");

    let mut result = Vec::new();
    for line in normalized.split('\n') {
        if line.trim().is_empty() {
            continue;
        }

        let matches: Vec<_> = ISSN_SPLIT_REGEX
            .find_iter(line)
            .map(|m| m.as_str().trim())
            .collect();

        if !matches.is_empty() {
            result.extend(matches.into_iter().map(String::from));
        }
    }
    result
}

/// Helper function to parse author names in various formats
pub fn parse_author_name(name: &str) -> (String, String) {
    // Handle formats like "Lastname, Firstname", "Lastname, FN", or "Lastname FN"
    let parts: Vec<&str> = if name.contains(',') {
        name.split(',').collect()
    } else {
        name.split_whitespace().collect()
    };

    match parts.len() {
        0 => (String::new(), String::new()),
        1 => (parts[0].trim().to_string(), String::new()),
        2 => {
            let family = parts[0].trim().to_string();
            let given = parts[1].trim().to_string();
            (family, given)
        }
        _ => {
            let family = parts[0].trim().to_string();
            let given = parts[1..].join(" ").trim().to_string();
            (family, given)
        }
    }
}

/// Split a full given name string into given name and middle name parts.
///
/// Returns a tuple of (given_name, middle_name), where each is Option<String>.
/// The first token becomes the given_name; remaining tokens (if any) are joined
/// with a single space and become the middle_name.
pub fn split_given_and_middle(full_given: &str) -> (Option<String>, Option<String>) {
    let trimmed = full_given.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    let mut parts = trimmed.split_whitespace();
    let first = parts.next().map(|s| s.to_string());
    let rest: Vec<&str> = parts.collect();
    let middle = if rest.is_empty() {
        None
    } else {
        Some(rest.join(" "))
    };
    (first, middle)
}

/// Parses PubMed format dates (e.g., "2020 Jun 9", "2023 May 30", "2023 Jan 3", "2023")
///
/// # Arguments
///
/// * `date_str` - The date string to parse
pub fn parse_pubmed_date(date_str: &str) -> Option<Date> {
    let date_str = date_str.trim();

    if date_str.is_empty() {
        return None;
    }

    // Split the date string into parts
    let parts: Vec<&str> = date_str.split_whitespace().collect();

    // First part should be year
    let year = if let Some(year_str) = parts.first() {
        year_str.parse::<i32>().ok()?
    } else {
        return None;
    };

    let mut month = None;
    let mut day = None;

    // Second part should be month (if present)
    if let Some(month_str) = parts.get(1) {
        month = parse_month_name(month_str);
    }

    // Third part should be day (if present)
    if let Some(day_str) = parts.get(2)
        && let Ok(parsed_day) = day_str.parse::<u8>()
            && (1..=31).contains(&parsed_day) {
                day = Some(parsed_day);
            }

    Some(Date { year, month, day })
}

/// Parses RIS format dates (e.g., "1999/12/25/Christmas edition", "2023/05/30", "2023")
///
/// # Arguments
///
/// * `date_str` - The date string to parse
pub fn parse_ris_date(date_str: &str) -> Option<Date> {
    let date_str = date_str.trim();

    if date_str.is_empty() {
        return None;
    }

    // Split by '/' and take first 3 parts (year/month/day)
    let parts: Vec<&str> = date_str.split('/').collect();

    // First part should be year
    let year = if let Some(year_str) = parts.first() {
        if !year_str.is_empty() {
            year_str.parse::<i32>().ok()?
        } else {
            return None;
        }
    } else {
        return None;
    };

    let mut month = None;
    let mut day = None;

    // Second part should be month (if present and not empty)
    if let Some(month_str) = parts.get(1)
        && !month_str.is_empty()
            && let Ok(parsed_month) = month_str.parse::<u8>()
                && (1..=12).contains(&parsed_month) {
                    month = Some(parsed_month);
                }

    // Third part should be day (if present and not empty)
    if let Some(day_str) = parts.get(2)
        && !day_str.is_empty()
            && let Ok(parsed_day) = day_str.parse::<u8>()
                && (1..=31).contains(&parsed_day) {
                    day = Some(parsed_day);
                }

    Some(Date { year, month, day })
}

/// Parses EndNote XML format dates from year attributes
///
/// # Arguments
///
/// * `year` - Year value
/// * `month` - Month value (optional)
/// * `day` - Day value (optional)
pub fn parse_endnote_date(year: Option<i32>, month: Option<u8>, day: Option<u8>) -> Option<Date> {
    let year = year?;
    Some(Date { year, month, day })
}

/// Parses a simple year string into a Date
///
/// # Arguments
///
/// * `year_str` - The year string to parse
pub fn parse_year_only(year_str: &str) -> Option<Date> {
    let year_str = year_str.trim();

    if year_str.is_empty() {
        return None;
    }

    // Handle cases like "2023/" or "2023//"
    let year_part = year_str.split('/').next().unwrap_or(year_str);

    let year = year_part.parse::<i32>().ok()?;

    Some(Date {
        year,
        month: None,
        day: None,
    })
}

/// Helper function to parse month names to month numbers
fn parse_month_name(month_str: &str) -> Option<u8> {
    match month_str.to_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

/// get the newline delimiter (e.g. CRLF for Windows, LF for Linux). of multi-line text.
pub(crate) fn newline_delimiter_of(text: &str) -> &'static str {
    // find the first '\n', then check whether the character before it is '\r'
    if text
        .find('\n')
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| text.get(i..i + 1))
        .is_some_and(|x| x == "\r")
    {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_page_numbers() {
        assert_eq!(format_page_numbers("1234-45"), "1234-1245");
        assert_eq!(format_page_numbers("1234"), "1234");
        assert_eq!(format_page_numbers("123-456"), "123-456");
        // assert_eq!(format_page_numbers("879-93.e13"), "879-893");
        // assert_eq!(format_page_numbers("879-93.s1"), "879-893");
        assert_eq!(format_page_numbers("e071674"), "e071674");
        assert_eq!(format_page_numbers("R575-82"), "R575-R582");
        assert_eq!(format_page_numbers("12-345"), "12-345"); // to is longer than from
        assert_eq!(format_page_numbers("5-10"), "5-10"); // single digit to double digit
        assert_eq!(format_page_numbers("A94-A95"), "A94-A95");
        assert_eq!(format_page_numbers("01-Apr"), "01-Apr");
        assert_eq!(format_page_numbers("iii613-iii614"), "iii613-iii614");
        assert_eq!(format_page_numbers("101-101"), "101");
    }

    #[test]
    fn test_format_doi() {
        let test_cases = vec![
            ("10.1000/test", Some("10.1000/test".to_string())),
            ("10.1000/test [doi]", Some("10.1000/test".to_string())),
            (
                "https://doi.org/10.1000/test",
                Some("10.1000/test".to_string()),
            ),
            (
                "http://dx.doi.org/10.1000/test",
                Some("10.1000/test".to_string()),
            ),
            (
                " https://doi.org/10.1000/test ",
                Some("10.1000/test".to_string()),
            ),
            ("doi:10.1000/test", Some("10.1000/test".to_string())),
            ("DOI:10.1000/test", Some("10.1000/test".to_string())),
            ("doi: 10.1000/test", Some("10.1000/test".to_string())),
            ("avn 10.1000/test", Some("10.1000/test".to_string())),
            ("dhs\r10.1000/test", Some("10.1000/test".to_string())),
            ("DOI: 10.1000/test", Some("10.1000/test".to_string())),
            ("DOI:10.1000/TEST", Some("10.1000/test".to_string())),
            ("DOI 10.1000/TEST", Some("10.1000/test".to_string())),
            ("DOI10.1000/TEST", Some("10.1000/test".to_string())),
            ("10.1000/TEST", Some("10.1000/test".to_string())),
            (
                "HTTPS://DOI.ORG/10.1000/TEST",
                Some("10.1000/test".to_string()),
            ),
            (
                "https://doi.org/10.1000/test [doi]",
                Some("10.1000/test".to_string()),
            ),
            ("", None),
            ("invalid", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(format_doi(input), expected);
        }
    }

    #[test]
    fn test_parse_author_name() {
        // Test standard format "LastName, FirstName"
        let (family, given) = parse_author_name("Smith, John");
        assert_eq!(family, "Smith");
        assert_eq!(given, "John");

        // Test format with initials "LastName, J.J."
        let (family, given) = parse_author_name("Duan, J.J.");
        assert_eq!(family, "Duan");
        assert_eq!(given, "J.J.");

        // Test format without comma "LastName FirstName"
        let (family, given) = parse_author_name("Smith John");
        assert_eq!(family, "Smith");
        assert_eq!(given, "John");

        // Test format with just initials "LastName JJ"
        let (family, given) = parse_author_name("Duan JJ");
        assert_eq!(family, "Duan");
        assert_eq!(given, "JJ");

        // Test single name
        let (family, given) = parse_author_name("Smith");
        assert_eq!(family, "Smith");
        assert_eq!(given, "");

        // Test hyphenated names
        let (family, given) = parse_author_name("Smith-Jones, John-Paul");
        assert_eq!(family, "Smith-Jones");
        assert_eq!(given, "John-Paul");

        // Test empty string
        let (family, given) = parse_author_name("");
        assert_eq!(family, "");
        assert_eq!(given, "");

        // Test with multiple spaces
        let (family, given) = parse_author_name("von  Neumann,    John");
        assert_eq!(family, "von  Neumann");
        assert_eq!(given, "John");
    }

    #[test]
    fn test_split_issns() {
        // Test single ISSN
        assert_eq!(split_issns("1234-5678"), vec!["1234-5678"]);

        assert_eq!(split_issns("1234-5678 (Print)"), vec!["1234-5678 (Print)"]);

        assert_eq!(
            split_issns("1234-5678 (Print) 5678-1234"),
            vec!["1234-5678 (Print)", "5678-1234"]
        );

        assert_eq!(
            split_issns("1234-5678 (Print) 1234-5678 (Linking)"),
            vec!["1234-5678 (Print)", "1234-5678 (Linking)"]
        );

        assert_eq!(
            split_issns("1234-5678 5678-1234 9876-5432"),
            vec!["1234-5678", "5678-1234", "9876-5432"]
        );

        // Test multiple ISSNs with various separators
        assert_eq!(
            split_issns("1234-5678\n5678-1234\n9876-5432"),
            vec!["1234-5678", "5678-1234", "9876-5432"]
        );

        // Test with escaped newlines
        assert_eq!(
            split_issns("1234-5678\\n5678-1234\\r\\n9876-5432"),
            vec!["1234-5678", "5678-1234", "9876-5432"]
        );

        // Test with extra whitespace and empty lines
        assert_eq!(
            split_issns("  1234-5678  \n\n  5678-1234  \n"),
            vec!["1234-5678", "5678-1234"]
        );

        // Test with ISSN types
        assert_eq!(
            split_issns("1234-5678 (Print)\n5678-1234 (Electronic)"),
            vec!["1234-5678 (Print)", "5678-1234 (Electronic)"]
        );

        // Test empty page_str
        assert_eq!(split_issns(""), Vec::<String>::new());
    }
    #[test]
    fn test_parse_pubmed_date() {
        // Test full date
        let date = parse_pubmed_date("2020 Jun 9").unwrap();
        assert_eq!(date.year, 2020);
        assert_eq!(date.month, Some(6));
        assert_eq!(date.day, Some(9));

        // Test year and month only
        let date = parse_pubmed_date("2023 May").unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, Some(5));
        assert_eq!(date.day, None);

        // Test year only
        let date = parse_pubmed_date("2023").unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, None);
        assert_eq!(date.day, None);

        // Test empty string
        let date = parse_pubmed_date("");
        assert!(date.is_none());
    }
    #[test]
    fn test_parse_ris_date() {
        // Test full date
        let date = parse_ris_date("1999/12/25/Christmas edition").unwrap();
        assert_eq!(date.year, 1999);
        assert_eq!(date.month, Some(12));
        assert_eq!(date.day, Some(25));

        // Test year and month only
        let date = parse_ris_date("2023/05").unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, Some(5));
        assert_eq!(date.day, None);

        // Test year only
        let date = parse_ris_date("2023").unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, None);
        assert_eq!(date.day, None);

        // Test with empty parts
        let date = parse_ris_date("2023//").unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, None);
        assert_eq!(date.day, None);

        // Test empty string
        let date = parse_ris_date("");
        assert!(date.is_none());
    }

    #[test]
    fn test_parse_endnote_date() {
        // Add tests for EndNote date parsing
        let test_cases = vec![
            (
                Some(2023),
                Some(5),
                Some(30),
                Some(Date {
                    year: 2023,
                    month: Some(5),
                    day: Some(30),
                }),
            ),
            (
                Some(2023),
                None,
                None,
                Some(Date {
                    year: 2023,
                    month: None,
                    day: None,
                }),
            ),
            (None, Some(12), Some(25), None),
        ];

        for (year, month, day, expected) in test_cases {
            assert_eq!(parse_endnote_date(year, month, day), expected);
        }
    }

    #[test]
    fn test_parse_year_only() {
        let date = parse_year_only("2023").unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, None);
        assert_eq!(date.day, None);

        let date = parse_year_only("2023/").unwrap();
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, None);
        assert_eq!(date.day, None);

        let date = parse_year_only(""); // Test empty string

        assert!(date.is_none());
    }

    #[test]
    fn test_parse_month_name() {
        assert_eq!(parse_month_name("Jan"), Some(1));
        assert_eq!(parse_month_name("january"), Some(1));
        assert_eq!(parse_month_name("Feb"), Some(2));
        assert_eq!(parse_month_name("december"), Some(12));
        assert_eq!(parse_month_name("invalid"), None);
    }

    #[test]
    fn test_newline_delimiter_of() {
        assert_eq!(newline_delimiter_of(""), "\n");
        assert_eq!(newline_delimiter_of("hello world"), "\n");
        assert_eq!(newline_delimiter_of("hello\nworld"), "\n");
        assert_eq!(newline_delimiter_of("\n"), "\n");
        assert_eq!(newline_delimiter_of("\nhello\nworld\n"), "\n");
        assert_eq!(newline_delimiter_of("hello\r\nworld"), "\r\n");
        assert_eq!(newline_delimiter_of("hello\r\nworld\r\n"), "\r\n");
    }
}
