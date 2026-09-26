pub(crate) mod common;
#[cfg(feature = "csv")]
pub(crate) mod csv;
#[cfg(feature = "xml")]
pub(crate) mod xml;

#[cfg(feature = "xml")]
pub(crate) use common::is_ictrp_url_field;
pub(crate) use common::{dedupe_urls, parse_ictrp_compact_date, parse_ictrp_standard_date};
#[cfg(feature = "csv")]
pub(crate) use csv::looks_like_ictrp_csv;
