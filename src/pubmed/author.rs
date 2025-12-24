//! (Pedantically correct) order-dependent parsing logic of author information
//! from PubMed formatted `.nbib` files.

use crate::pubmed::tags::PubmedTag;
use compact_str::CompactString;
use std::borrow::Cow;

/// Value of `AU` or `FAU` in a PubMed citation.
#[derive(PartialEq)]
pub(crate) struct AuthorName {
    /// Author name value
    name: String,
    /// Is `FAU`
    full: bool,
}

impl AuthorName {
    /// Create an [AuthorName] from an `AU` value.
    pub fn au(name: String) -> Self {
        AuthorName { name, full: false }
    }

    /// Create an [AuthorName] from a `FAU` value.
    pub fn fau(name: String) -> Self {
        AuthorName { name, full: true }
    }

    /// Get the author's last (family) name.
    pub fn last_name(&self) -> &str {
        let parts = if self.full {
            self.name.split_once(", ")
        } else {
            self.name.rsplit_once(' ')
        };
        if let Some((last_name, _)) = parts {
            last_name
        } else {
            &self.name
        }
    }

    /// Get the first initials of the author's (given) names.
    pub fn first_initials(&self) -> CompactString {
        if self.full {
            fau_initials(&self.name)
        } else {
            au_initials(&self.name)
        }
    }

    /// Get the author's given names as a space-separated string if possible,
    /// otherwise return their first initials.
    pub(crate) fn given_name(&self) -> Option<&str> {
        if self.full {
            self.name.split_once(", ").map(|(_, r)| r)
        } else {
            self.name.rsplit_once(' ').map(|(_, r)| r)
        }
    }

    // not used, consider deleting?
    /// Get the name as an `AU`.
    pub fn as_au(&self) -> Cow<'_, str> {
        if self.full {
            let initials = self.first_initials();
            if initials.is_empty() {
                Cow::Borrowed(self.last_name())
            } else {
                Cow::Owned(format!("{} {}", self.last_name(), initials))
            }
        } else {
            Cow::Borrowed(&self.name)
        }
    }

    /// Check whether an `AU` is equivalent to this name.
    ///
    /// `AU` may omit middle initials, for example the name "Francis Harry Compton Crick"
    /// can be represented by any of the following `AU` values: "Crick FH", "Crick FHC".
    pub fn au_equals(&self, au: &str) -> bool {
        let (last_name, initials) = au.rsplit_once(' ').unwrap_or((au, ""));
        self.last_name() == last_name && self.first_initials().starts_with(initials)
    }
}

impl std::fmt::Debug for AuthorName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.full { "FAU" } else { "AU" })?;
        f.write_str("(")?;
        f.write_str(&self.name)?;
        f.write_str(")")?;
        Ok(())
    }
}

/// Get the first two initials from an `AU` value.
fn au_initials(au: &str) -> CompactString {
    if let Some((_, r)) = au.rsplit_once(' ') {
        CompactString::new(r)
    } else {
        CompactString::const_new("")
    }
}

/// Get the first two initials from a `FAU` value.
fn fau_initials(fau: &str) -> CompactString {
    if let Some((_, r)) = fau.split_once(", ") {
        let chars = r.split(' ').map_while(|s| s.chars().next());
        CompactString::from_iter(chars)
    } else {
        CompactString::const_new("")
    }
}

/// PubMed format tags which must be parsed with consecutive context.
#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum ConsecutiveTag {
    /// AU - Author
    Author,
    /// FAU - Full author name
    FullAuthorName,
    /// AD - Affiliation
    Affiliation,
}

impl ConsecutiveTag {
    pub(crate) fn from_tag(tag: PubmedTag) -> Option<Self> {
        match tag {
            PubmedTag::Author => Some(ConsecutiveTag::Author),
            PubmedTag::Affiliation => Some(ConsecutiveTag::Affiliation),
            PubmedTag::FullAuthorName => Some(ConsecutiveTag::FullAuthorName),
            _ => None,
        }
    }
}

/// Details about an author from a PubMed formatted citation.
#[derive(Debug, PartialEq)]
pub(crate) struct PubmedAuthor {
    pub(crate) name: AuthorName,
    pub(crate) affiliations: Vec<String>,
}

impl PubmedAuthor {
    fn new(name: AuthorName) -> Self {
        Self {
            name,
            affiliations: Vec::with_capacity(1),
        }
    }

    fn from_au(au: String) -> Self {
        Self::new(AuthorName::au(au))
    }

    fn from_fau(au: String) -> Self {
        Self::new(AuthorName::fau(au))
    }
}

