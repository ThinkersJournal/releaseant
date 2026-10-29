//! The publish set and its order (DESIGN sections 0 and 2).
//!
//! Edge rule, measured against real `cargo package` (DESIGN section 0): A depends on B when A's
//! manifest names B **with a version**, in any position (normal, build, dev, optional). A dev-dependency
//! written with a path only (`req == "*"`) is stripped at package time and is not an edge.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use semver::{Version, VersionReq};

use crate::metadata::Metadata;

/// One crate to publish, with the crates that must be on the index first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCrate {
    /// Package name.
    pub name: String,
    /// Version that will be published.
    pub version: Version,
    /// Names of the set members this crate needs published first.
    pub depends_on: BTreeSet<String>,
}

/// The ordered publish plan: dependencies before dependents, ties broken by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Crates in publish order.
    pub order: Vec<PlannedCrate>,
}

impl Plan {
    /// A stable text identity of the plan (`name@version` in order), used to detect that the plan
    /// changed between runs at the same commit.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.order
            .iter()
            .map(|c| format!("{}@{}", c.name, c.version))
            .collect::<Vec<_>>()
            .join(";")
    }

    /// This plan without the named crates (a recorded exclusion). The order of the rest is
    /// unchanged and the excluded names disappear from every `depends_on`: a crate that needs an
    /// excluded one is the registry's business, see [`crate::externals::missing_externals`].
    #[must_use]
    pub fn without(&self, names: &BTreeSet<String>) -> Plan {
        Plan {
            order: self
                .order
                .iter()
                .filter(|c| !names.contains(&c.name))
                .map(|c| PlannedCrate {
                    depends_on: c.depends_on.difference(names).cloned().collect(),
                    ..c.clone()
                })
                .collect(),
        }
    }
}

/// Why a plan could not be made. Nothing is uploaded when planning fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// No publishable crate in the workspace.
    EmptySet,
    /// The dependency graph has a cycle; the members are listed in walk order.
    Cycle(Vec<String>),
    /// A requirement does not match the version this plan publishes (trap 3).
    VersionMismatch {
        /// The crate with the requirement.
        dependent: String,
        /// The crate it requires.
        dependency: String,
        /// The requirement as written.
        req: String,
        /// The version the plan will publish for the dependency.
        planned: Version,
    },
    /// A requirement that is not valid semver.
    BadRequirement {
        /// The crate with the requirement.
        dependent: String,
        /// The crate it requires.
        dependency: String,
        /// The text that did not parse.
        req: String,
    },
    /// A non-dev dependency on another member without a version: `cargo package` rejects it.
    Unversioned {
        /// The crate with the dependency.
        dependent: String,
        /// The crate it names.
        dependency: String,
    },
    /// A versioned dependency on a member that is not publishable to crates.io.
    DependsOnUnpublishable {
        /// The crate with the dependency.
        dependent: String,
        /// The unpublishable crate it names.
        dependency: String,
    },
    /// Two members share a name.
    DuplicateName(String),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySet => write!(f, "no publishable crate in the workspace"),
            Self::Cycle(members) => write!(f, "dependency cycle: {}", members.join(" -> ")),
            Self::VersionMismatch {
                dependent,
                dependency,
                req,
                planned,
            } => write!(
                f,
                "{dependent} requires {dependency} {req}, but this plan publishes {dependency} {planned}"
            ),
            Self::BadRequirement {
                dependent,
                dependency,
                req,
            } => write!(
                f,
                "{dependent} has an invalid requirement on {dependency}: {req:?}"
            ),
            Self::Unversioned {
                dependent,
                dependency,
            } => write!(f, "{dependent} depends on {dependency} without a version"),
            Self::DependsOnUnpublishable {
                dependent,
                dependency,
            } => write!(
                f,
                "{dependent} depends with a version on {dependency}, which is not publishable to crates.io"
            ),
            Self::DuplicateName(n) => write!(f, "two workspace members are named {n}"),
        }
    }
}

impl std::error::Error for PlanError {}

