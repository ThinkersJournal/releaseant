//! `plan --check-names`: a crate name that already belongs to someone else, found before any upload
//! (SURVEY Q5; the failure was `fuel-core` and `fuel-compression`, which crates.io refuses only at upload).
//!
//! For every crate of the plan whose planned version is not on the registry yet: if the name has no
//! versions at all it is free; if it has, the owners are asked, and the name is fine only if at least
//! one owner is one of **ours** (named by `--owner` or `[names] owners` in `releaseant.toml`). A name
//! that exists and is owned by nobody of ours is a collision. A lookup that fails is never read as
//! "free": it is reported as unknown and refuses just like a collision.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use semver::Version;
use serde::Deserialize;

use crate::registry::{Lookup, Registry};

/// An owner of a crate on the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    /// `user` or `team`.
    pub kind: String,
    /// The login (`ciresnave`) or, for a team, `github:org:team`.
    pub login: String,
}

/// Who owns a crate that exists.
pub trait OwnerSource {
    /// The owners of `name`, which the registry's index already showed to exist.
    ///
    /// # Errors
    /// The failure text when the registry could not be asked (never read as "no owners").
    fn owners(&mut self, name: &str) -> Result<Vec<Owner>, String>;
}

/// What was found for one crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameState {
    /// The planned version is already on the registry: nothing to check.
    Published,
    /// No crate of this name exists.
    Free,
    /// The name exists and one of its owners is ours.
    Ours,
    /// The name exists and none of its owners is ours.
    Taken(Vec<Owner>),
    /// The registry could not say.
    Unknown(String),
}

impl NameState {
    /// Whether this refuses the run.
    #[must_use]
    pub fn refuses(&self) -> bool {
        matches!(self, Self::Taken(_) | Self::Unknown(_))
    }
}

/// Check every `(name, version)` of the plan against `registry` and `owners`; `ours` are logins,
/// compared without regard to case.
pub fn check_names(
    registry: &mut dyn Registry,
    owners: &mut dyn OwnerSource,
    crates: &[(String, Version)],
    ours: &BTreeSet<String>,
) -> Vec<(String, NameState)> {
    let ours: BTreeSet<String> = ours.iter().map(|o| o.to_ascii_lowercase()).collect();
    crates
        .iter()
        .map(|(name, version)| {
            let state = match registry.lookup(name, version) {
                Lookup::Present { .. } => NameState::Published,
                // Absent, or present but yanked (Unknown): either way the name still has to be judged.
                Lookup::Absent | Lookup::Unknown(_) => judge(registry, owners, name, &ours),
            };
            (name.clone(), state)
        })
        .collect()
}

fn judge(
    registry: &mut dyn Registry,
    owners: &mut dyn OwnerSource,
    name: &str,
    ours: &BTreeSet<String>,
) -> NameState {
    match registry.versions(name) {
        Err(why) => NameState::Unknown(why),
        Ok(v) if v.is_empty() => NameState::Free,
        Ok(_) => match owners.owners(name) {
            Err(why) => NameState::Unknown(why),
            Ok(list)
                if list
                    .iter()
                    .any(|o| ours.contains(&o.login.to_ascii_lowercase())) =>
            {
                NameState::Ours
            }
            Ok(list) => NameState::Taken(list),
        },
    }
}

/// One line for a finding.
#[must_use]
pub fn describe(name: &str, state: &NameState) -> String {
    match state {
        NameState::Published => format!("{name}: this version is already on the registry"),
        NameState::Free => format!("{name}: free (no such crate)"),
        NameState::Ours => format!("{name}: exists, owned by us"),
        NameState::Taken(list) => {
            let who: Vec<String> = list
                .iter()
                .map(|o| format!("{} ({})", o.login, o.kind))
                .collect();
            format!(
                "{name}: NAME TAKEN, owned by {} and by none of ours",
                if who.is_empty() {
                    "nobody listed".to_owned()
                } else {
                    who.join(", ")
                }
            )
        }
        NameState::Unknown(why) => format!("{name}: UNKNOWN, could not check ({why})"),
    }
}

#[derive(Deserialize)]
struct OwnersBody {
    users: Vec<OwnerJson>,
}

