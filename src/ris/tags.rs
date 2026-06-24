//! RIS format tags and their definitions.
//!
//! This module defines all the standard RIS tags used in bibliographic citations.
//! See: http://en.wikipedia.org/wiki/RIS_(file_format)

/// RIS format tags.
///
/// RIS (Research Information Systems) is a standardized tag format developed by
/// Research Information Systems for expressing bibliographic citations.
#[allow(unused)]
#[allow(clippy::upper_case_acronyms)]
#[non_exhaustive]
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum RisTag {
    /// TY - Type of reference
    Type,
    /// TI - Primary title
    Title,
    /// T1 - Primary title (alternative)
    TitleAlternative,
    /// AU - Author
    Author,
    /// A1 - Primary author
    AuthorPrimary,
    /// A2 - Secondary author (Editor, if any)
    AuthorSecondary,
    /// A3 - Tertiary author
    AuthorTertiary,
    /// A4 - Subsidiary author
    AuthorSubsidiary,
    /// JF - Journal/Periodical name: full format
    JournalFull,
    /// JO - Journal/Periodical name: full format (alternative)
    JournalFullAlternative,
    /// JA - Journal/Periodical name: standard abbreviation
    JournalAbbreviation,
    /// J2 - Alternate title (journal abbreviation alternative)
    JournalAbbreviationAlternative,
    /// T2 - Secondary title (journal title alternative)
    SecondaryTitle,
    /// PY - Publication year
    PublicationYear,
    /// Y1 - Primary date
    DatePrimary,
    /// Y2 - Access date
    DateAccess,
    /// VL - Volume number
    Volume,
    /// IS - Issue number
    Issue,
    /// SP - Start page
    StartPage,
    /// EP - End page
    EndPage,
    /// DO - DOI
    Doi,
    /// AN - Accession number
    AccessionNumber,
    /// ID - Reference ID
    ReferenceId,
    /// AB - Abstract
    Abstract,
    /// N2 - Abstract (alternative)
    AbstractAlternative,
    /// KW - Keywords
    Keywords,
    /// SN - ISSN/ISBN
    SerialNumber,
    /// L1 - Link to PDF
    LinkPdf,
    /// L2 - Link to full text
    LinkFullText,
    /// L3 - Related records
    LinkRelated,
    /// L4 - Images
    LinkImages,
    /// UR - Web/URL
    Url,
    /// LK - Website link
    Link,
    /// LA - Language
    Language,
    /// PB - Publisher
    Publisher,
    /// C2 - PMCID
    PmcId,
    /// M3 - Type of Work
    WorkType,
    /// ER - End of reference
    EndOfReference,
    /// Unknown tag
    Unknown(String),
}

impl RisTag {
    /// Convert a string tag to a RisTag enum.
    pub fn from_tag(tag: &str) -> Self {
        match tag {
            "TY" => RisTag::Type,
            "TI" => RisTag::Title,
            "T1" => RisTag::TitleAlternative,
            "AU" => RisTag::Author,
            "A1" => RisTag::AuthorPrimary,
            "A2" => RisTag::AuthorSecondary,
            "A3" => RisTag::AuthorTertiary,
            "A4" => RisTag::AuthorSubsidiary,
            "JF" => RisTag::JournalFull,
            "JO" => RisTag::JournalFullAlternative,
            "JA" => RisTag::JournalAbbreviation,
            "J2" => RisTag::JournalAbbreviationAlternative,
            "T2" => RisTag::SecondaryTitle,
            "PY" => RisTag::PublicationYear,
            "Y1" => RisTag::DatePrimary,
            "Y2" => RisTag::DateAccess,
            "VL" => RisTag::Volume,
            "IS" => RisTag::Issue,
            "SP" => RisTag::StartPage,
            "EP" => RisTag::EndPage,
            "DO" => RisTag::Doi,
            "AN" => RisTag::AccessionNumber,
            "ID" => RisTag::ReferenceId,
            "AB" => RisTag::Abstract,
            "N2" => RisTag::AbstractAlternative,
            "KW" => RisTag::Keywords,
            "SN" => RisTag::SerialNumber,
            "L1" => RisTag::LinkPdf,
            "L2" => RisTag::LinkFullText,
            "L3" => RisTag::LinkRelated,
            "L4" => RisTag::LinkImages,
            "UR" => RisTag::Url,
            "LK" => RisTag::Link,
            "LA" => RisTag::Language,
            "PB" => RisTag::Publisher,
            "C2" => RisTag::PmcId,
            "M3" => RisTag::WorkType,
            "ER" => RisTag::EndOfReference,
            _ => RisTag::Unknown(tag.to_string()),
        }
    }

