//! The real registry: crates.io's sparse index for lookups, `cargo publish` for uploads.

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::cargo::Cargo;
use crate::classify::{Class, classify};
use crate::clock::now_unix;
use crate::registry::{Lookup, Registry, Upload};

/// The public sparse index.
pub const INDEX_URL: &str = "https://index.crates.io";

/// The index path for a crate name: `1/a`, `2/ab`, `3/a/abc`, `ab/cd/abcd...`.
#[must_use]
pub fn index_path(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    match n.len() {
        0 => String::new(),
        1 => format!("1/{n}"),
        2 => format!("2/{n}"),
        3 => format!("3/{}/{n}", &n[..1]),
        _ => format!("{}/{}/{n}", &n[..2], &n[2..4]),
    }
}

/// One line of an index file.
#[derive(Debug, Deserialize)]
struct Entry {
    vers: String,
    cksum: String,
    #[serde(default)]
    yanked: bool,
}

/// Find `version` in an index file body.
///
/// # Errors
/// A line that is not JSON in the index shape.
pub fn find_version(body: &str, version: &Version) -> Result<Lookup, String> {
    let want = version.to_string();
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        let e: Entry = serde_json::from_str(line).map_err(|e| format!("bad index line: {e}"))?;
        if e.vers == want {
            return Ok(if e.yanked {
                Lookup::Unknown(format!("{want} is on the index but yanked"))
            } else {
                Lookup::Present { checksum: e.cksum }
            });
        }
    }
    Ok(Lookup::Absent)
}

/// Every version in an index file body with its yanked flag.
///
/// # Errors
/// A line that is not JSON in the index shape, or a version that is not semver.
pub fn parse_versions(body: &str) -> Result<Vec<(Version, bool)>, String> {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let e: Entry =
                serde_json::from_str(line).map_err(|e| format!("bad index line: {e}"))?;
            let v = e
                .vers
                .parse()
                .map_err(|err| format!("bad version {:?}: {err}", e.vers))?;
            Ok((v, e.yanked))
        })
        .collect()
}

/// A sparse index client.
pub struct Index {
    agent: ureq::Agent,
    base: String,
}

impl Index {
    /// A client for `base` (use [`INDEX_URL`]).
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
            .user_agent(concat!("releaseant/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            agent,
            base: base.trim_end_matches('/').to_owned(),
        }
    }

    /// All versions of `name`. A 404 is a definitive "no such crate" (empty list); everything
    /// else that is not a 200 is an error.
    ///
    /// # Errors
    /// Transport errors, unexpected statuses and unparseable bodies, as text.
    pub fn versions(&self, name: &str) -> Result<Vec<(Version, bool)>, String> {
        let url = format!("{}/{}", self.base, index_path(name));
        let mut resp = self
            .agent
            .get(&url)
            .header("cache-control", "no-cache")
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        match resp.status().as_u16() {
            200 => {
                let body = resp
                    .body_mut()
                    .read_to_string()
                    .map_err(|e| format!("reading {url}: {e}"))?;
                parse_versions(&body)
            }
            404 => Ok(Vec::new()),
            s => Err(format!("GET {url}: HTTP {s}")),
        }
    }

    /// Ask the index about `name@version`. A transport error or an unexpected status is
    /// [`Lookup::Unknown`], never [`Lookup::Absent`].
    #[must_use]
    pub fn lookup(&self, name: &str, version: &Version) -> Lookup {
        let url = format!("{}/{}", self.base, index_path(name));
        let mut resp = match self
            .agent
            .get(&url)
            .header("cache-control", "no-cache")
            .call()
        {
            Ok(r) => r,
            Err(e) => return Lookup::Unknown(format!("GET {url}: {e}")),
        };
        match resp.status().as_u16() {
            200 => match resp.body_mut().read_to_string() {
                Ok(body) => find_version(&body, version).unwrap_or_else(Lookup::Unknown),
                Err(e) => Lookup::Unknown(format!("reading {url}: {e}")),
            },
            404 => Lookup::Absent, // the crate has no index file at all
            s => Lookup::Unknown(format!("GET {url}: HTTP {s}")),
        }
    }
}

/// SHA-256 of a file as lowercase hex.
///
/// # Errors
/// I/O errors.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let mut out = String::with_capacity(64);
    for b in h.finalize() {
        write!(out, "{b:02x}").expect("writing to a String cannot fail");
    }
    Ok(out)
}

