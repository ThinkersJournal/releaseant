//! `--check-rust-version`: a crate that declares a `rust-version` must not depend on something that
//! needs a newer Rust (ROADMAP R6, "Later").
//!
//! A crate that says `rust-version = "1.70"` promises that it builds on 1.70. If a dependency it
//! cannot do without needs 1.80, the promise is false, and nothing says so until a user on 1.70 tries.
//! The check reads what the index records (`rust_version` of each published version) and, for
//! dependencies inside the set, the manifest in the commit:
//!
//! - **Error** (exit 4): no published, not-yanked version that satisfies the requirement fits the
//!   declared `rust-version`; or a dependency on a crate of the set that declares a newer one.
//! - **Note** (printed, does not refuse): the newest matching version needs a newer Rust but an older
//!   matching one fits, so only a resolver that respects `rust-version` finds the build; and any
//!   finding for a dependency limited to a target.
//!
//! Dev-dependencies and optional dependencies are skipped (a consumer need not build either). A
//! version that declares no `rust-version` is taken to fit: nothing is known against it. A crate
//! that declares none is skipped: it promises nothing (`--require-metadata rust-version` is the
//! lever for that).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use semver::{Version, VersionReq};

use crate::metadata::Metadata;
use crate::plan::Plan;
use crate::registry::{Registry, VersionInfo};

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Refuses the run.
    Error,
    /// Listed, does not refuse.
    Note,
}

/// One dependency that needs a newer Rust than its dependent declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The planned crate.
    pub dependent: String,
    /// What it depends on.
    pub dependency: String,
    /// The requirement as written.
    pub req: String,
    /// The `rust-version` the dependent declares.
    pub declared: String,
    /// How serious it is.
    pub severity: Severity,
    /// What is wrong, in words.
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match self.severity {
            Severity::Error => "RUST-VERSION",
            Severity::Note => "note",
        };
        write!(
            f,
            "{tag}: {} declares rust-version {} but {} {}: {}",
            self.dependent, self.declared, self.dependency, self.req, self.detail
        )
    }
}

/// Why the check itself could not finish.
#[derive(Debug, PartialEq, Eq)]
pub enum MsrvError {
    /// The registry could not be asked about this crate (never read as "fits").
    Registry {
        /// The crate being looked up.
        name: String,
        /// The failure text.
        why: String,
    },
    /// A requirement that is not valid semver.
    BadRequirement {
        /// The crate with the requirement.
        dependent: String,
        /// The crate it names.
        dependency: String,
        /// The text.
        req: String,
    },
}

impl fmt::Display for MsrvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry { name, why } => {
                write!(f, "could not ask the registry about {name}: {why}")
            }
            Self::BadRequirement {
                dependent,
                dependency,
                req,
            } => write!(
                f,
                "{dependent} has an invalid requirement on {dependency}: {req:?}"
            ),
        }
    }
}

impl std::error::Error for MsrvError {}

/// `1.70` or `1.70.1` as a version; `None` for anything else.
#[must_use]
pub fn parse_rust_version(text: &str) -> Option<Version> {
    let mut parts = text.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    parts
        .next()
        .is_none()
        .then(|| Version::new(major, minor, patch))
}

