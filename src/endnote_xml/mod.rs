//! EndNote XML format parser implementation.
//!
//! Provides functionality to parse EndNote XML formatted citations with improved structure
//! and error handling. EndNote XML is an export format from EndNote reference management software.
//!
//! # Example
//!
//! ```
//! use biblib::{CitationParser, EndNoteXmlParser};
//!
//! let xml_content = r#"
//! <?xml version="1.0" encoding="UTF-8"?>
//! <xml>
//!   <records>
//!     <record>
//!       <ref-type name="Journal Article">17</ref-type>
//!       <contributors>
//!         <authors>
//!           <author>Doe, John</author>
//!           <author>Smith, Jane</author>
//!         </authors>
//!       </contributors>
//!       <titles>
//!         <title>Sample Research Article</title>
//!         <secondary-title>Journal of Science</secondary-title>
//!       </titles>
//!       <volume>15</volume>
//!       <number>3</number>
//!       <pages>123-135</pages>
//!       <year>2023</year>
//!       <electronic-resource-num>10.1234/example.doi</electronic-resource-num>
//!     </record>
//!   </records>
//! </xml>
//! "#;
//!
//! let parser = EndNoteXmlParser::new();
//! let citations = parser.parse(xml_content).unwrap();
//! assert_eq!(citations.len(), 1);
//!
//! let citation = &citations[0];
//! assert_eq!(citation.title, "Sample Research Article");
//! assert_eq!(citation.journal, Some("Journal of Science".to_string()));
//! assert_eq!(citation.authors.len(), 2);
//! assert_eq!(citation.authors[0].name, "Doe");
//! assert_eq!(citation.authors[0].given_name.as_deref(), Some("John"));
//! ```

mod parse;

use crate::error::ParseError;
use crate::{Citation, CitationParser};
use parse::parse_endnote_xml;

/// Parser for EndNote XML format citations.
///
/// EndNote XML is an export format from EndNote reference management software
/// that stores bibliographic data in a structured XML format.
#[derive(Debug, Clone, Default)]
pub struct EndNoteXmlParser;