/// Compute the publish plan for every publishable member.
///
/// # Errors
/// See [`PlanError`]; any error means nothing may be uploaded.
pub fn plan(meta: &Metadata) -> Result<Plan, PlanError> {
    let mut all: BTreeMap<&str, bool> = BTreeMap::new();
    for p in &meta.packages {
        if all.insert(&p.name, p.is_publishable()).is_some() {
            return Err(PlanError::DuplicateName(p.name.clone()));
        }
    }
    let members: BTreeMap<&str, &crate::metadata::Package> = meta
        .packages
        .iter()
        .filter(|p| p.is_publishable())
        .map(|p| (p.name.as_str(), p))
        .collect();
    if members.is_empty() {
        return Err(PlanError::EmptySet);
    }

    let mut deps: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for (name, pkg) in &members {
        let mut set = BTreeSet::new();
        for d in &pkg.dependencies {
            let Some(&publishable) = all.get(d.name.as_str()) else {
                continue; // not a workspace member: the registry's business, not ours
            };
            if d.name == *name && d.is_dev() {
                continue; // a crate's dev-dependency on itself (feature enabling) is not an edge
            }
            if d.req == "*" {
                if d.is_dev() {
                    continue; // path-only dev-dependency: stripped at package time
                }
                return Err(PlanError::Unversioned {
                    dependent: (*name).to_owned(),
                    dependency: d.name.clone(),
                });
            }
            if !publishable {
                return Err(PlanError::DependsOnUnpublishable {
                    dependent: (*name).to_owned(),
                    dependency: d.name.clone(),
                });
            }
            let req = VersionReq::parse(&d.req).map_err(|_| PlanError::BadRequirement {
                dependent: (*name).to_owned(),
                dependency: d.name.clone(),
                req: d.req.clone(),
            })?;
            let planned = &members[d.name.as_str()].version;
            if !req.matches(planned) {
                return Err(PlanError::VersionMismatch {
                    dependent: (*name).to_owned(),
                    dependency: d.name.clone(),
                    req: d.req.clone(),
                    planned: planned.clone(),
                });
            }
            set.insert(d.name.clone());
        }
        deps.insert(name, set);
    }

    let order = topological(&members, &deps)?;
    Ok(Plan { order })
}

fn topological(
    members: &BTreeMap<&str, &crate::metadata::Package>,
    deps: &BTreeMap<&str, BTreeSet<String>>,
) -> Result<Vec<PlannedCrate>, PlanError> {
    let mut remaining: BTreeMap<&str, BTreeSet<&str>> = deps
        .iter()
        .map(|(n, ds)| (*n, ds.iter().map(String::as_str).collect()))
        .collect();
    let mut order = Vec::with_capacity(members.len());
    while let Some(next) = remaining
        .iter()
        .find(|(_, ds)| ds.is_empty())
        .map(|(n, _)| *n)
    {
        remaining.remove(next);
        for ds in remaining.values_mut() {
            ds.remove(next);
        }
        let pkg = members[next];
        order.push(PlannedCrate {
            name: next.to_owned(),
            version: pkg.version.clone(),
            depends_on: deps[next].clone(),
        });
    }
    if remaining.is_empty() {
        Ok(order)
    } else {
        Err(PlanError::Cycle(find_cycle(&remaining)))
    }
}