#[derive(Deserialize)]
struct OwnerJson {
    login: String,
    #[serde(default)]
    kind: String,
}

/// Read the body of `GET /api/v1/crates/<name>/owners`.
///
/// # Errors
/// If the text is not that response.
pub fn parse_owners(text: &str) -> Result<Vec<Owner>, String> {
    let body: OwnersBody =
        serde_json::from_str(text).map_err(|e| format!("not an owners list: {e}"))?;
    Ok(body
        .users
        .into_iter()
        .map(|u| Owner {
            kind: u.kind,
            login: u.login,
        })
        .collect())
}

/// crates.io's web API. It asks for at most one request a second (its data-access policy), so this
/// waits between calls; only names that exist and are not yet published at the planned version cost one.
pub struct CratesIoOwners {
    agent: ureq::Agent,
    base: String,
    last: Option<Instant>,
}

impl CratesIoOwners {
    /// A client for `base` (use [`BASE`]).
    #[must_use]
    pub fn new(base: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .user_agent(concat!(
                "releaseant/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/ThinkersJournal/releaseant)"
            ))
            .build()
            .into();
        Self {
            agent,
            base: base.trim_end_matches('/').to_owned(),
            last: None,
        }
    }
}

/// The public web API.
pub const BASE: &str = "https://crates.io";

