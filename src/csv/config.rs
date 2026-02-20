//! CSV header mapping definitions and configuration.
//!
//! This module defines the default header mappings and configuration
//! structures for CSV parsing.

use std::collections::HashMap;

/// Default header mappings for common CSV column names
pub(crate) const DEFAULT_HEADERS: &[(&str, &[&str])] = &[
    ("title", &["title", "article title", "publication title"]),
    ("authors", &["author", "authors", "creator", "creators"]),
    (
        "journal",
        &["journal", "journal title", "source title", "publication"],
    ),
    ("year", &["year", "publication year", "pub year"]),
    ("volume", &["volume", "vol"]),
    ("issue", &["issue", "number", "no"]),
    ("pages", &["pages", "page numbers", "page range"]),
    ("doi", &["doi", "digital object identifier"]),
    ("abstract", &["abstract", "summary"]),
    ("keywords", &["keywords", "tags"]),
    ("issn", &["issn"]),
    ("language", &["language", "lang"]),
    ("publisher", &["publisher"]),
    ("url", &["url", "link", "web link"]),
    ("label", &["label"]),
    ("duplicate_id", &["duplicateid", "duplicate_id"]),
];

/// Configuration for CSV parsing with custom header mappings.
///
/// Allows customization of how CSV columns are mapped to citation fields,
/// along with general CSV parsing options like delimiters and header presence.
///
/// # Default Mappings
///
/// The default configuration includes mappings for common column names:
/// - "title" → ["title", "article title", "publication title"]
/// - "authors" → ["author", "authors", "creator", "creators"]
/// - "year" → ["year", "publication year", "pub year"]
///   etc.
///
/// # Examples
///
/// ```
/// use biblib::csv::CsvConfig;
///
/// let mut config = CsvConfig::new();
/// config.set_header_mapping("title", vec!["Article Name".to_string()]);
/// config.set_delimiter(b';');
/// ```
#[derive(Debug, Clone)]
pub struct CsvConfig {
    /// Custom header mappings for CSV columns
    pub(crate) header_map: HashMap<String, Vec<String>>,
    /// Reverse lookup map for O(1) header-to-field mapping
    pub(crate) reverse_map: HashMap<String, String>,
    /// Delimiter to use for parsing the CSV
    pub(crate) delimiter: u8,
    /// Whether the CSV has headers
    pub(crate) has_header: bool,
    /// Quote character
    pub(crate) quote: u8,
    /// Whether to trim whitespace
    pub(crate) trim: bool,
    /// Flexible parsing (ignore some errors)
    pub(crate) flexible: bool,
    /// Whether to store original record for debugging (memory optimization)
    pub(crate) store_original_record: bool,
}

impl Default for CsvConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvConfig {
    /// Creates a new CSV configuration with default settings
    #[must_use]
    pub fn new() -> Self {
        let mut config = Self {
            header_map: HashMap::new(),
            reverse_map: HashMap::new(),
            delimiter: b',',
            has_header: true,
            quote: b'"',
            trim: true,
            flexible: false,
            store_original_record: false,
        };
        config.set_default_headers();
        config
    }

    /// Sets the default header mappings
    fn set_default_headers(&mut self) {
        for (field, aliases) in DEFAULT_HEADERS {
            self.header_map.insert(
                field.to_string(),
                aliases.iter().map(|s| s.to_string()).collect(),
            );
        }
        self.rebuild_reverse_map();
    }

    /// Rebuild the reverse lookup map after header mappings change
    fn rebuild_reverse_map(&mut self) {
        self.reverse_map.clear();
        for (field, aliases) in &self.header_map {
            for alias in aliases {
                self.reverse_map.insert(alias.to_lowercase(), field.clone());
            }
        }
    }

    /// Sets a custom header mapping
    pub fn set_header_mapping(&mut self, field: &str, aliases: Vec<String>) -> &mut Self {
        self.header_map.insert(field.to_string(), aliases);
        self.rebuild_reverse_map();
        self
    }

    /// Adds additional aliases to an existing field mapping
    pub fn add_header_aliases(&mut self, field: &str, aliases: Vec<String>) -> &mut Self {
        self.header_map
            .entry(field.to_string())
            .or_default()
            .extend(aliases);
        self.rebuild_reverse_map();
        self
    }

    /// Sets the delimiter character
    pub fn set_delimiter(&mut self, delimiter: u8) -> &mut Self {
        self.delimiter = delimiter;
        self
    }

    /// Sets whether the CSV has headers
    pub fn set_has_header(&mut self, has_header: bool) -> &mut Self {
        self.has_header = has_header;
        self
    }