/// Every node left has at least one unmet dependency that is itself left, so walking the first such
/// dependency from the smallest name must revisit a node; the revisited stretch is a cycle.
fn find_cycle(remaining: &BTreeMap<&str, BTreeSet<&str>>) -> Vec<String> {
    let mut walk: Vec<&str> = Vec::new();
    let mut at = *remaining.keys().next().expect("caller checked non-empty");
    loop {
        if let Some(i) = walk.iter().position(|n| *n == at) {
            return walk[i..].iter().map(|n| (*n).to_owned()).collect();
        }
        walk.push(at);
        at = *remaining[at]
            .iter()
            .next()
            .expect("a node with unmet dependencies has one that is left");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{Dependency, Package};

    fn pkg(name: &str, version: &str, deps: Vec<Dependency>) -> Package {
        Package {
            name: name.into(),
            version: version.parse().unwrap(),
            publish: None,
            dependencies: deps,
        }
    }

    fn dep(name: &str, req: &str, kind: Option<&str>) -> Dependency {
        Dependency {
            name: name.into(),
            req: req.into(),
            kind: kind.map(Into::into),
            optional: false,
        }
    }

    fn names(p: &Plan) -> Vec<&str> {
        p.order.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn without_drops_the_named_crates_keeps_the_order_and_forgets_them_as_prerequisites() {
        let m = Metadata {
            packages: vec![
                pkg("a", "1.0.0", vec![]),
                pkg("b", "1.0.0", vec![dep("a", "^1", None)]),
                pkg(
                    "c",
                    "1.0.0",
                    vec![dep("a", "^1", None), dep("b", "^1", None)],
                ),
            ],
        };
        let full = plan(&m).unwrap();
        assert_eq!(names(&full), ["a", "b", "c"]);
        let cut = full.without(&BTreeSet::from(["b".to_owned()]));
        assert_eq!(names(&cut), ["a", "c"]);
        let c = cut.order.iter().find(|c| c.name == "c").unwrap();
        assert_eq!(c.depends_on, BTreeSet::from(["a".to_owned()]));
        // The plan changed, so a resume that expected the full plan is told so.
        assert_ne!(cut.fingerprint(), full.fingerprint());
        // Negative control: excluding nothing is the identity.
        assert_eq!(full.without(&BTreeSet::new()), full);
    }

    // T3: the edge rule, one fixture per dependency position.
    #[test]
    fn versioned_dev_dep_is_an_edge_the_baracuda_nvrtc_shape() {
        let m = Metadata {
            packages: vec![
                pkg("nvrtc", "1.0.0", vec![dep("zdriver", "^1.0", Some("dev"))]),
                pkg("zdriver", "1.0.0", vec![]),
            ],
        };
        // Normal deps alone would allow either order; the dev edge forces driver first.
        assert_eq!(names(&plan(&m).unwrap()), ["zdriver", "nvrtc"]);
    }

    #[test]
    fn path_only_dev_dep_is_not_an_edge() {
        let m = Metadata {
            packages: vec![
                pkg("a", "1.0.0", vec![dep("z", "*", Some("dev"))]),
                pkg("z", "1.0.0", vec![]),
            ],
        };
        // No edge, so name order wins: a before z. (With the edge it would be z, a.)
        assert_eq!(names(&plan(&m).unwrap()), ["a", "z"]);
    }

    #[test]
    fn optional_build_and_normal_deps_are_edges() {
        for (kind, optional) in [(None, false), (None, true), (Some("build"), false)] {
            let mut d = dep("z", "^1.0", kind);
            d.optional = optional;
            let m = Metadata {
                packages: vec![pkg("a", "1.0.0", vec![d]), pkg("z", "1.0.0", vec![])],
            };
            assert_eq!(names(&plan(&m).unwrap()), ["z", "a"], "{kind:?} {optional}");
        }
    }

    #[test]
    fn dev_dep_on_self_is_ignored_and_external_deps_are_not_edges() {
        let m = Metadata {
            packages: vec![pkg(
                "a",
                "1.0.0",
                vec![dep("a", "*", Some("dev")), dep("serde", "^1", None)],
            )],
        };
        assert_eq!(names(&plan(&m).unwrap()), ["a"]);
    }

    #[test]
    fn unversioned_normal_dep_and_unpublishable_target_are_errors() {
        let m = Metadata {
            packages: vec![
                pkg("a", "1.0.0", vec![dep("z", "*", None)]),
                pkg("z", "1.0.0", vec![]),
            ],
        };
        assert!(matches!(plan(&m), Err(PlanError::Unversioned { .. })));

        let mut z = pkg("z", "1.0.0", vec![]);
        z.publish = Some(vec![]);
        let m = Metadata {
            packages: vec![pkg("a", "1.0.0", vec![dep("z", "^1", Some("dev"))]), z],
        };
        assert!(matches!(
            plan(&m),
            Err(PlanError::DependsOnUnpublishable { .. })
        ));
    }

    // T4: cycles are errors that name their members; the publishable-cycle shape is not a cycle.
    #[test]
    fn cycles_of_two_and_three_are_named() {
        let two = Metadata {
            packages: vec![
                pkg("a", "1.0.0", vec![dep("b", "^1", None)]),
                pkg("b", "1.0.0", vec![dep("a", "^1", None)]),
            ],
        };
        assert_eq!(
            plan(&two),
            Err(PlanError::Cycle(vec!["a".into(), "b".into()]))
        );

        let three = Metadata {
            packages: vec![
                pkg("a", "1.0.0", vec![dep("b", "^1", None)]),
                pkg("b", "1.0.0", vec![dep("c", "^1", None)]),
                pkg("c", "1.0.0", vec![dep("a", "^1", None)]),
                pkg("ok", "1.0.0", vec![]),
            ],
        };
        assert_eq!(
            plan(&three),
            Err(PlanError::Cycle(vec!["a".into(), "b".into(), "c".into()]))
        );
    }

    #[test]
    fn types_and_types_derive_shape_is_not_a_cycle() {
        // baracuda-types depends on -derive; -derive dev-depends on -types by path only.
        let m = Metadata {
            packages: vec![
                pkg("types", "1.0.0", vec![dep("types-derive", "^1", None)]),
                pkg(
                    "types-derive",
                    "1.0.0",
                    vec![dep("types", "*", Some("dev"))],
                ),
            ],
        };
        assert_eq!(names(&plan(&m).unwrap()), ["types-derive", "types"]);
    }

    // T5: a caret on a prerelease does not match an older prerelease.
    #[test]
    fn prerelease_requirement_against_older_prerelease_fails() {
        let mk = |planned: &str| Metadata {
            packages: vec![
                pkg(
                    "a",
                    "0.0.1-alpha.152",
                    vec![dep("b", "^0.0.1-alpha.152", None)],
                ),
                pkg("b", planned, vec![]),
            ],
        };
        assert!(matches!(
            plan(&mk("0.0.1-alpha.147")),
            Err(PlanError::VersionMismatch { .. })
        ));
        assert!(plan(&mk("0.0.1-alpha.152")).is_ok());
        assert!(plan(&mk("0.0.1-alpha.153")).is_ok());
    }

    // T12: dependencies before dependents before the umbrella; input order does not matter.
    #[test]
    fn order_is_dependencies_first_and_independent_of_input_order() {
        let build = |rev: bool| {
            let mut packages = vec![
                pkg(
                    "umbrella",
                    "1.0.0",
                    vec![dep("mid", "^1", None), dep("leaf", "^1", None)],
                ),
                pkg("mid", "1.0.0", vec![dep("leaf", "^1", None)]),
                pkg("leaf", "1.0.0", vec![]),
                pkg("other", "1.0.0", vec![]),
            ];
            if rev {
                packages.reverse();
            }
            plan(&Metadata { packages }).unwrap()
        };
        let a = build(false);
        assert_eq!(names(&a), ["leaf", "mid", "other", "umbrella"]);
        assert_eq!(a, build(true));
    }

    #[test]
    fn publish_false_and_non_crates_io_allowlist_are_excluded_and_empty_set_errors() {
        let mut private = pkg("private", "1.0.0", vec![]);
        private.publish = Some(vec![]);
        let mut other = pkg("other", "1.0.0", vec![]);
        other.publish = Some(vec!["my-registry".into()]);
        let m = Metadata {
            packages: vec![private.clone(), other.clone(), pkg("pub", "1.0.0", vec![])],
        };
        assert_eq!(names(&plan(&m).unwrap()), ["pub"]);
        let m = Metadata {
            packages: vec![private, other],
        };
        assert_eq!(plan(&m), Err(PlanError::EmptySet));
    }

    #[test]
    fn duplicate_names_and_bad_requirements_are_errors() {
        let m = Metadata {
            packages: vec![pkg("a", "1.0.0", vec![]), pkg("a", "2.0.0", vec![])],
        };
        assert!(matches!(plan(&m), Err(PlanError::DuplicateName(_))));
        let m = Metadata {
            packages: vec![
                pkg("a", "1.0.0", vec![dep("b", "not a req", None)]),
                pkg("b", "1.0.0", vec![]),
            ],
        };
        assert!(matches!(plan(&m), Err(PlanError::BadRequirement { .. })));
    }
}
