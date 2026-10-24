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