/// Resolve authors from an ordered list of author-related entries.
///
/// Any leading affiliation entries are unassociated with an author,
/// and they are returned in a separate [Vec].
pub(crate) fn resolve_authors(
    data: Vec<(ConsecutiveTag, String)>,
) -> (Vec<PubmedAuthor>, Vec<String>) {
    let mut authors: Vec<PubmedAuthor> = Vec::with_capacity(data.len() / 2 + 1);
    let mut unused_affiliations = Vec::new();
    for (tag, value) in data {
        match tag {
            ConsecutiveTag::Author => {
                // Add new author if AU is not the same as the previous FAU.
                let prev = authors.last().map(|a| &a.name);
                if !prev.is_some_and(|n| n.full && n.au_equals(&value)) {
                    authors.push(PubmedAuthor::from_au(value));
                }
            }
            ConsecutiveTag::FullAuthorName => {
                // FAU always indicates start of new author description
                authors.push(PubmedAuthor::from_fau(value));
            }
            ConsecutiveTag::Affiliation => {
                // add affiliation to most recently parsed author
                if let Some(author) = authors.last_mut() {
                    author.affiliations.push(value);
                } else {
                    unused_affiliations.push(value);
                }
            }
        }
    }
    (authors, unused_affiliations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::*;

    #[rstest]
    #[case("", "", "", "", None)]
    #[case("Archimedes", "Archimedes", "Archimedes", "", None)]
    #[case("Einstein A", "Einstein, Albert", "Einstein", "A", Some("Albert"))]
    #[case("Newton I", "Newton, Issac", "Newton", "I", Some("Issac"))]
    #[case("Watson JD", "Watson, James D", "Watson", "JD", Some("James D"))]
    #[case(
        "Watson JD",
        "Watson, James Dewey",
        "Watson",
        "JD",
        Some("James Dewey")
    )]
    #[case("Crick FH", "Crick, Francis H", "Crick", "FH", Some("Francis H"))]
    #[case(
        "Crick FHC",
        "Crick, Francis Harry Compton",
        "Crick",
        "FHC",
        Some("Francis Harry Compton")
    )]
    // Complicated name from https://pubmed.ncbi.nlm.nih.gov/27206507/
    #[case(
        "van der Valk JPM",
        "van der Valk, J P M",
        "van der Valk",
        "JPM",
        Some("J P M")
    )]
    fn test_author_name(
        #[case] au: &str,
        #[case] fau: &str,
        #[case] last_name: &str,
        #[case] initials: &str,
        #[case] given_name: Option<&str>,
    ) {
        let full = AuthorName::fau(fau.to_string());
        assert_eq!(full.last_name(), last_name);
        assert_eq!(full.first_initials(), initials);
        assert_eq!(full.as_au(), au);
        assert_eq!(full.given_name(), given_name);

        let short = AuthorName::au(au.to_string());
        assert_eq!(short.last_name(), last_name);
        assert_eq!(short.first_initials(), initials);
        assert_eq!(short.as_au(), au);
        if given_name.is_some() {
            assert_eq!(short.given_name(), Some(initials));
        }
    }

    #[rstest]
    // Two consecutive AU lines
    #[case(&["Watson JD", "Crick FH"])]
    // Two equivalent AU lines would appear consecutively. This is a rare case.
    // We interpret this as two different authors with the same name.
    #[case(&["Watson JD", "Watson JD"])]
    fn test_resolve_author_consecutive_au(#[case] names: &[&str]) {
        let data = names
            .into_iter()
            .map(|s| (ConsecutiveTag::Author, s.to_string()))
            .collect();
        let (authors, _) = resolve_authors(data);
        let actual: Vec<_> = authors.iter().map(|a| a.name.as_au()).collect::<Vec<_>>();
        assert_eq!(&actual, names);
    }

    #[rstest]
    fn test_resolve_author_typical() {
        // From https://pubmed.ncbi.nlm.nih.gov/28230838/
        let data = vec![
            (ConsecutiveTag::FullAuthorName, "Lerch, Jason P".to_string()),
            (ConsecutiveTag::Author, "Lerch JP".to_string()),
            (ConsecutiveTag::Affiliation, "Program in Neuroscience and Mental Health, The Hospital for Sick Children, Toronto, Canada.".to_string()),
            (ConsecutiveTag::Affiliation, "Department of Medical Biophysics, University of Toronto, Toronto, Canada.".to_string()),
            // -----
            (ConsecutiveTag::FullAuthorName, "van der Kouwe, André J W".to_string()),
            (ConsecutiveTag::Author, "van der Kouwe AJ".to_string()),
            (ConsecutiveTag::Affiliation, "Athinoula A. Martinos Center for Biomedical Research, Department of Radiology, Massachusetts General Hospital and Harvard Medical School, Charlestown, Massachusetts, USA.".to_string()),
            (ConsecutiveTag::Affiliation, "Department of Radiology, Massachusetts General Hospital and Harvard Medical School, Boston, Massachusetts, USA.".to_string()),
            // -----
            (ConsecutiveTag::FullAuthorName, "Fischl, Bruce".to_string()),
            (ConsecutiveTag::Author, "Fischl B".to_string()),
            (ConsecutiveTag::Affiliation, "Athinoula A. Martinos Center for Biomedical Research, Department of Radiology, Massachusetts General Hospital and Harvard Medical School, Charlestown, Massachusetts, USA.".to_string()),
            (ConsecutiveTag::Affiliation, "Department of Radiology, Massachusetts General Hospital and Harvard Medical School, Boston, Massachusetts, USA.".to_string()),
            (ConsecutiveTag::Affiliation, "Computer Science and Artificial Intelligence Laboratory, Massachusetts Institute of Technology, Cambridge, Massachusetts, USA.".to_string()),
        ];
        let (actual, leading_affiliations) = resolve_authors(data);
        assert!(leading_affiliations.is_empty());
        let expected = vec![
            PubmedAuthor {
                name: AuthorName::fau("Lerch, Jason P".to_string()),
                affiliations: vec![
                    "Program in Neuroscience and Mental Health, The Hospital for Sick Children, Toronto, Canada.".to_string(),
                    "Department of Medical Biophysics, University of Toronto, Toronto, Canada.".to_string()
                ],
            },
            PubmedAuthor {
                name: AuthorName::fau("van der Kouwe, André J W".to_string()),
                affiliations: vec![
                    "Athinoula A. Martinos Center for Biomedical Research, Department of Radiology, Massachusetts General Hospital and Harvard Medical School, Charlestown, Massachusetts, USA.".to_string(),
                    "Department of Radiology, Massachusetts General Hospital and Harvard Medical School, Boston, Massachusetts, USA.".to_string()
                ],
            },
            PubmedAuthor {
                name: AuthorName::fau("Fischl, Bruce".to_string()),
                affiliations: vec![
                    "Athinoula A. Martinos Center for Biomedical Research, Department of Radiology, Massachusetts General Hospital and Harvard Medical School, Charlestown, Massachusetts, USA.".to_string(),
                    "Department of Radiology, Massachusetts General Hospital and Harvard Medical School, Boston, Massachusetts, USA.".to_string(),
                    "Computer Science and Artificial Intelligence Laboratory, Massachusetts Institute of Technology, Cambridge, Massachusetts, USA.".to_string()
                ],
            },
        ];
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case(&[
        (ConsecutiveTag::FullAuthorName, "Bose, Satyendra N"),
        (ConsecutiveTag::Author, "Bose SN"),
        (ConsecutiveTag::FullAuthorName, "Einstein, Albert"),
        (ConsecutiveTag::Author, "Einstein A"),
    ])]
    #[case(&[
        (ConsecutiveTag::FullAuthorName, "Bose, Satyendra N"),
        (ConsecutiveTag::FullAuthorName, "Einstein, Albert"),
        (ConsecutiveTag::Author, "Einstein A"),
    ])]
    #[case(&[
        (ConsecutiveTag::Author, "Bose SN"),
        (ConsecutiveTag::Author, "Einstein A"),
    ])]
    fn test_resolve_author_deduplication(#[case] names: &[(ConsecutiveTag, &str)]) {
        let data = names
            .into_iter()
            .map(|(t, n)| (*t, n.to_string()))
            .collect();
        let (authors, _) = resolve_authors(data);
        let actual: Vec<_> = authors.iter().map(|a| a.name.as_au()).collect::<Vec<_>>();
        assert_eq!(&actual, &["Bose SN", "Einstein A"]);
    }

    #[rstest]
    fn test_resolve_author_leading_affiliations() {
        let data = vec![
            (
                ConsecutiveTag::Affiliation,
                "Lab of Unknown Stuff".to_string(),
            ),
            (
                ConsecutiveTag::Affiliation,
                "Mysterious Basement".to_string(),
            ),
            (
                ConsecutiveTag::FullAuthorName,
                "Einstein, Albert".to_string(),
            ),
            (ConsecutiveTag::Author, "Einstein A".to_string()),
            (
                ConsecutiveTag::Affiliation,
                "University of Zurich".to_string(),
            ),
            (
                ConsecutiveTag::Affiliation,
                "University of Bern".to_string(),
            ),
        ];
        let (authors, leading_affiliations) = resolve_authors(data);
        let expected = [
            "Lab of Unknown Stuff".to_string(),
            "Mysterious Basement".to_string(),
        ];
        assert_eq!(leading_affiliations, &expected);
        assert_eq!(authors.len(), 1);
        let author = &authors[0].name;
        assert_eq!(author.name, "Einstein, Albert");
        assert_eq!(author.full, true);
        let affiliations = &authors[0].affiliations;
        let expected = [
            "University of Zurich".to_string(),
            "University of Bern".to_string(),
        ];
        assert_eq!(affiliations, &expected)
    }
}
