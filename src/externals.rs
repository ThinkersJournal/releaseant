//! External prerequisites: dependencies outside the publish set must already be on the registry.
//!
//! `cargo package` resolves every versioned dependency (normal, build, optional, dev) against the
//! registry. For a dependency on a workspace member the plan orders it first; for any other crate
//! the registry must already hold a non-yanked version that satisfies the requirement, or the
//! dependent cannot be published yet. Found by running `plan` on baracuda: `baracuda-cuda-seam`
//! needs `fuel-kernel-seam 0.23.0`, which another project has to publish first, and the PM had
//! excluded the crate by hand.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use semver::VersionReq;

use crate::metadata::Metadata;
use crate::plan::Plan;
use crate::registry::Registry;

/// A dependency outside the set that the registry cannot satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingExternal {
    /// The crate in the plan that needs it.
    pub dependent: String,
    /// The external crate.
    pub dependency: String,
    /// The requirement as written.
    pub req: String,
    /// Why it is not satisfied.
    pub reason: String,
}

impl fmt::Display for MissingExternal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} needs {} {}: {}",
            self.dependent, self.dependency, self.req, self.reason
        )
    }
}

/// Why the check itself could not finish.
#[derive(Debug, PartialEq, Eq)]
pub enum ExternalsError {
    /// The registry could not be asked about this crate (never read as "missing").
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

impl fmt::Display for ExternalsError {
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

impl std::error::Error for ExternalsError {}

/// Every external dependency of every planned crate that the registry cannot satisfy.
///
/// A dependency counts when it names a package that is not a workspace member and carries a
/// version (`req != "*"`); a dev-dependency written with a path only is stripped at package time and
/// is skipped, as in [`crate::plan::plan`]. A version satisfies it when it is not yanked and
/// matches the requirement under cargo's prerelease rules.
///
/// # Errors
/// [`ExternalsError`] when the registry cannot be asked or a requirement does not parse.
pub fn missing_externals(
    meta: &Metadata,
    plan: &Plan,
    registry: &mut dyn Registry,
) -> Result<Vec<MissingExternal>, ExternalsError> {
    // The plan orders its own members. A publishable member that is not in the plan was excluded
    // by name, so it is as external as any other crate: the registry must already hold it.
    let members: BTreeSet<&str> = meta
        .packages
        .iter()
        .filter(|p| !p.is_publishable() || plan.order.iter().any(|c| c.name == p.name))
        .map(|p| p.name.as_str())
        .collect();
    let mut cache: BTreeMap<String, Vec<(semver::Version, bool)>> = BTreeMap::new();
    let mut missing = Vec::new();
    for planned in &plan.order {
        let Some(pkg) = meta.packages.iter().find(|p| p.name == planned.name) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        for d in &pkg.dependencies {
            if members.contains(d.name.as_str()) || d.req == "*" {
                continue;
            }
            if !seen.insert((d.name.clone(), d.req.clone())) {
                continue;
            }
            let req = VersionReq::parse(&d.req).map_err(|_| ExternalsError::BadRequirement {
                dependent: planned.name.clone(),
                dependency: d.name.clone(),
                req: d.req.clone(),
            })?;
            if !cache.contains_key(&d.name) {
                let v = registry
                    .versions(&d.name)
                    .map_err(|why| ExternalsError::Registry {
                        name: d.name.clone(),
                        why,
                    })?;
                cache.insert(d.name.clone(), v);
            }
            let versions = &cache[&d.name];
            if versions.iter().any(|(v, yanked)| !yanked && req.matches(v)) {
                continue;
            }
            let reason = if versions.is_empty() {
                "the registry has no such crate".to_owned()
            } else {
                let mut live: Vec<String> = versions
                    .iter()
                    .filter(|(_, y)| !y)
                    .map(|(v, _)| v.to_string())
                    .collect();
                live.sort();
                if live.is_empty() {
                    "every published version is yanked".to_owned()
                } else {
                    format!("no published version matches (have {})", live.join(", "))
                }
            };
            missing.push(MissingExternal {
                dependent: planned.name.clone(),
                dependency: d.name.clone(),
                req: d.req.clone(),
                reason,
            });
        }
    }
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{Dependency, Package};
    use crate::plan::plan;
    use crate::registry::fake::FakeRegistry;

    fn pkg(name: &str, deps: Vec<Dependency>) -> Package {
        Package::test(name, "1.0.0", deps)
    }

    fn dep(name: &str, req: &str, kind: Option<&str>) -> Dependency {
        Dependency {
            name: name.into(),
            req: req.into(),
            kind: kind.map(Into::into),
            optional: false,
        }
    }

    fn check(
        deps: Vec<Dependency>,
        reg: &mut FakeRegistry,
    ) -> Result<Vec<MissingExternal>, ExternalsError> {
        let meta = Metadata {
            packages: vec![pkg("app", deps)],
        };
        let p = plan(&meta).unwrap();
        missing_externals(&meta, &p, reg)
    }

    // The baracuda-cuda-seam case: an external crate that nobody has published yet.
    #[test]
    fn unpublished_external_is_reported_and_published_one_is_not() {
        let mut reg = FakeRegistry::new().with_published("serde", "1.0.200");
        let r = check(
            vec![
                dep("serde", "^1", None),
                dep("fuel-kernel-seam", "0.23.0", None),
            ],
            &mut reg,
        )
        .unwrap();
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].dependency, "fuel-kernel-seam");
        assert!(r[0].reason.contains("no such crate"));
    }