/// Turn a classified `cargo publish` result into the run loop's [`Upload`].
///
/// `checksum` is read lazily, only for a successful publish. `now` is Unix seconds.
#[must_use]
pub fn to_upload(
    class: Class,
    text: &str,
    now: i64,
    checksum: impl FnOnce() -> Result<String, String>,
) -> Upload {
    match class {
        Class::Published => match checksum() {
            Ok(checksum) => Upload::Accepted { checksum },
            Err(why) => {
                Upload::Unknown(format!("published, but the checksum is unavailable: {why}"))
            }
        },
        Class::AlreadyPublished => Upload::AlreadyPublished,
        Class::DependencyMissing { .. } => Upload::DependencyNotVisible,
        Class::RateLimited { retry_at_unix } => Upload::RateLimited {
            retry_after: retry_at_unix
                .map(|at| Duration::from_secs(u64::try_from(at - now).unwrap_or(0))),
        },
        Class::Unknown => {
            let tail: String = text
                .chars()
                .rev()
                .take(1500)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Upload::Unknown(tail)
        }
    }
}

/// crates.io through the sparse index and `cargo publish`, from a prepared archive.
pub struct CratesIo {
    index: Index,
    cargo: Cargo,
    dir: PathBuf,
    target_dir: PathBuf,
}

impl CratesIo {
    /// Publish from `dir` (the `git archive` directory); `target_dir` holds cargo's build output.
    #[must_use]
    pub fn new(cargo: Cargo, dir: PathBuf, target_dir: PathBuf) -> Self {
        Self {
            index: Index::new(INDEX_URL),
            cargo,
            dir,
            target_dir,
        }
    }
}

impl Registry for CratesIo {
    fn lookup(&mut self, name: &str, version: &Version) -> Lookup {
        self.index.lookup(name, version)
    }

    fn versions(&mut self, name: &str) -> Result<Vec<(Version, bool)>, String> {
        self.index.versions(name)
    }

    fn upload(&mut self, name: &str, version: &Version) -> Upload {
        let spec = format!("{name}@{version}");
        let target = self.target_dir.to_string_lossy().into_owned();
        let mut args = vec!["publish", "-p", &spec, "--target-dir", &target];
        if self.dir.join("Cargo.lock").is_file() {
            args.push("--locked");
        }
        let out = match self.cargo.run(&self.dir, &args) {
            Ok(o) => o,
            Err(e) => return Upload::Transient(format!("could not start cargo: {e}")),
        };
        let class = classify(out.success, &out.text);
        let crate_file = self
            .target_dir
            .join("package")
            .join(format!("{name}-{version}.crate"));
        to_upload(class, &out.text, now_unix(), || {
            sha256_file(&crate_file).map_err(|e| format!("{}: {e}", crate_file.display()))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn index_paths_follow_the_registry_layout() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("serde"), "se/rd/serde");
        assert_eq!(index_path("Serde_JSON"), "se/rd/serde_json");
    }

    const BODY: &str = concat!(
        r#"{"name":"x","vers":"1.0.0","cksum":"aaa","yanked":false}"#,
        "\n",
        r#"{"name":"x","vers":"1.1.0","cksum":"bbb","yanked":true}"#,
        "\n",
        r#"{"name":"x","vers":"2.0.0-alpha.1","cksum":"ccc"}"#,
        "\n"
    );

    #[test]
    fn parse_versions_lists_every_version_with_its_yanked_flag() {
        let v = parse_versions(BODY).unwrap();
        let s = |x: &str| x.parse::<Version>().unwrap();
        assert_eq!(
            v,
            [
                (s("1.0.0"), false),
                (s("1.1.0"), true),
                (s("2.0.0-alpha.1"), false)
            ]
        );
        assert!(parse_versions("").unwrap().is_empty());
        assert!(parse_versions("not json").is_err());
        assert!(parse_versions(r#"{"vers":"x.y","cksum":"c"}"#).is_err());
    }

    #[test]
    fn find_version_reads_present_absent_and_yanked() {
        let v = |s: &str| s.parse::<Version>().unwrap();
        assert_eq!(
            find_version(BODY, &v("1.0.0")),
            Ok(Lookup::Present {
                checksum: "aaa".into()
            })
        );
        assert_eq!(
            find_version(BODY, &v("2.0.0-alpha.1")),
            Ok(Lookup::Present {
                checksum: "ccc".into()
            })
        );
        assert_eq!(find_version(BODY, &v("3.0.0")), Ok(Lookup::Absent));
        assert!(matches!(
            find_version(BODY, &v("1.1.0")),
            Ok(Lookup::Unknown(m)) if m.contains("yanked")
        ));
        assert!(find_version("not json\n", &v("1.0.0")).is_err());
    }

    /// Serve `responses` (raw HTTP) one connection each, then stop; returns the base URL.
    fn serve(responses: Vec<&'static str>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for r in responses {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                s.write_all(r.as_bytes()).unwrap();
            }
        });
        format!("http://{addr}")
    }

