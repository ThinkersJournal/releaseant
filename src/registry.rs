//! The registry and clock the run loop talks to, as traits, plus a scriptable fake (DESIGN section 1).
//!
//! The real implementation (cargo and crates.io) arrives in R3; classifying cargo's text into
//! [`Upload`] is the part DESIGN section 4 reserves for samples captured in R4.

use std::time::Duration;

use semver::Version;

/// What the registry says about one `name@version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// The version is on the index; `checksum` is the index's SHA-256 of the `.crate`.
    Present {
        /// Index checksum.
        checksum: String,
    },
    /// A definitive answer that the version is not there.
    Absent,
    /// The lookup itself failed (timeout, server error, DNS). Never read as `Absent`.
    Unknown(String),
}

/// A classified upload result (DESIGN section 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upload {
    /// The registry accepted the crate; `checksum` is the SHA-256 of what was uploaded.
    Accepted {
        /// SHA-256 of the uploaded `.crate`.
        checksum: String,
    },
    /// The version already exists.
    AlreadyPublished,
    /// 429 or equivalent; `retry_after` is the server's advice when it gave one.
    RateLimited {
        /// Server-advised wait.
        retry_after: Option<Duration>,
    },
    /// A dependency is not yet visible on the index.
    DependencyNotVisible,
    /// Network-level trouble worth a bounded retry.
    Transient(String),
    /// The registry refused for a reason a retry cannot fix.
    Permanent(String),
    /// Output that matched no known class. Stops the run; never retried.
    Unknown(String),
}

/// The registry operations a run needs.
pub trait Registry {
    /// Ask whether `name@version` is on the registry.
    fn lookup(&mut self, name: &str, version: &Version) -> Lookup;
    /// Upload `name@version` from the prepared archive.
    fn upload(&mut self, name: &str, version: &Version) -> Upload;
}

/// Time, injectable so waiting is testable.
pub trait Clock {
    /// Block for `d`.
    fn sleep(&mut self, d: Duration);
    /// The current UTC time as text for ledger events.
    fn now_utc(&mut self) -> String;
}

/// Scriptable in-memory registry and clock for tests.
pub mod fake {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    use super::{Clock, Duration, Lookup, Registry, Upload, Version};

    /// A callback run at the start of every upload with `(name, version)`.
    pub type UploadHook = Box<dyn FnMut(&str, &str)>;

    /// In-memory registry. Behaviour not scripted is the honest default: a duplicate upload is
    /// refused, an accepted upload becomes visible after `lookup_lag` `Absent` answers.
    #[derive(Default)]
    pub struct FakeRegistry {
        /// `(name, version)` to the checksum the index shows.
        pub published: BTreeMap<(String, String), String>,
        /// Results returned before the default behaviour, per crate name, oldest first.
        pub upload_script: BTreeMap<String, VecDeque<Upload>>,
        /// Answers returned before the default behaviour, per crate name, oldest first.
        pub lookup_script: BTreeMap<String, VecDeque<Lookup>>,
        /// How many `Absent` answers follow an accepted upload before it is visible.
        pub lookup_lag: u32,
        /// Crates that are accepted but never appear (index never catches up).
        pub never_visible: BTreeSet<String>,
        /// Crates whose index checksum differs from the uploaded one.
        pub index_checksum_override: BTreeMap<String, String>,
        /// Every accepted upload, in order.
        pub uploads: Vec<(String, String)>,
        /// Every `upload` call, including refused ones, in order.
        pub upload_calls: Vec<String>,
        /// Called at the start of each upload.
        pub on_upload: Option<UploadHook>,
        lag_left: BTreeMap<(String, String), u32>,
    }

    impl FakeRegistry {
        /// An empty registry.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Mark `name@version` as already published.
        pub fn with_published(mut self, name: &str, version: &str) -> Self {
            self.published.insert(
                (name.into(), version.into()),
                format!("sum-{name}-{version}"),
            );
            self
        }
    }

    impl Registry for FakeRegistry {
        fn lookup(&mut self, name: &str, version: &Version) -> Lookup {
            if let Some(forced) = self
                .lookup_script
                .get_mut(name)
                .and_then(VecDeque::pop_front)
            {
                return forced;
            }
            let key = (name.to_owned(), version.to_string());
            if let Some(left) = self.lag_left.get_mut(&key)
                && *left > 0
            {
                *left -= 1;
                return Lookup::Absent;
            }
            match self.published.get(&key) {
                Some(checksum) => Lookup::Present {
                    checksum: self
                        .index_checksum_override
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| checksum.clone()),
                },
                None => Lookup::Absent,
            }
        }

        fn upload(&mut self, name: &str, version: &Version) -> Upload {
            let v = version.to_string();
            self.upload_calls.push(name.to_owned());
            if let Some(hook) = self.on_upload.as_mut() {
                hook(name, &v);
            }
            if let Some(forced) = self
                .upload_script
                .get_mut(name)
                .and_then(VecDeque::pop_front)
            {
                return forced;
            }
            let key = (name.to_owned(), v.clone());
            if self.published.contains_key(&key) {
                return Upload::AlreadyPublished;
            }
            let checksum = format!("sum-{name}-{v}");
            self.uploads.push((name.to_owned(), v));
            if !self.never_visible.contains(name) {
                self.published.insert(key.clone(), checksum.clone());
                self.lag_left.insert(key, self.lookup_lag);
            }
            Upload::Accepted { checksum }
        }
    }

    /// A clock that records sleeps instead of sleeping.
    #[derive(Debug, Default)]
    pub struct FakeClock {
        /// Every requested sleep, in order.
        pub sleeps: Vec<Duration>,
    }

    impl Clock for FakeClock {
        fn sleep(&mut self, d: Duration) {
            self.sleeps.push(d);
        }

        fn now_utc(&mut self) -> String {
            "2026-10-24T00:00:00Z".to_owned()
        }
    }
}