/// Every planned crate's dependencies that need a newer Rust than it declares.
///
/// # Errors
/// [`MsrvError`] when the registry cannot be asked or a requirement does not parse.
pub fn check_rust_versions(
    meta: &Metadata,
    plan: &Plan,
    registry: &mut dyn Registry,
) -> Result<Vec<Finding>, MsrvError> {
    let in_plan: BTreeMap<&str, Option<Version>> = plan
        .order
        .iter()
        .filter_map(|c| meta.packages.iter().find(|p| p.name == c.name))
        .map(|p| {
            (
                p.name.as_str(),
                p.rust_version.as_deref().and_then(parse_rust_version),
            )
        })
        .collect();
    let members: BTreeSet<&str> = meta
        .packages
        .iter()
        .filter(|p| !p.is_publishable() || in_plan.contains_key(p.name.as_str()))
        .map(|p| p.name.as_str())
        .collect();
    let mut cache: BTreeMap<String, Vec<VersionInfo>> = BTreeMap::new();
    let mut found = Vec::new();
    for planned in &plan.order {
        let Some(pkg) = meta.packages.iter().find(|p| p.name == planned.name) else {
            continue;
        };
        let Some(declared_text) = pkg.rust_version.as_deref() else {
            continue;
        };
        let Some(declared) = parse_rust_version(declared_text) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        for d in &pkg.dependencies {
            if d.is_dev()
                || d.optional
                || !seen.insert((d.name.clone(), d.req.clone(), d.target.clone()))
            {
                continue;
            }
            let severity = |s: Severity| {
                if d.target.is_some() {
                    Severity::Note
                } else {
                    s
                }
            };
            let finding = |s: Severity, detail: String| Finding {
                dependent: planned.name.clone(),
                dependency: d.name.clone(),
                req: d.req.clone(),
                declared: declared_text.to_owned(),
                severity: severity(s),
                detail,
            };
            if let Some(theirs) = in_plan.get(d.name.as_str()) {
                if let Some(theirs) = theirs.as_ref().filter(|t| **t > declared) {
                    found.push(finding(
                        Severity::Error,
                        format!(
                            "it is in this set and declares rust-version {}",
                            short(theirs)
                        ),
                    ));
                }
                continue;
            }
            if members.contains(d.name.as_str()) || d.req == "*" {
                continue;
            }
            let req = VersionReq::parse(&d.req).map_err(|_| MsrvError::BadRequirement {
                dependent: planned.name.clone(),
                dependency: d.name.clone(),
                req: d.req.clone(),
            })?;
            if !cache.contains_key(&d.name) {
                let infos = registry
                    .version_infos(&d.name)
                    .map_err(|why| MsrvError::Registry {
                        name: d.name.clone(),
                        why,
                    })?;
                cache.insert(d.name.clone(), infos);
            }
            let mut matching: Vec<&VersionInfo> = cache[&d.name]
                .iter()
                .filter(|i| !i.yanked && req.matches(&i.version))
                .collect();
            if matching.is_empty() {
                continue; // `--check-externals` reports what is missing; this check is about Rust.
            }
            matching.sort_by(|a, b| a.version.cmp(&b.version));
            let needs = |i: &VersionInfo| i.rust_version.as_deref().and_then(parse_rust_version);
            let fits = |i: &VersionInfo| needs(i).is_none_or(|n| n <= declared);
            if !matching.iter().any(|i| fits(i)) {
                let lowest = matching
                    .iter()
                    .filter_map(|i| needs(i).map(|n| (n, i)))
                    .min_by(|a, b| a.0.cmp(&b.0))
                    .expect("none fit, so every match declares one");
                found.push(finding(
                    Severity::Error,
                    format!(
                        "no published version that matches needs less than rust-version {} ({} {})",
                        short(&lowest.0),
                        d.name,
                        lowest.1.version
                    ),
                ));
            } else if let Some(newest) = matching.last().filter(|i| !fits(i)) {
                let fitting = matching.iter().rev().find(|i| fits(i)).expect("one fits");
                found.push(finding(
                    Severity::Note,
                    format!(
                        "the newest match {} needs rust-version {}; {} fits, found only by a resolver that respects rust-version",
                        newest.version,
                        short(&needs(newest).expect("does not fit, so it declares one")),
                        fitting.version
                    ),
                ));
            }
        }
    }
    Ok(found)
}

