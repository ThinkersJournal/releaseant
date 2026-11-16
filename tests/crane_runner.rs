//! The real-`crane` check against a real `registry:2` (docs/ARTIFACTS-DESIGN.md section 9, slice A6;
//! docs/RUNNER-TESTS.md). It needs Docker and `crane`, which the hosted `cargo test` job does not provide
//! (an ubuntu-latest hosted runner has Docker and the workflow installs crane; the windows and macos
//! hosted runners have no Linux containers), so it sits behind the `runner-tests` feature: type-checked by
//! CI on every PR (clippy and doc build with all features), RUN by `.github/workflows/runner-tests.yml` on
//! an ubuntu-latest hosted runner. It is never `#[ignore]`d, and a missing Docker or `crane` is a FAILURE with a
//! message, not a skip: a test that quietly does nothing is indistinguishable from one that passed.
//!
//! What it does is what `docs/ARTIFACT-MEASUREMENTS.md` records being done by hand on 2026-11-08:
//! build a two-platform image index with crane only (no daemon), export it as an OCI layout, then run
//! `stage_images`, a second `stage_images`, `promote_images`, a held tag and an unreachable registry
//! through the real `Crane` against a throwaway `registry:2` container that the test starts and removes.
#![cfg(feature = "runner-tests")]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use releaseant::artifact::{Artifact, Kind};
use releaseant::images::{Crane, ImageRegistry, promote_images, read_layout, stage_images};
use releaseant::ledger::Ledger;
use releaseant::registry::fake::FakeClock;
use releaseant::stage::{Ctx, Outcome};

/// `crane`: `$CRANE`, else `crane` on the PATH. Fails loudly when neither works.
fn crane_exe() -> PathBuf {
    let exe = std::env::var_os("CRANE").map_or_else(|| PathBuf::from("crane"), PathBuf::from);
    let ok = Command::new(&exe)
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    assert!(
        ok,
        "crane is not runnable ({}): the runner must provide it (set CRANE or put it on the PATH). This is a failure, not a skip.",
        exe.display()
    );
    exe
}

/// A `registry:2` container on a free localhost port, removed when dropped.
struct Registry {
    id: String,
    host: String,
}

impl Registry {
    fn start() -> Self {
        let out = Command::new("docker")
            .args(["run", "-d", "-p", "127.0.0.1::5000", "registry:2"])
            .output()
            .expect("docker could not be run: the runner must provide it. This is a failure, not a skip.");
        assert!(
            out.status.success(),
            "docker run registry:2 failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let id = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        // Owned from here on, so the container is removed even if the steps below panic.
        let mut reg = Self {
            id: id.clone(),
            host: String::new(),
        };
        // `docker port` prints `127.0.0.1:49153` (possibly more than one line).
        let port = Command::new("docker")
            .args(["port", &id, "5000/tcp"])
            .output()
            .unwrap();
        let text = String::from_utf8(port.stdout).unwrap();
        let addr = text
            .lines()
            .find(|l| l.starts_with("127.0.0.1:"))
            .unwrap_or_else(|| panic!("no 127.0.0.1 port mapping in {text:?}"))
            .trim()
            .to_owned();
        reg.host = addr;
        let up = Instant::now();
        loop {
            let ok = ureq::get(format!("http://{}/v2/", reg.host))
                .call()
                .is_ok_and(|r| r.status().as_u16() == 200);
            if ok {
                return reg;
            }
            assert!(
                up.elapsed() < Duration::from_secs(60),
                "registry:2 did not answer on {} within 60 s",
                reg.host
            );
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.id]).output();
    }
}