    /// Sets the quote character
    pub fn set_quote(&mut self, quote: u8) -> &mut Self {
        self.quote = quote;
        self
    }

    /// Sets whether to trim whitespace from fields
    pub fn set_trim(&mut self, trim: bool) -> &mut Self {
        self.trim = trim;
        self
    }

    /// Sets whether to use flexible parsing (ignore some errors)
    pub fn set_flexible(&mut self, flexible: bool) -> &mut Self {
        self.flexible = flexible;
        self
    }

    /// Sets whether to store original records for debugging (impacts memory usage)
    pub fn set_store_original_record(&mut self, store: bool) -> &mut Self {
        self.store_original_record = store;
        self
    }

    /// Finds the field name for a given header using O(1) lookup
    pub(crate) fn get_field_for_header(&self, header: &str) -> Option<&str> {
        let header_lower = header.to_lowercase();
        self.reverse_map.get(&header_lower).map(|s| s.as_str())
    }

    /// Gets all available field mappings
    pub fn get_field_mappings(&self) -> &HashMap<String, Vec<String>> {
        &self.header_map
    }

    /// Validates the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.header_map.is_empty() {
            return Err("No header mappings defined".to_string());
        }

        // Check for empty field names
        for (field, aliases) in &self.header_map {
            if field.is_empty() {
                return Err("Empty field name found in mappings".to_string());
            }
            if aliases.is_empty() {
                return Err(format!("Field '{}' has no aliases defined", field));
            }
            for alias in aliases {
                if alias.is_empty() {
                    return Err(format!("Empty alias found for field '{}'", field));
                }
            }
        }

        // Check for invalid delimiter characters
        if self.delimiter == b'\n' || self.delimiter == b'\r' {
            return Err("Delimiter cannot be a newline character".to_string());
        }

        // Check for duplicate aliases across different fields
        let mut all_aliases = HashMap::new();
        for (field, aliases) in &self.header_map {
            for alias in aliases {
                let alias_lower = alias.to_lowercase();
                if let Some(existing_field) = all_aliases.get(&alias_lower)
                    && existing_field != field
                {
                    return Err(format!(
                        "Alias '{}' is mapped to both '{}' and '{}'",
                        alias, existing_field, field
                    ));
                }
                all_aliases.insert(alias_lower, field.clone());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_new() {
        let config = CsvConfig::new();
        assert_eq!(config.delimiter, b',');
        assert!(config.has_header);
        assert!(!config.header_map.is_empty());
    }

    #[test]
    fn test_set_header_mapping() {
        let mut config = CsvConfig::new();
        config.set_header_mapping("title", vec!["my_title".to_string()]);

        assert_eq!(config.get_field_for_header("my_title"), Some("title"));
    }

    #[test]
    fn test_add_header_aliases() {
        let mut config = CsvConfig::new();
        config.add_header_aliases("title", vec!["article_name".to_string()]);

        // Should still recognize default aliases
        assert_eq!(config.get_field_for_header("title"), Some("title"));

        // Should also recognize new alias
        assert_eq!(config.get_field_for_header("article_name"), Some("title"));
    }

    #[test]
    fn test_get_field_for_header_case_insensitive() {
        let config = CsvConfig::new();

        assert_eq!(config.get_field_for_header("TITLE"), Some("title"));
        assert_eq!(config.get_field_for_header("Title"), Some("title"));
    }

    #[test]
    fn test_validate_success() {
        let config = CsvConfig::new();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_empty_mappings() {
        let mut config = CsvConfig::new();
        config.header_map.clear();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_duplicate_aliases() {
        let mut config = CsvConfig::new();
        config.set_header_mapping("field1", vec!["alias".to_string()]);
        config.set_header_mapping("field2", vec!["alias".to_string()]);

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_empty_field_name() {
        let mut config = CsvConfig::new();
        config.set_header_mapping("", vec!["alias".to_string()]);

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_empty_alias() {
        let mut config = CsvConfig::new();
        config.set_header_mapping("field", vec!["".to_string()]);

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_delimiter() {
        let mut config = CsvConfig::new();
        config.set_delimiter(b'\n');

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_configuration_chaining() {
        let mut config = CsvConfig::new();
        config
            .set_delimiter(b';')
            .set_has_header(false)
            .set_quote(b'\'')
            .set_trim(false)
            .set_flexible(true)
            .set_store_original_record(true);

        assert_eq!(config.delimiter, b';');
        assert!(!config.has_header);
        assert_eq!(config.quote, b'\'');
        assert!(!config.trim);
        assert!(config.flexible);
        assert!(config.store_original_record);
    }
}