impl EndNoteXmlParser {
    /// Creates a new EndNote XML parser instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use biblib::EndNoteXmlParser;
    /// let parser = EndNoteXmlParser::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CitationParser for EndNoteXmlParser {
    /// Parse EndNote XML content into citations.
    ///
    /// # Arguments
    ///
    /// * `input` - The EndNote XML content as a string
    ///
    /// # Returns
    ///
    /// A Result containing either a vector of citations or a parsing error.
    ///
    /// # Examples
    ///
    /// ```
    /// use biblib::{CitationParser, EndNoteXmlParser};
    ///
    /// let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
    /// <xml><records><record>
    /// <titles><title>Test Title</title></titles>
    /// </record></records></xml>"#;
    ///
    /// let parser = EndNoteXmlParser::new();
    /// let citations = parser.parse(xml).unwrap();
    /// assert_eq!(citations[0].title, "Test Title");
    /// ```
    fn parse(&self, input: &str) -> Result<Vec<Citation>, ParseError> {
        // Handle empty input by returning empty vector
        if input.trim().is_empty() {
            return Ok(Vec::new());
        }

        parse_endnote_xml(input)
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_complete_endnote_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xml>
  <records>
    <record>
      <ref-type name="Journal Article">17</ref-type>
      <contributors>
        <authors>
          <author>Smith, John A.</author>
          <author>Doe, Jane B.</author>
          <author>Brown, Robert C.</author>
        </authors>
      </contributors>
      <titles>
        <title>Advanced Methods in Computational Biology</title>
        <secondary-title>Journal of Computational Science</secondary-title>
        <alt-title>J Comput Sci</alt-title>
      </titles>
      <volume>25</volume>
      <number>4</number>
      <pages>123-145</pages>
      <year>2023</year>
      <electronic-resource-num>10.1016/j.jocs.2023.123456</electronic-resource-num>
      <url>https://www.sciencedirect.com/science/article/example</url>
      <abstract>This paper presents novel computational methods for analyzing biological data with improved accuracy and performance.</abstract>
      <keywords>
        <keyword>computational biology</keyword>
        <keyword>algorithms</keyword>
        <keyword>data analysis</keyword>
        <keyword>bioinformatics</keyword>
      </keywords>
      <language>English</language>
      <publisher>Elsevier</publisher>
      <isbn>1877-7503</isbn>
      <custom2>PMC9876543</custom2>
    </record>
    <record>
      <contributors>
        <authors>
          <author>Wilson, Emily</author>
        </authors>
      </contributors>
      <titles>
        <title>Machine Learning Applications in Healthcare</title>
        <secondary-title>Nature Medicine</secondary-title>
      </titles>
      <volume>29</volume>
      <number>2</number>
      <pages>78-92</pages>
      <year>2023</year>
      <electronic-resource-num>10.1038/s41591-023-02234-x</electronic-resource-num>
    </record>
  </records>
</xml>"#;

        let citations = parse_endnote_xml(xml).unwrap();
        assert_eq!(citations.len(), 2);

        // Test first citation
        let citation1 = &citations[0];
        assert_eq!(citation1.title, "Advanced Methods in Computational Biology");
        assert_eq!(
            citation1.journal,
            Some("Journal of Computational Science".to_string())
        );
        assert_eq!(citation1.authors.len(), 3);
        assert_eq!(citation1.authors[0].name, "Smith");
        assert_eq!(citation1.authors[0].given_name.as_deref(), Some("John"));
        assert_eq!(citation1.authors[0].middle_name.as_deref(), Some("A."));
        assert_eq!(citation1.volume, Some("25".to_string()));
        assert_eq!(citation1.issue, Some("4".to_string()));
        assert_eq!(citation1.pages, Some("123-145".to_string()));
        assert_eq!(citation1.date.as_ref().unwrap().year, 2023);
        assert!(citation1.doi.is_some());
        assert!(citation1.doi.as_ref().unwrap().contains("10.1016"));
        assert_eq!(
            citation1.urls,
            vec!["https://www.sciencedirect.com/science/article/example".to_string()]
        );
        assert!(citation1.abstract_text.is_some());
        assert_eq!(
            citation1.keywords,
            vec![
                "computational biology",
                "algorithms",
                "data analysis",
                "bioinformatics"
            ]
        );
        assert_eq!(citation1.language, Some("English".to_string()));
        assert_eq!(citation1.publisher, Some("Elsevier".to_string()));
        assert_eq!(citation1.issn, vec!["1877-7503".to_string()]); // From ISBN field
        assert_eq!(citation1.pmc_id, Some("PMC9876543".to_string()));

        // Test second citation
        let citation2 = &citations[1];
        assert_eq!(
            citation2.title,
            "Machine Learning Applications in Healthcare"
        );
        assert_eq!(citation2.journal, Some("Nature Medicine".to_string()));
        assert_eq!(citation2.authors.len(), 1);
        assert_eq!(citation2.authors[0].name, "Wilson");
        assert_eq!(citation2.authors[0].given_name.as_deref(), Some("Emily"));
        assert_eq!(citation2.volume, Some("29".to_string()));
        assert_eq!(citation2.issue, Some("2".to_string()));
        assert_eq!(citation2.pages, Some("78-92".to_string()));
        assert_eq!(citation2.date.as_ref().unwrap().year, 2023);
        assert!(citation2.doi.is_some());
        assert!(citation2.doi.as_ref().unwrap().contains("10.1038"));
    }

