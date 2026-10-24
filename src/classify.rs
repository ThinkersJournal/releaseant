//! Classify what `cargo publish` / `cargo package` printed (DESIGN section 4).
//!
//! Every pattern here is anchored to a whole line and has a captured sample in
//! `tests/fixtures/` (see its README). Output that matches none of them is [`Class::Unknown`],
//! which stops a run: an unrecognised message is never assumed to be a rate limit or a success.

/// What the output means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// cargo reported `Published <crate> v<version> at registry ...` and exited 0.
    Published,
    /// The version is already on the registry.
    AlreadyPublished,
    /// A dependency with a version is not on the index (yet).
    DependencyMissing {
        /// The package cargo could not find.
        package: String,
    },
    /// crates.io answered 429. `retry_at_unix` is the server's "try again after" time (seconds since
    /// the Unix epoch, from the GMT date in the message); wait until then rather than a fixed time.
    RateLimited {
        /// When the server says to try again, if the message carried a parseable date.
        retry_at_unix: Option<i64>,
    },
    /// Anything else.
    Unknown,
}

/// Classify one cargo invocation from its exit status and combined output.
#[must_use]
pub fn classify(success: bool, output: &str) -> Class {
    let lines: Vec<&str> = output.lines().map(str::trim_end).collect();
    if success {
        if lines.iter().any(|l| is_published_line(l)) {
            return Class::Published;
        }
        return Class::Unknown; // exit 0 without cargo's own "Published" line: do not guess
    }
    if lines.iter().any(|l| is_already_exists_line(l)) {
        return Class::AlreadyPublished;
    }
    if lines.contains(&"error: failed to prepare local package for uploading")
        && let Some(p) = lines.iter().find_map(|l| missing_package(l))
    {
        return Class::DependencyMissing {
            package: p.to_owned(),
        };
    }
    if let Some(r) = lines.iter().find_map(|l| rate_limit(l)) {
        return Class::RateLimited { retry_at_unix: r };
    }
    Class::Unknown
}

/// `   Published NAME vVERSION at registry `crates-io``
fn is_published_line(l: &str) -> bool {
    let Some(rest) = l.trim_start().strip_prefix("Published ") else {
        return false;
    };
    let mut it = rest.split(' ');
    matches!(
        (it.next(), it.next(), it.next(), it.next()),
        (Some(n), Some(v), Some("at"), Some("registry")) if !n.is_empty() && v.starts_with('v')
    )
}

/// `error: crate NAME@VERSION already exists on crates.io index`
fn is_already_exists_line(l: &str) -> bool {
    let Some(rest) = l.strip_prefix("error: crate ") else {
        return false;
    };
    let Some(spec) = rest.strip_suffix(" already exists on crates.io index") else {
        return false;
    };
    !spec.contains(' ')
        && spec
            .split_once('@')
            .is_some_and(|(n, v)| !n.is_empty() && !v.is_empty())
}

const RATE_PREFIX: &str =
    "the remote server responded with an error (status 429 Too Many Requests): ";
const RATE_DATE_LEAD: &str = " Please try again after ";
const RATE_TAIL: &str = " GMT and see https://crates.io/docs/rate-limits for more details.";

/// `  the remote server responded with an error (status 429 Too Many Requests): <text> Please try
/// again after Sat, 24 Oct 2026 01:27:14 GMT and see https://crates.io/docs/rate-limits for more
/// details.` The middle text (which kind of limit) is not matched; the outer shape is. The inner
/// `Option` is `None` when the date does not parse.
fn rate_limit(l: &str) -> Option<Option<i64>> {
    let rest = l.trim_start().strip_prefix(RATE_PREFIX)?;
    let rest = rest.strip_suffix(RATE_TAIL)?;
    let (_, date) = rest.rsplit_once(RATE_DATE_LEAD)?;
    Some(parse_http_date(date))
}

/// `Sat, 24 Oct 2026 01:27:14` (the text before ` GMT`) to Unix seconds.
fn parse_http_date(s: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (_, rest) = s.split_once(", ")?;
    let mut it = rest.split(' ');
    let day: i64 = it.next()?.parse().ok()?;
    let name = it.next()?;
    let month = i64::try_from(MONTHS.iter().position(|m| *m == name)?).ok()? + 1;
    let year: i64 = it.next()?.parse().ok()?;
    let mut hms = it.next()?.split(':');
    let (h, m, sec): (i64, i64, i64) = (
        hms.next()?.parse().ok()?,
        hms.next()?.parse().ok()?,
        hms.next()?.parse().ok()?,
    );
    if it.next().is_some() || hms.next().is_some() {
        return None;
    }
    if !(1..=31).contains(&day) || h > 23 || m > 59 || sec > 60 || year < 1970 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + sec)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `  no matching package named `NAME` found`