    #[test]
    fn version_that_does_not_match_and_yanked_only_are_reported_with_why() {
        let mut reg = FakeRegistry::new()
            .with_published("x", "0.22.0")
            .with_published("y", "1.0.0");
        reg.yanked.insert("y".into());
        let r = check(
            vec![dep("x", "^0.23", None), dep("y", "^1", None)],
            &mut reg,
        )
        .unwrap();
        assert_eq!(r.len(), 2);
        assert!(r[0].reason.contains("have 0.22.0"), "{}", r[0].reason);
        assert!(r[1].reason.contains("yanked"), "{}", r[1].reason);
    }

    #[test]
    fn prerelease_requirement_follows_cargo_rules() {
        let mut reg = FakeRegistry::new().with_published("p", "0.0.1-alpha.147");
        let r = check(vec![dep("p", "^0.0.1-alpha.152", None)], &mut reg).unwrap();
        assert_eq!(
            r.len(),
            1,
            "an older prerelease must not satisfy a newer one"
        );
        let mut reg = FakeRegistry::new().with_published("p", "0.0.1-alpha.153");
        assert!(
            check(vec![dep("p", "^0.0.1-alpha.152", None)], &mut reg)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn dev_and_build_externals_count_but_path_only_dev_and_members_do_not() {
        let mut reg = FakeRegistry::new();
        let r = check(
            vec![
                dep("criterion", "0.5", Some("dev")),
                dep("cc", "1", Some("build")),
                dep("local-only", "*", Some("dev")),
            ],
            &mut reg,
        )
        .unwrap();
        let names: Vec<_> = r.iter().map(|m| m.dependency.as_str()).collect();
        assert_eq!(names, ["criterion", "cc"]);
        // A workspace member is the plan's business, not an external.
        let meta = Metadata {
            packages: vec![pkg("app", vec![dep("lib", "^1", None)]), pkg("lib", vec![])],
        };
        let p = plan(&meta).unwrap();
        assert!(
            missing_externals(&meta, &p, &mut FakeRegistry::new())
                .unwrap()
                .is_empty()
        );
    }

    // T8 again: a registry that cannot be asked is an error, never "missing" and never "fine".
    // A recorded exclusion turns a member into an external: its dependents still need it on the registry.
    #[test]
    fn an_excluded_member_a_planned_crate_needs_must_be_on_the_registry() {
        let meta = Metadata {
            packages: vec![pkg("app", vec![dep("lib", "^1", None)]), pkg("lib", vec![])],
        };
        let cut = plan(&meta)
            .unwrap()
            .without(&std::collections::BTreeSet::from(["lib".to_owned()]));
        let r = missing_externals(&meta, &cut, &mut FakeRegistry::new()).unwrap();
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(
            (r[0].dependent.as_str(), r[0].dependency.as_str()),
            ("app", "lib")
        );
        let mut published = FakeRegistry::new().with_published("lib", "1.0.0");
        assert!(
            missing_externals(&meta, &cut, &mut published)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn registry_failure_is_an_error() {
        let mut reg = FakeRegistry::new();
        reg.versions_error.insert("serde".into(), "timeout".into());
        assert!(matches!(
            check(vec![dep("serde", "^1", None)], &mut reg),
            Err(ExternalsError::Registry { .. })
        ));
    }

    #[test]
    fn each_external_is_asked_about_once_however_many_crates_use_it() {
        struct Counting(usize);
        impl Registry for Counting {
            fn lookup(&mut self, _: &str, _: &semver::Version) -> crate::registry::Lookup {
                unreachable!()
            }
            fn upload(&mut self, _: &str, _: &semver::Version) -> crate::registry::Upload {
                unreachable!()
            }
            fn versions(&mut self, _: &str) -> Result<Vec<(semver::Version, bool)>, String> {
                self.0 += 1;
                Ok(vec![("1.0.0".parse().unwrap(), false)])
            }
        }
        let meta = Metadata {
            packages: vec![
                pkg("a", vec![dep("serde", "^1", None)]),
                pkg(
                    "b",
                    vec![dep("serde", "^1", None), dep("serde", "^1", Some("dev"))],
                ),
            ],
        };
        let p = plan(&meta).unwrap();
        let mut c = Counting(0);
        assert!(missing_externals(&meta, &p, &mut c).unwrap().is_empty());
        assert_eq!(c.0, 1);
    }
}