    #[test]
    fn test_minimal_endnote_xml() {
        let xml = r#"
        <xml>
          <records>
            <record>
              <titles>
                <title>Minimal Citation</title>
              </titles>
            </record>
          </records>
        </xml>
        "#;

        let citations = parse_endnote_xml(xml).unwrap();
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].title, "Minimal Citation");
    }

    #[test]
    fn test_author_only_citation() {
        let xml = r#"
        <xml>
          <records>
            <record>
              <contributors>
                <authors>
                  <author>Anonymous Author</author>
                </authors>
              </contributors>
            </record>
          </records>
        </xml>
        "#;

        let citations = parse_endnote_xml(xml).unwrap();
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].authors.len(), 1);
        assert_eq!(citations[0].authors[0].name, "Anonymous");
        assert_eq!(
            citations[0].authors[0].given_name.as_deref(),
            Some("Author")
        );
        assert_eq!(citations[0].title, ""); // Empty title since none provided
    }

    #[test]
    fn test_complex_nesting() {
        let xml = r#"
        <xml>
          <records>
            <record>
              <contributors>
                <authors>
                  <author>First Author</author>
                  <author>Second Author</author>
                </authors>
              </contributors>
              <titles>
                <title>Complex Nested Structure</title>
                <secondary-title>Test Journal</secondary-title>
              </titles>
              <dates>
                <year>2023</year>
              </dates>
              <keywords>
                <keyword>keyword1</keyword>
                <keyword>keyword2</keyword>
              </keywords>
            </record>
          </records>
        </xml>
        "#;

        let citations = parse_endnote_xml(xml).unwrap();
        assert_eq!(citations.len(), 1);

        let citation = &citations[0];
        assert_eq!(citation.title, "Complex Nested Structure");
        assert_eq!(citation.journal, Some("Test Journal".to_string()));
        assert_eq!(citation.authors.len(), 2);
        assert_eq!(citation.authors[0].name, "First");
        assert_eq!(citation.authors[0].given_name.as_deref(), Some("Author"));
        assert_eq!(citation.date.as_ref().unwrap().year, 2023);
        assert_eq!(citation.keywords, vec!["keyword1", "keyword2"]);
    }

    #[test]
    fn test_malformed_xml_error() {
        let xml = r#"
        <xml>
          <records>
            <record>
              <!-- This record has no content -->
            </record>
          </records>
        </xml>
        "#;

        let result = parse_endnote_xml(xml);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_xml() {
        let xml = r#"
        <xml>
          <records>
          </records>
        </xml>
        "#;

        let citations = parse_endnote_xml(xml).unwrap();
        assert_eq!(citations.len(), 0);
    }

    #[test]
    fn test_title_fallback_scenarios() {
        // Test when only alt-title exists
        let xml1 = r#"
        <xml>
          <records>
            <record>
              <titles>
                <alt-title>Only Alt Title</alt-title>
              </titles>
            </record>
          </records>
        </xml>
        "#;

        let citations1 = parse_endnote_xml(xml1).unwrap();
        assert_eq!(citations1[0].title, "Only Alt Title");

        // Test when secondary-title and alt-title exist (no primary title)
        let xml2 = r#"
        <xml>
          <records>
            <record>
              <titles>
                <secondary-title>Secondary as Title</secondary-title>
                <alt-title>Alt as Journal</alt-title>
              </titles>
            </record>
          </records>
        </xml>
        "#;

        let citations2 = parse_endnote_xml(xml2).unwrap();
        assert_eq!(citations2[0].title, "Secondary as Title");
        assert_eq!(citations2[0].journal, Some("Alt as Journal".to_string()));
    }

    #[test]
    fn test_line_number_tracking_in_errors() {
        // Test that we get accurate line numbers in error messages
        let xml_with_empty_record = r#"
<xml>
  <records>
    <record>
      <!-- This record has no title or author, it's on line 5 -->
    </record>
  </records>
</xml>
        "#;

        let result = parse_endnote_xml(xml_with_empty_record);
        assert!(result.is_err());

        if let Err(parse_error) = result {
            // Check that it's a ParseError with line tracking
            assert!(parse_error.line.is_some(), "Line number should be tracked");
            let line = parse_error.line.unwrap();
            assert!(line > 0, "Line number should be tracked and greater than 0");
            assert!(
                line <= 6,
                "Line number should be reasonable for this small XML, got line {}",
                line
            );
            // Check that the error message mentions missing title/author
            let error_msg = format!("{}", parse_error.error);
            assert!(
                error_msg.contains("title") || error_msg.contains("author"),
                "Error should mention missing title or author, got: {}",
                error_msg
            );
        } else {
            panic!("Expected ParseError");
        }

        // Test line number tracking with malformed XML text
        let xml_with_unclosed_tag = r#"
<?xml version="1.0" encoding="UTF-8"?>
<xml>
  <records>
    <record>
      <title>Unclosed Title
      <!-- Missing closing title tag -->
    </record>
  </records>
</xml>
        "#;

        let result2 = parse_endnote_xml(xml_with_unclosed_tag);
        assert!(result2.is_err());
        // This should produce an error with line number information
    }

    #[test]
    fn test_detailed_line_tracking() {
        // XML with specific content to test line tracking precision
        let xml = r#"<?xml version="1.0"?>
<xml>
  <records>
    <record>
      <!-- This record is intentionally empty -->
      <!-- and should fail at approximately line 5 -->
    </record>
    <record>
      <title>Valid Record</title>
    </record>
  </records>
</xml>"#;

        let result = parse_endnote_xml(xml);
        assert!(result.is_err());

        if let Err(parse_error) = result {
            let line = parse_error.line.unwrap_or(0);
            println!("Error at line {}: {}", line, parse_error.error);
            // The empty record starts around line 4, buffer position captured earlier
            assert!(
                line >= 3 && line <= 7,
                "Line number should be around line 3-7, got {}",
                line
            );
            let error_msg = format!("{}", parse_error.error);
            assert!(
                error_msg.contains("title") || error_msg.contains("author"),
                "Error should mention missing title or author, got: {}",
                error_msg
            );
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_empty_input() {
        let parser = EndNoteXmlParser::new();
        let result = parser.parse("").unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_whitespace_only_input() {
        let parser = EndNoteXmlParser::new();
        let result = parser.parse("   \n  \t  ").unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_no_citations_found() {
        let parser = EndNoteXmlParser::new();
        let xml = r#"
        <xml>
          <records>
          </records>
        </xml>
        "#;
        let result = parser.parse(xml).unwrap();
        assert_eq!(result.len(), 0);
    }
}