    /// Convert a RisTag enum back to its string representation.
    pub fn as_tag(&self) -> &str {
        match self {
            RisTag::Type => "TY",
            RisTag::Title => "TI",
            RisTag::TitleAlternative => "T1",
            RisTag::Author => "AU",
            RisTag::AuthorPrimary => "A1",
            RisTag::AuthorSecondary => "A2",
            RisTag::AuthorTertiary => "A3",
            RisTag::AuthorSubsidiary => "A4",
            RisTag::JournalFull => "JF",
            RisTag::JournalFullAlternative => "JO",
            RisTag::JournalAbbreviation => "JA",
            RisTag::JournalAbbreviationAlternative => "J2",
            RisTag::SecondaryTitle => "T2",
            RisTag::PublicationYear => "PY",
            RisTag::DatePrimary => "Y1",
            RisTag::DateAccess => "Y2",
            RisTag::Volume => "VL",
            RisTag::Issue => "IS",
            RisTag::StartPage => "SP",
            RisTag::EndPage => "EP",
            RisTag::Doi => "DO",
            RisTag::AccessionNumber => "AN",
            RisTag::ReferenceId => "ID",
            RisTag::Abstract => "AB",
            RisTag::AbstractAlternative => "N2",
            RisTag::Keywords => "KW",
            RisTag::SerialNumber => "SN",
            RisTag::LinkPdf => "L1",
            RisTag::LinkFullText => "L2",
            RisTag::LinkRelated => "L3",
            RisTag::LinkImages => "L4",
            RisTag::Url => "UR",
            RisTag::Link => "LK",
            RisTag::Language => "LA",
            RisTag::Publisher => "PB",
            RisTag::PmcId => "C2",
            RisTag::WorkType => "M3",
            RisTag::EndOfReference => "ER",
            RisTag::Unknown(tag) => tag,
        }
    }

    /// Check if this tag represents an author field.
    pub fn is_author_tag(&self) -> bool {
        matches!(
            self,
            RisTag::Author
                | RisTag::AuthorPrimary
                | RisTag::AuthorSecondary
                | RisTag::AuthorTertiary
                | RisTag::AuthorSubsidiary
        )
    }

    /// Get the priority of this tag for journal name selection.
    /// Lower numbers have higher priority.
    ///
    /// Priority order:
    /// 1. JF (Journal Full) - primary full journal name
    /// 2. T2 (Secondary Title) - alternative journal title
    /// 3. JO (Journal Full Alternative) - alternative full name
    pub fn journal_priority(&self) -> Option<u8> {
        match self {
            RisTag::JournalFull => Some(1),
            RisTag::SecondaryTitle => Some(2),
            RisTag::JournalFullAlternative => Some(3),
            _ => None,
        }
    }

    /// Get the priority of this tag for journal abbreviation selection.
    /// Lower numbers have higher priority.
    ///
    /// Priority order:
    /// 1. JA (Journal Abbreviation) - primary standard abbreviation
    /// 2. J2 (Journal Abbreviation Alternative) - alternative abbreviation
    pub fn journal_abbr_priority(&self) -> Option<u8> {
        match self {
            RisTag::JournalAbbreviation => Some(1),
            RisTag::JournalAbbreviationAlternative => Some(2),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[rstest]
    #[case("TY", RisTag::Type)]
    #[case("TI", RisTag::Title)]
    #[case("AU", RisTag::Author)]
    #[case("JF", RisTag::JournalFull)]
    #[case("AN", RisTag::AccessionNumber)]
    #[case("ER", RisTag::EndOfReference)]
    #[case("UNKNOWN", RisTag::Unknown("UNKNOWN".to_string()))]
    fn test_from_tag(#[case] input: &str, #[case] expected: RisTag) {
        assert_eq!(RisTag::from_tag(input), expected);
    }

    #[rstest]
    #[case(RisTag::Type, "TY")]
    #[case(RisTag::Title, "TI")]
    #[case(RisTag::Author, "AU")]
    #[case(RisTag::Unknown("TEST".to_string()), "TEST")]
    fn test_as_tag(#[case] input: RisTag, #[case] expected: &str) {
        assert_eq!(input.as_tag(), expected);
    }

    #[rstest]
    #[case(RisTag::Author, true)]
    #[case(RisTag::AuthorPrimary, true)]
    #[case(RisTag::Title, false)]
    fn test_is_author_tag(#[case] tag: RisTag, #[case] expected: bool) {
        assert_eq!(tag.is_author_tag(), expected);
    }
}
