use crate::Date;

pub(crate) const ICTRP_URL_FIELD_KEYS: &[&str] = &[
    "web address",
    "results url link",
    "results url protocol",
    "web_address",
    "results_url_link",
    "results_url_protocol",
];

pub(crate) fn is_ictrp_url_field(key: &str) -> bool {
    ICTRP_URL_FIELD_KEYS.contains(&key)
}

pub(crate) fn dedupe_urls(urls: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for url in urls {
        if !url.trim().is_empty() && !unique.contains(&url) {
            unique.push(url);
        }
    }
    unique
}

pub(crate) fn parse_ictrp_compact_date(value: &str) -> Option<Date> {
    let trimmed = value.trim();
    if trimmed.len() != 8 {
        return None;
    }

    // `get` rather than indexing: an 8-byte value can contain multi-byte
    // characters, and slicing inside one would panic.
    let year = trimmed.get(0..4)?.parse().ok()?;
    let month = trimmed.get(4..6)?.parse().ok()?;
    let day = trimmed.get(6..8)?.parse().ok()?;

    Some(Date {
        year,
        month: Some(month),
        day: Some(day),
    })
}

pub(crate) fn parse_ictrp_standard_date(value: &str) -> Option<Date> {
    parse_ictrp_slash_date(value).or_else(|| parse_ictrp_hyphen_date(value))
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

fn parse_ictrp_hyphen_date(value: &str) -> Option<Date> {
    let parts = value.trim().split('-').map(str::trim).collect::<Vec<_>>();

    if parts.len() != 3 || parts[0].len() != 4 {
        return None;
    }

    Some(Date {
        year: parts[0].parse().ok()?,
        month: Some(parts[1].parse().ok()?),
        day: Some(parts[2].parse().ok()?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ictrp_compact_date() {
        assert_eq!(
            parse_ictrp_compact_date("20260501"),
            Some(Date {
                year: 2026,
                month: Some(5),
                day: Some(1),
            })
        );
    }

    #[test]
    fn test_parse_ictrp_compact_date_multibyte_does_not_panic() {
        // 8 bytes, with byte 4 inside the 2-byte 'é'.
        assert_eq!(parse_ictrp_compact_date("202é050"), None);
        // 8 bytes, boundaries intact, but not digits.
        assert_eq!(parse_ictrp_compact_date("202605é"), None);
    }

    #[test]
    fn test_parse_ictrp_standard_slash_date() {
        assert_eq!(
            parse_ictrp_standard_date("01/05/2026"),
            Some(Date {
                year: 2026,
                month: Some(5),
                day: Some(1),
            })
        );
    }

    #[test]
    fn test_parse_ictrp_standard_hyphen_date() {
        assert_eq!(
            parse_ictrp_standard_date("2026-05-01"),
            Some(Date {
                year: 2026,
                month: Some(5),
                day: Some(1),
            })
        );
    }
}