fn short(v: &Version) -> String {
    if v.patch == 0 {
        format!("{}.{}", v.major, v.minor)
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{Dependency, Package};
    use crate::plan::plan;
    use crate::registry::fake::FakeRegistry;

    fn dep(name: &str, req: &str) -> Dependency {
        Dependency {
            name: name.into(),
            req: req.into(),
            kind: None,
            optional: false,
            target: None,
        }
    }

    fn app(rust_version: Option<&str>, deps: Vec<Dependency>) -> Package {
        let mut p = Package::test("app", "1.0.0", deps);
        p.rust_version = rust_version.map(Into::into);
        p
    }

    fn reg(versions: &[(&str, &str, Option<&str>)]) -> FakeRegistry {
        let mut r = FakeRegistry::new();
        for (name, ver, rv) in versions {
            r.published
                .insert(((*name).into(), (*ver).into()), "ck".into());
            if let Some(rv) = rv {
                r.rust_versions
                    .insert(((*name).into(), (*ver).into()), (*rv).into());
            }
        }
        r
    }

    fn run(packages: Vec<Package>, r: &mut FakeRegistry) -> Result<Vec<Finding>, MsrvError> {
        let meta = Metadata { packages };
        let p = plan(&meta).unwrap();
        check_rust_versions(&meta, &p, r)
    }

    fn errors(f: &[Finding]) -> usize {
        f.iter().filter(|x| x.severity == Severity::Error).count()
    }

    #[test]
    fn parses_the_forms_cargo_accepts_and_nothing_else() {
        assert_eq!(parse_rust_version("1.70"), Some(Version::new(1, 70, 0)));
        assert_eq!(parse_rust_version("1.70.1"), Some(Version::new(1, 70, 1)));
        for bad in ["1", "1.x", "1.70.1.2", "", "^1.70"] {
            assert_eq!(parse_rust_version(bad), None, "{bad}");
        }
        assert!(
            parse_rust_version("1.9") < parse_rust_version("1.70"),
            "numeric, not text"
        );
    }

    #[test]
    fn a_dependency_that_needs_a_newer_rust_than_declared_is_an_error() {
        let mut r = reg(&[("dep", "1.2.0", Some("1.80"))]);
        let f = run(vec![app(Some("1.70"), vec![dep("dep", "^1")])], &mut r).unwrap();
        assert_eq!(errors(&f), 1, "{f:?}");
        assert!(
            f[0].detail.contains("1.80") && f[0].detail.contains("dep 1.2.0"),
            "{}",
            f[0]
        );
        assert!(
            f[0].to_string()
                .starts_with("RUST-VERSION: app declares rust-version 1.70"),
            "{}",
            f[0]
        );
    }

    // The controls: equal, lower, and not declared at all all pass.
    #[test]
    fn equal_lower_and_undeclared_versions_fit() {
        for rv in [Some("1.70"), Some("1.56"), None] {
            let mut r = reg(&[("dep", "1.2.0", rv)]);
            let f = run(vec![app(Some("1.70"), vec![dep("dep", "^1")])], &mut r).unwrap();
            assert!(f.is_empty(), "{rv:?}: {f:?}");
        }
    }

    #[test]
    fn a_crate_that_declares_no_rust_version_promises_nothing() {
        let mut r = reg(&[("dep", "1.2.0", Some("1.99"))]);
        assert!(
            run(vec![app(None, vec![dep("dep", "^1")])], &mut r)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_older_matching_version_that_fits_downgrades_it_to_a_note() {
        let mut r = reg(&[
            ("dep", "1.1.0", Some("1.60")),
            ("dep", "1.2.0", Some("1.80")),
        ]);
        let f = run(vec![app(Some("1.70"), vec![dep("dep", "^1")])], &mut r).unwrap();
        assert_eq!((f.len(), errors(&f)), (1, 0), "{f:?}");
        assert!(
            f[0].detail.contains("1.2.0") && f[0].detail.contains("1.1.0 fits"),
            "{}",
            f[0]
        );
        assert!(f[0].to_string().starts_with("note:"));
    }

    #[test]
    fn yanked_versions_and_versions_the_requirement_excludes_do_not_count() {
        // 1.0.0 would fail the check but is yanked; 2.0.0 would fail but `^1` excludes it; 1.5.0 fits.
        let mut r = reg(&[
            ("dep", "1.0.0", Some("1.99")),
            ("dep", "1.5.0", Some("1.60")),
            ("dep", "2.0.0", Some("1.99")),
        ]);
        r.yanked_versions.insert(("dep".into(), "1.0.0".into()));
        let f = run(vec![app(Some("1.70"), vec![dep("dep", "^1")])], &mut r).unwrap();
        assert!(f.is_empty(), "{f:?}");
        // The control: unyank 1.0.0 and 1.5.0 is no longer the newest to be ignored... 1.5.0 still fits,
        // so the newest match (1.5.0) fits and there is still nothing to say; make the only fit yanked.
        r.yanked_versions.insert(("dep".into(), "1.5.0".into()));
        r.yanked_versions.remove(&("dep".into(), "1.0.0".into()));
        let f = run(vec![app(Some("1.70"), vec![dep("dep", "^1")])], &mut r).unwrap();
        assert_eq!(errors(&f), 1, "the only live match needs 1.99: {f:?}");
    }

    #[test]
    fn dev_optional_and_target_limited_dependencies_are_treated_as_they_matter() {
        let mut r = reg(&[("dep", "1.2.0", Some("1.80"))]);
        let mut dev = dep("dep", "^1");
        dev.kind = Some("dev".into());
        let mut optional = dep("dep", "^1");
        optional.optional = true;
        let f = run(vec![app(Some("1.70"), vec![dev, optional])], &mut r).unwrap();
        assert!(f.is_empty(), "{f:?}");
        let mut windows = dep("dep", "^1");
        windows.target = Some("cfg(windows)".into());
        let f = run(vec![app(Some("1.70"), vec![windows])], &mut r).unwrap();
        assert_eq!(
            (f.len(), errors(&f)),
            (1, 0),
            "a target-limited one is only a note: {f:?}"
        );
    }

    #[test]
    fn a_member_of_the_set_that_declares_a_newer_rust_is_an_error() {
        let mut lib = Package::test("lib", "1.0.0", vec![]);
        lib.rust_version = Some("1.85".into());
        let mut app = app(Some("1.70"), vec![dep("lib", "^1")]);
        app.dependencies[0].req = "^1.0.0".into();
        let f = run(vec![lib, app], &mut reg(&[])).unwrap();
        assert_eq!(errors(&f), 1, "{f:?}");
        assert!(
            f[0].detail.contains("in this set") && f[0].detail.contains("1.85"),
            "{}",
            f[0]
        );
    }

    #[test]
    fn a_registry_that_cannot_be_asked_is_an_error_never_a_pass() {
        let mut r = reg(&[]);
        r.versions_error.insert("dep".into(), "HTTP 503".into());
        let err = run(vec![app(Some("1.70"), vec![dep("dep", "^1")])], &mut r).unwrap_err();
        assert!(
            matches!(&err, MsrvError::Registry { why, .. } if why.contains("503")),
            "{err}"
        );
        let err = run(
            vec![app(Some("1.70"), vec![dep("dep", "not a req")])],
            &mut reg(&[]),
        )
        .unwrap_err();
        assert!(matches!(err, MsrvError::BadRequirement { .. }), "{err}");
    }
}