fn run(exe: &Path, args: &[&str]) -> String {
    let out = Command::new(exe).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "crane {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A one-file layer tarball.
fn layer(path: &Path, name: &str, body: &str) {
    let f = std::fs::File::create(path).unwrap();
    let mut b = tar::Builder::new(f);
    let mut h = tar::Header::new_gnu();
    h.set_size(body.len() as u64);
    h.set_mode(0o644);
    h.set_cksum();
    b.append_data(&mut h, name, body.as_bytes()).unwrap();
    b.finish().unwrap();
}

/// A free local port nothing listens on (bind, note, drop).
fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn real_crane_stages_promotes_holds_and_stops_against_a_real_registry() {
    let crane = crane_exe();
    let reg = Registry::start();
    let host = &reg.host;
    let work = tempfile::tempdir().unwrap();

    // The fixture: a two-platform index, built with crane only, exported as an OCI layout.
    let (l1, l2) = (work.path().join("l1.tar"), work.path().join("l2.tar"));
    layer(&l1, "f.txt", "hello-amd64");
    layer(&l2, "g.txt", "hello-arm64");
    let src = format!("{host}/stage/app");
    run(
        &crane,
        &[
            "append",
            "-f",
            &l1.to_string_lossy(),
            "-t",
            &format!("{src}:amd64"),
        ],
    );
    run(
        &crane,
        &[
            "append",
            "-f",
            &l2.to_string_lossy(),
            "-t",
            &format!("{src}:arm64"),
        ],
    );
    run(
        &crane,
        &[
            "mutate",
            "--set-platform",
            "linux/amd64",
            "-t",
            &format!("{src}:amd64p"),
            &format!("{src}:amd64"),
        ],
    );
    run(
        &crane,
        &[
            "mutate",
            "--set-platform",
            "linux/arm64",
            "-t",
            &format!("{src}:arm64p"),
            &format!("{src}:arm64"),
        ],
    );
    run(
        &crane,
        &[
            "index",
            "append",
            "-t",
            &format!("{src}:idx"),
            "-m",
            &format!("{src}:amd64p"),
            "-m",
            &format!("{src}:arm64p"),
        ],
    );
    let digest = run(&crane, &["digest", &format!("{src}:idx")])
        .trim()
        .to_owned();
    let root = work.path().join("artifacts");
    std::fs::create_dir_all(root.join("dist")).unwrap();
    run(
        &crane,
        &[
            "pull",
            "--format",
            "oci",
            &format!("{src}@{digest}"),
            &root.join("dist/img").to_string_lossy(),
        ],
    );
    // What releaseant reads from the layout is what crane built.
    let layout = read_layout(&root.join("dist/img")).unwrap();
    assert_eq!(layout.digest, digest);

    let repo = format!("{host}/real/app");
    let art = Artifact {
        name: "img".into(),
        kind: Kind::Image,
        path: None,
        source: Some("dir:dist/img".into()),
        sha256: None,
        digest: Some(digest.clone()),
        sums: None,
        target: repo.clone(),
        tags: Some(vec!["1.0.0".into(), "latest".into()]),
        platforms: Some(vec!["linux/amd64".into(), "linux/arm64".into()]),
    };
    let ledger = Ledger::open(work.path().join("ledger.jsonl")).unwrap();
    let ctx = Ctx {
        run_id: "r".into(),
        sha: "a".repeat(40),
    };
    let mut clock = FakeClock::default();
    let mut c = Crane { exe: crane.clone() };
    let one = |r: &releaseant::stage::StageReport| {
        assert_eq!(r.outcomes.len(), 1, "{r:?}");
        r.outcomes[0].1.clone()
    };

    // The registry has nothing yet: absent is `None`, not an error.
    assert_eq!(c.manifest_digest(&repo, &digest), Ok(None));
    // Stage: pushed by digest, read back, and no tag exists afterwards.
    let r = stage_images(
        &mut c,
        &ledger,
        &mut clock,
        &ctx,
        &root,
        std::slice::from_ref(&art),
    )
    .unwrap();
    assert_eq!(one(&r), Outcome::Visible, "{r:?}");
    assert_eq!(
        c.manifest_digest(&repo, "1.0.0"),
        Ok(None),
        "nothing is pullable by name yet"
    );
    // Stage again: found by asking, not pushed again.
    let r = stage_images(
        &mut c,
        &ledger,
        &mut clock,
        &ctx,
        &root,
        std::slice::from_ref(&art),
    )
    .unwrap();
    assert_eq!(one(&r), Outcome::AlreadyPublished, "{r:?}");
    // Promote: both tags now resolve to the declared digest.
    let r = promote_images(
        &mut c,
        &ledger,
        &mut clock,
        &ctx,
        &root,
        std::slice::from_ref(&art),
    )
    .unwrap();
    assert_eq!(one(&r), Outcome::Visible, "{r:?}");
    for t in ["1.0.0", "latest"] {
        assert_eq!(c.manifest_digest(&repo, t), Ok(Some(digest.clone())), "{t}");
    }
    // Promote again: nothing to move.
    let r = promote_images(
        &mut c,
        &ledger,
        &mut clock,
        &ctx,
        &root,
        std::slice::from_ref(&art),
    )
    .unwrap();
    assert_eq!(one(&r), Outcome::AlreadyPublished, "{r:?}");

    // A tag held by another image stops staging before anything is pushed or moved.
    let other = run(&crane, &["digest", &format!("{src}:amd64p")])
        .trim()
        .to_owned();
    c.tag(&src, &other, "x").unwrap();
    let mut held = art.clone();
    held.target = src.clone();
    held.tags = Some(vec!["x".into()]);
    let r = stage_images(&mut c, &ledger, &mut clock, &ctx, &root, &[held]).unwrap();
    assert!(
        matches!(one(&r), Outcome::Held(m) if m.contains("nothing was pushed or moved")),
        "{r:?}"
    );
    assert_eq!(
        c.manifest_digest(&src, "x"),
        Ok(Some(other)),
        "the taken tag was left alone"
    );

    // Nothing listening: the run stops, it does not read that as "absent" and push.
    let mut down = art.clone();
    down.target = format!("127.0.0.1:{}/x/y", closed_port());
    let r = stage_images(&mut c, &ledger, &mut clock, &ctx, &root, &[down]).unwrap();
    assert!(r.outcomes.is_empty() && r.stopped.is_some(), "{r:?}");
}