impl OwnerSource for CratesIoOwners {
    fn owners(&mut self, name: &str) -> Result<Vec<Owner>, String> {
        if let Some(t) = self.last {
            let since = t.elapsed();
            if since < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_secs(1) - since);
            }
        }
        self.last = Some(Instant::now());
        let url = format!("{}/api/v1/crates/{name}/owners", self.base);
        let mut resp = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        match resp.status().as_u16() {
            200 => {
                let body = resp
                    .body_mut()
                    .read_to_string()
                    .map_err(|e| format!("reading {url}: {e}"))?;
                parse_owners(&body)
            }
            s => Err(format!("GET {url}: HTTP {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::registry::fake::FakeRegistry;

    struct Owners(BTreeMap<String, Result<Vec<Owner>, String>>, Vec<String>);
    impl OwnerSource for Owners {
        fn owners(&mut self, name: &str) -> Result<Vec<Owner>, String> {
            self.1.push(name.to_owned());
            self.0
                .get(name)
                .cloned()
                .unwrap_or_else(|| Err("unscripted".into()))
        }
    }

    fn user(login: &str) -> Owner {
        Owner {
            kind: "user".into(),
            login: login.into(),
        }
    }

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    fn ours() -> BTreeSet<String> {
        ["ciresnave".to_owned()].into()
    }

    fn registry() -> FakeRegistry {
        let mut r = FakeRegistry::default();
        for (n, ver) in [
            ("mine", "0.1.0"),
            ("mine", "0.2.0"),
            ("taken", "1.0.0"),
            ("published", "0.3.0"),
        ] {
            r.published.insert((n.into(), ver.into()), "ck".into());
        }
        r
    }

    fn run(
        crates: &[(&str, &str)],
        owners: &mut Owners,
        ours: &BTreeSet<String>,
    ) -> Vec<(String, NameState)> {
        let crates: Vec<(String, Version)> = crates
            .iter()
            .map(|(n, ver)| ((*n).to_owned(), v(ver)))
            .collect();
        check_names(&mut registry(), owners, &crates, ours)
    }

    fn scripted() -> Owners {
        Owners(
            BTreeMap::from([
                ("mine".to_owned(), Ok(vec![user("CireSnave")])),
                (
                    "taken".to_owned(),
                    Ok(vec![
                        user("someone-else"),
                        Owner {
                            kind: "team".into(),
                            login: "github:org:team".into(),
                        },
                    ]),
                ),
            ]),
            Vec::new(),
        )
    }

    #[test]
    fn free_ours_taken_and_published_are_told_apart() {
        let mut o = scripted();
        let got = run(
            &[
                ("brand-new", "0.1.0"),
                ("mine", "0.3.0"),
                ("taken", "1.1.0"),
                ("published", "0.3.0"),
            ],
            &mut o,
            &ours(),
        );
        let states: Vec<&NameState> = got.iter().map(|(_, s)| s).collect();
        assert_eq!(states[0], &NameState::Free);
        assert_eq!(states[1], &NameState::Ours, "login match ignores case");
        assert!(
            matches!(states[2], NameState::Taken(l) if l.len() == 2),
            "{:?}",
            states[2]
        );
        assert_eq!(states[3], &NameState::Published);
        assert_eq!(got.iter().filter(|(_, s)| s.refuses()).count(), 1);
    }

    #[test]
    fn only_names_that_exist_and_are_not_published_cost_an_owners_call() {
        let mut o = scripted();
        run(
            &[
                ("brand-new", "0.1.0"),
                ("published", "0.3.0"),
                ("mine", "0.3.0"),
            ],
            &mut o,
            &ours(),
        );
        assert_eq!(o.1, vec!["mine".to_owned()]);
    }

    #[test]
    fn one_of_several_owners_being_ours_is_enough() {
        let mut o = Owners(
            BTreeMap::from([(
                "mine".to_owned(),
                Ok(vec![user("other"), user("ciresnave")]),
            )]),
            Vec::new(),
        );
        assert_eq!(
            run(&[("mine", "0.3.0")], &mut o, &ours())[0].1,
            NameState::Ours
        );
    }

    #[test]
    fn a_name_is_taken_when_none_of_ours_is_an_owner_even_if_we_name_nobody() {
        let mut o = scripted();
        assert!(matches!(
            run(&[("mine", "0.3.0")], &mut o, &BTreeSet::new())[0].1,
            NameState::Taken(_)
        ));
    }

    #[test]
    fn a_failed_lookup_is_unknown_never_free() {
        let mut o = Owners(
            BTreeMap::from([("mine".to_owned(), Err("HTTP 503".to_owned()))]),
            Vec::new(),
        );
        let got = run(&[("mine", "0.3.0")], &mut o, &ours());
        assert!(
            matches!(&got[0].1, NameState::Unknown(w) if w.contains("503")),
            "{:?}",
            got[0].1
        );
        assert!(got[0].1.refuses());
        // The index failing is no better.
        let mut reg = registry();
        reg.versions_error
            .insert("brand-new".into(), "index down".into());
        let got = check_names(
            &mut reg,
            &mut scripted(),
            &[("brand-new".into(), v("0.1.0"))],
            &ours(),
        );
        assert!(
            matches!(&got[0].1, NameState::Unknown(w) if w.contains("index down")),
            "{:?}",
            got[0].1
        );
    }

    #[test]
    fn a_yanked_version_of_a_name_we_do_not_own_is_still_taken() {
        let mut reg = registry();
        // What the real index client answers for a yanked version: not Present, not Absent.
        reg.lookup_script.insert(
            "taken".into(),
            [Lookup::Unknown("1.0.0 is on the index but yanked".into())].into(),
        );
        let got = check_names(
            &mut reg,
            &mut scripted(),
            &[("taken".into(), v("1.0.0"))],
            &ours(),
        );
        assert!(matches!(got[0].1, NameState::Taken(_)), "{:?}", got[0].1);
    }

    #[test]
    fn reads_the_owners_response_of_crates_io() {
        let body = r#"{"users":[{"kind":"user","id":3618,"login":"dtolnay","url":"https://github.com/dtolnay","name":"David Tolnay"},{"kind":"team","id":8138,"login":"github:serde-rs:publish"}]}"#;
        assert_eq!(
            parse_owners(body).unwrap(),
            vec![
                user("dtolnay"),
                Owner {
                    kind: "team".into(),
                    login: "github:serde-rs:publish".into()
                }
            ]
        );
        assert_eq!(parse_owners(r#"{"users":[]}"#).unwrap(), vec![]);
        assert!(parse_owners(r#"{"errors":[{"detail":"x"}]}"#).is_err());
        assert!(parse_owners("<html>").is_err());
    }

    #[test]
    fn the_report_lines_name_the_owners() {
        let s = describe("taken", &NameState::Taken(vec![user("someone-else")]));
        assert!(
            s.contains("NAME TAKEN") && s.contains("someone-else (user)"),
            "{s}"
        );
        assert!(describe("x", &NameState::Free).contains("free"));
        assert!(describe("x", &NameState::Unknown("why".into())).contains("UNKNOWN"));
    }
}
