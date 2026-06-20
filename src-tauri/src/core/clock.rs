/// Current UTC time as an RFC-3339 string (e.g. `2026-06-20T12:34:56Z`).
/// Falls back to the Unix epoch only if formatting somehow fails.
pub fn now_iso() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_iso_is_rfc3339() {
        let ts = now_iso();
        // RFC-3339 looks like 2026-06-16T12:34:56...Z — 4-digit year then '-', 'T' at index 10.
        assert_eq!(ts.as_bytes()[4], b'-', "expected YYYY- prefix, got {ts}");
        assert_eq!(
            ts.as_bytes()[10],
            b'T',
            "expected date/time 'T' separator, got {ts}"
        );
        assert!(
            !ts.starts_with("unixtime"),
            "should not be the old placeholder, got {ts}"
        );
    }
}
