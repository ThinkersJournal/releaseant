//! Which GitHub repository a release's file artifacts go to.
//!
//! PM ruling (2026-11-09): derive it from the `repository` field of the crates being released, allow
//! an explicit override, refuse with a named error when the sources disagree or the field is missing,
//! and never guess a repository for a release upload.

use std::collections::BTreeSet;
use std::fmt;

use crate::ci::slug_of;
use crate::metadata::Package;

/// Why no repository could be named.
#[derive(Debug, PartialEq, Eq)]
pub enum RepoError {
    /// No crate of the release declares a `repository`, and no override was given.
    Missing,
    /// A crate declares no `repository` while another does (or the override is the only source).
    MissingOn(Vec<String>),
    /// A declared `repository` is not a GitHub URL.
    NotGithub {
        /// The crate.
        krate: String,
        /// What it declares.
        url: String,
    },
    /// The crates name different repositories.
    Disagree(Vec<String>),
    /// The override differs from what the crates declare.
    OverrideDisagrees {
        /// The override.
        given: String,
        /// What the crates declare.
        declared: String,
    },
    /// The override is not `owner/name`.
    BadOverride(String),
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(
                f,
                "no crate of this release declares `repository` in Cargo.toml, so the GitHub repository for the artifacts is unknown; set it, or name it with --artifacts-repo owner/name"
            ),
            Self::MissingOn(v) => write!(
                f,
                "these crates declare no `repository` while others do: {}; the repository for the artifacts is not guessed",
                v.join(", ")
            ),
            Self::NotGithub { krate, url } => write!(
                f,
                "{krate}: `repository` {url:?} is not a GitHub URL; name the repository with --artifacts-repo owner/name"
            ),
            Self::Disagree(v) => write!(
                f,
                "the crates declare different repositories ({}); the repository for the artifacts is not guessed",
                v.join(", ")
            ),
            Self::OverrideDisagrees { given, declared } => write!(
                f,
                "--artifacts-repo {given} disagrees with the `repository` the crates declare ({declared})"
            ),
            Self::BadOverride(s) => {
                write!(f, "--artifacts-repo {s:?} is not of the form owner/name")
            }
        }
    }
}

impl std::error::Error for RepoError {}

fn is_slug(s: &str) -> bool {
    s.split_once('/').is_some_and(|(o, n)| {
        !o.is_empty() && !n.is_empty() && !n.contains('/') && !s.contains(char::is_whitespace)
    })
}

/// The `owner/name` the artifacts of a release of `packages` go to.
///
/// `packages` are the crates being released. With an override, it fills a missing `repository` but
/// must equal one that is declared.
///
/// # Errors
/// [`RepoError`], naming what is missing or in conflict.
pub fn resolve(packages: &[&Package], over: Option<&str>) -> Result<String, RepoError> {
    if let Some(o) = over
        && !is_slug(o)
    {
        return Err(RepoError::BadOverride(o.to_owned()));
    }
    let mut declared: BTreeSet<String> = BTreeSet::new();
    let mut without = Vec::new();
    for p in packages {
        match p.repository.as_deref().map(str::trim) {
            None | Some("") => without.push(p.name.clone()),
            Some(url) => match slug_of(url) {
                Some(s) => {
                    declared.insert(s.to_ascii_lowercase());
                }
                None => {
                    return Err(RepoError::NotGithub {
                        krate: p.name.clone(),
                        url: url.to_owned(),
                    });
                }
            },
        }
    }
    if declared.len() > 1 {
        return Err(RepoError::Disagree(declared.into_iter().collect()));
    }
    match (declared.into_iter().next(), over) {
        (Some(d), Some(o)) if !d.eq_ignore_ascii_case(o) => Err(RepoError::OverrideDisagrees {
            given: o.to_owned(),
            declared: d,
        }),
        (Some(_), _) if !without.is_empty() => Err(RepoError::MissingOn(without)),
        (Some(d), _) => Ok(d),
        (None, Some(o)) => Ok(o.to_owned()),
        (None, None) => Err(RepoError::Missing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, repo: Option<&str>) -> Package {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "dependencies": [],
            "repository": repo,
        }))
        .unwrap()
    }

    fn go(ps: &[Package], o: Option<&str>) -> Result<String, RepoError> {
        let refs: Vec<&Package> = ps.iter().collect();
        resolve(&refs, o)
    }

    #[test]
    fn derives_from_every_crate_agreeing() {
        let ps = [
            pkg("a", Some("https://github.com/Org/Thing")),
            pkg("b", Some("https://github.com/org/thing.git")),
        ];
        assert_eq!(go(&ps, None).unwrap(), "org/thing");
    }

    #[test]
    fn missing_field_refuses() {
        assert_eq!(go(&[pkg("a", None)], None), Err(RepoError::Missing));
        assert_eq!(go(&[], None), Err(RepoError::Missing));
    }

    #[test]
    fn partly_missing_refuses_even_with_a_matching_override() {
        let ps = [pkg("a", Some("https://github.com/o/n")), pkg("b", None)];
        assert_eq!(go(&ps, None), Err(RepoError::MissingOn(vec!["b".into()])));
        assert_eq!(
            go(&ps, Some("o/n")),
            Err(RepoError::MissingOn(vec!["b".into()]))
        );
    }

    #[test]
    fn crates_that_disagree_refuse() {
        let ps = [
            pkg("a", Some("https://github.com/o/one")),
            pkg("b", Some("https://github.com/o/two")),
        ];
        assert!(matches!(go(&ps, None), Err(RepoError::Disagree(_))));
        assert!(matches!(
            go(&ps, Some("o/one")),
            Err(RepoError::Disagree(_))
        ));
    }

    #[test]
    fn override_must_agree_with_a_declared_repository() {
        let ps = [pkg("a", Some("https://github.com/o/n"))];
        assert_eq!(go(&ps, Some("O/N")).unwrap(), "o/n");
        assert!(matches!(
            go(&ps, Some("o/other")),
            Err(RepoError::OverrideDisagrees { .. })
        ));
    }

    #[test]
    fn override_fills_a_field_nobody_declares() {
        assert_eq!(go(&[pkg("a", None)], Some("o/n")).unwrap(), "o/n");
    }

    #[test]
    fn non_github_url_and_bad_override_refuse() {
        assert!(matches!(
            go(&[pkg("a", Some("https://gitlab.com/o/n"))], None),
            Err(RepoError::NotGithub { .. })
        ));
        for bad in ["", "o", "o/", "/n", "o/n/x", "o /n"] {
            assert!(
                matches!(
                    go(&[pkg("a", None)], Some(bad)),
                    Err(RepoError::BadOverride(_))
                ),
                "{bad:?}"
            );
        }
    }
}