    fn http(status: &str, body: &str) -> &'static str {
        Box::leak(
            format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_boxed_str(),
        )
    }

    // T8 for the real client: only a definitive answer is Present/Absent.
    #[test]
    fn lookup_maps_statuses_and_transport_errors_without_confusing_them_with_absent() {
        let base = serve(vec![
            http("200 OK", BODY),
            http("404 Not Found", ""),
            http("503 Service Unavailable", "busy"),
            http("200 OK", "this is not an index file"),
        ]);
        let idx = Index::new(&base);
        let v: Version = "1.0.0".parse().unwrap();
        assert_eq!(
            idx.lookup("xyz", &v),
            Lookup::Present {
                checksum: "aaa".into()
            }
        );
        assert_eq!(idx.lookup("xyz", &v), Lookup::Absent);
        assert!(matches!(idx.lookup("xyz", &v), Lookup::Unknown(m) if m.contains("503")));
        assert!(matches!(idx.lookup("xyz", &v), Lookup::Unknown(_)));
        // Nothing listening any more: a transport error, not Absent.
        assert!(matches!(idx.lookup("xyz", &v), Lookup::Unknown(_)));
    }

    // Needs network: the real index, a crate that exists and a name that does not.
    #[test]
    fn real_index_present_and_absent() {
        let idx = Index::new(INDEX_URL);
        let semver: Version = "1.0.0".parse().unwrap();
        match idx.lookup("semver", &semver) {
            Lookup::Present { checksum } => assert_eq!(checksum.len(), 64),
            other => panic!("semver 1.0.0 should be on the index: {other:?}"),
        }
        assert_eq!(
            idx.lookup("semver", &"99.0.0".parse().unwrap()),
            Lookup::Absent
        );
        assert_eq!(
            idx.lookup(
                &format!("zzrl-no-such-crate-{}", std::process::id()),
                &semver
            ),
            Lookup::Absent
        );
        // The same questions through `versions`, which the external-prerequisite check uses.
        let all = idx.versions("semver").unwrap();
        assert!(all.iter().any(|(v, y)| *v == semver && !*y));
        assert!(
            idx.versions(&format!("zzrl-no-such-crate-{}", std::process::id()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn classified_results_map_to_upload_values() {
        let ok = || Ok("deadbeef".to_owned());
        assert_eq!(
            to_upload(Class::Published, "", 0, ok),
            Upload::Accepted {
                checksum: "deadbeef".into()
            }
        );
        assert!(matches!(
            to_upload(Class::Published, "", 0, || Err("gone".into())),
            Upload::Unknown(m) if m.contains("gone")
        ));
        assert_eq!(
            to_upload(Class::AlreadyPublished, "", 0, ok),
            Upload::AlreadyPublished
        );
        assert_eq!(
            to_upload(
                Class::DependencyMissing {
                    package: "p".into()
                },
                "",
                0,
                ok
            ),
            Upload::DependencyNotVisible
        );
        // Retry-after is the server's time minus now, never negative.
        assert_eq!(
            to_upload(
                Class::RateLimited {
                    retry_at_unix: Some(1_000)
                },
                "",
                940,
                ok
            ),
            Upload::RateLimited {
                retry_after: Some(Duration::from_secs(60))
            }
        );
        assert_eq!(
            to_upload(
                Class::RateLimited {
                    retry_at_unix: Some(1_000)
                },
                "",
                5_000,
                ok
            ),
            Upload::RateLimited {
                retry_after: Some(Duration::ZERO)
            }
        );
        assert_eq!(
            to_upload(
                Class::RateLimited {
                    retry_at_unix: None
                },
                "",
                0,
                ok
            ),
            Upload::RateLimited { retry_after: None }
        );
        let long = "x".repeat(5000);
        assert!(matches!(
            to_upload(Class::Unknown, &long, 0, ok),
            Upload::Unknown(m) if m.len() == 1500
        ));
    }

    #[test]
    fn sha256_matches_the_known_digest_of_abc() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, "abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