fn missing_package(l: &str) -> Option<&str> {
    let rest = l.trim_start().strip_prefix("no matching package named `")?;
    rest.strip_suffix("` found")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLISHED: &str = include_str!("../tests/fixtures/published_ok.txt");
    const EXISTS: &str = include_str!("../tests/fixtures/already_exists.txt");
    const DEP_MISSING: &str = include_str!("../tests/fixtures/dep_missing.txt");

    // T9: each captured sample classifies as expected.
    #[test]
    fn captured_samples_classify() {
        assert_eq!(classify(true, PUBLISHED), Class::Published);
        assert_eq!(classify(false, EXISTS), Class::AlreadyPublished);
        match classify(false, DEP_MISSING) {
            Class::DependencyMissing { package } => assert!(package.starts_with("zzrlprobe")),
            other => panic!("{other:?}"),
        }
    }

    const RATE_LIMITED: &str = include_str!("../tests/fixtures/rate_limited.txt");

    // T9: the 429 sample (relayed by the PM, see the fixtures README) carries the retry-after time.
    #[test]
    fn rate_limit_sample_yields_the_server_advised_time() {
        // 2026-10-24 01:27:14 UTC; 1_792_805_234 was computed independently with Python's datetime.
        let expected = days_from_civil(2026, 10, 24) * 86_400 + 3600 + 27 * 60 + 14;
        assert_eq!(expected, 1_792_805_234);
        assert_eq!(
            classify(false, RATE_LIMITED),
            Class::RateLimited {
                retry_at_unix: Some(1_792_805_234)
            }
        );
        // The other kind of limit (new crates) has a different middle sentence but the same shape.
        let new_crates = RATE_LIMITED.replace("updates to existing crates", "new crates");
        assert!(matches!(
            classify(false, &new_crates),
            Class::RateLimited { .. }
        ));
        // A 429 whose date does not parse is still a rate limit, with no time.
        let no_date = RATE_LIMITED.replace("Sat, 24 Oct 2026 01:27:14", "soon");
        assert_eq!(
            classify(false, &no_date),
            Class::RateLimited {
                retry_at_unix: None
            }
        );
        assert_eq!(
            classify(true, RATE_LIMITED),
            Class::Unknown,
            "a zero exit is never a rate limit"
        );
    }

    #[test]
    fn date_parser_known_values() {
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00"), Some(0));
        assert_eq!(
            parse_http_date("Sat, 29 Feb 2020 12:00:00"),
            Some(1_582_977_600)
        );
        assert_eq!(
            parse_http_date("Fri, 31 Dec 1999 23:59:59"),
            Some(946_684_799)
        );
        for bad in [
            "",
            "Sat 24 Oct 2026 01:27:14",
            "Sat, 24 Foo 2026 01:27:14",
            "Sat, 24 Oct 2026 25:00:00",
            "Sat, 24 Oct 2026 01:27",
        ] {
            assert_eq!(parse_http_date(bad), None, "{bad}");
        }
    }

    // T9 negative controls: loose substrings must not classify.
    #[test]
    fn look_alike_text_is_unknown() {
        for text in [
            "error: failed to generate the separate rate table",
            "error: the remote server responded with an error (status 429 Too Many Requests)",
            "error: crate foo@1.0.0 already exists somewhere else",
            "note: this crate already exists on crates.io index",
            "Published is a word",
        ] {
            assert_eq!(classify(false, text), Class::Unknown, "{text}");
        }
    }

    #[test]
    fn exit_status_matters_and_exit_zero_without_published_line_is_unknown() {
        assert_eq!(
            classify(false, PUBLISHED),
            Class::Unknown,
            "a Published line on a failed exit"
        );
        assert_eq!(
            classify(true, EXISTS),
            Class::Unknown,
            "an error line on a zero exit"
        );
        assert_eq!(classify(true, "everything is fine\n"), Class::Unknown);
        for text in [
            "Published is a word",
            "Unpublished foo v1 at registry",
            "Published foo 1 at registry",
        ] {
            assert_eq!(classify(true, text), Class::Unknown, "{text}");
        }
    }
}
