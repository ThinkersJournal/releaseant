//! Refuse to publish a commit unless its CI is green (ROADMAP R5).
//!
//! The guard the hand-written `publish-when-green.sh` was: wait for the runs on the commit to
//! finish, publish only if every one of them succeeded. No runs at all is not green: CI that has not
//! started yet is waited for, and CI that never appears ends in [`CiVerdict::TimedOut`].

use std::process::Command;
use std::time::Duration;

use crate::registry::Clock;

/// One workflow run on a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiRun {
    /// `completed`, `in_progress`, `queued` ...
    pub status: String,
    /// `success`, `failure`, `cancelled` ...; empty until the run completes.
    pub conclusion: String,
}

/// Where the runs on a commit come from.
pub trait CiProbe {
    /// The runs of `slug` (`owner/name`) on the commit `sha`.
    ///
    /// # Errors
    /// If the forge cannot be asked, as text.
    fn runs(&mut self, slug: &str, sha: &str) -> Result<Vec<CiRun>, String>;
}

/// What the guard decided.
#[derive(Debug, PartialEq, Eq)]
pub enum CiVerdict {
    /// Runs exist, all have finished, none failed.
    Green,
    /// A run finished with a conclusion other than success, skipped or neutral.
    Red(String),
    /// No decision within the time allowed (CI still running, never started, or the forge could not
    /// be asked).
    TimedOut(String),
}

impl CiVerdict {
    /// The process exit status the hand-written script used: 0 green, 3 red, 4 no decision.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Green => 0,
            Self::Red(_) => 3,
            Self::TimedOut(_) => 4,
        }
    }
}

fn is_ok(conclusion: &str) -> bool {
    matches!(conclusion, "success" | "skipped" | "neutral")
}

/// Poll every `poll` until the runs on `sha` have all finished, or `wait` is used up. A red run is
/// reported at once, without waiting for the others.
pub fn wait_green(
    probe: &mut dyn CiProbe,
    clock: &mut dyn Clock,
    slug: &str,
    sha: &str,
    wait: Duration,
    poll: Duration,
) -> CiVerdict {
    let mut waited = Duration::ZERO;
    loop {
        let state = match probe.runs(slug, sha) {
            Ok(runs) => {
                if let Some(bad) = runs
                    .iter()
                    .find(|r| r.status == "completed" && !is_ok(&r.conclusion))
                {
                    return CiVerdict::Red(format!("a run concluded {:?}", bad.conclusion));
                }
                let open = runs.iter().filter(|r| r.status != "completed").count();
                if !runs.is_empty() && open == 0 {
                    return CiVerdict::Green;
                }
                format!("{} run(s), {open} not finished", runs.len())
            }
            Err(why) => format!("could not ask the forge: {why}"),
        };
        if waited >= wait {
            return CiVerdict::TimedOut(state);
        }
        clock.sleep(poll);
        waited += poll;
    }
}

/// `gh run list`, authenticated however `gh` already is. `RELEASEANT_GH` names another program.
#[derive(Debug, Default)]
pub struct GhProbe;

impl CiProbe for GhProbe {
    fn runs(&mut self, slug: &str, sha: &str) -> Result<Vec<CiRun>, String> {
        let out = Command::new(std::env::var_os("RELEASEANT_GH").unwrap_or_else(|| "gh".into()))
            .args(["run", "list", "-R", slug, "--commit", sha, "--limit", "50"])
            .args(["--json", "status,conclusion"])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
        }
        parse_runs(&String::from_utf8_lossy(&out.stdout))
    }
}

/// Read `gh run list --json status,conclusion`.
///
/// # Errors
/// If the text is not a JSON list of runs.
pub fn parse_runs(text: &str) -> Result<Vec<CiRun>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{e}: {text:?}"))?;
    let list = v
        .as_array()
        .ok_or_else(|| format!("not a list: {text:?}"))?;
    let field = |r: &serde_json::Value, k: &str| {
        r.get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(list
        .iter()
        .map(|r| CiRun {
            status: field(r, "status"),
            conclusion: field(r, "conclusion"),
        })
        .collect())
}

/// `owner/name` from a GitHub remote URL (`https://github.com/o/n.git`, `git@github.com:o/n`).
#[must_use]
pub fn slug_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("github.com")?.1;
    let rest = rest.trim_start_matches([':', '/']).trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (o, n) = rest.split_once('/')?;
    (!o.is_empty() && !n.is_empty() && !n.contains('/')).then(|| format!("{o}/{n}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::FakeClock;

    struct Script(Vec<Result<Vec<CiRun>, String>>);
    impl CiProbe for Script {
        fn runs(&mut self, _: &str, _: &str) -> Result<Vec<CiRun>, String> {
            if self.0.len() > 1 {
                self.0.remove(0)
            } else {
                self.0[0].clone()
            }
        }
    }

    fn run(status: &str, conclusion: &str) -> CiRun {
        CiRun {
            status: status.into(),
            conclusion: conclusion.into(),
        }
    }

    fn go(script: Vec<Result<Vec<CiRun>, String>>) -> (CiVerdict, usize) {
        let mut clock = FakeClock::default();
        let v = wait_green(
            &mut Script(script),
            &mut clock,
            "o/n",
            "sha",
            Duration::from_secs(90),
            Duration::from_secs(30),
        );
        (v, clock.sleeps.len())
    }

    #[test]
    fn green_when_every_run_finished_well() {
        let (v, _) = go(vec![Ok(vec![
            run("completed", "success"),
            run("completed", "skipped"),
        ])]);
        assert_eq!(v, CiVerdict::Green);
    }

    // The control the script had: red is refused with exit 3, even while other runs are open.
    #[test]
    fn red_is_refused_with_exit_3() {
        let (v, slept) = go(vec![Ok(vec![
            run("completed", "failure"),
            run("in_progress", ""),
        ])]);
        assert!(matches!(v, CiVerdict::Red(_)), "{v:?}");
        assert_eq!(v.exit_code(), 3);
        assert_eq!(slept, 0, "a red run is not waited out");
        let (v, _) = go(vec![Ok(vec![run("completed", "cancelled")])]);
        assert_eq!(v.exit_code(), 3, "cancelled is not success");
    }

    #[test]
    fn no_runs_is_not_green_and_times_out_with_exit_4() {
        let (v, slept) = go(vec![Ok(vec![])]);
        assert!(matches!(v, CiVerdict::TimedOut(_)), "{v:?}");
        assert_eq!(v.exit_code(), 4);
        assert_eq!(slept, 3);
    }

    #[test]
    fn waits_for_open_runs_then_decides() {
        let (v, slept) = go(vec![
            Ok(vec![run("in_progress", "")]),
            Ok(vec![run("completed", "success")]),
        ]);
        assert_eq!((v, slept), (CiVerdict::Green, 1));
    }

    #[test]
    fn a_forge_that_cannot_be_asked_is_never_green() {
        let (v, _) = go(vec![Err("HTTP 502".into())]);
        assert!(
            matches!(&v, CiVerdict::TimedOut(w) if w.contains("502")),
            "{v:?}"
        );
    }

    #[test]
    fn reads_gh_json_and_remote_urls() {
        let runs = parse_runs(
            r#"[{"status":"completed","conclusion":"success"},{"status":"queued","conclusion":""}]"#,
        )
        .unwrap();
        assert_eq!(runs, vec![run("completed", "success"), run("queued", "")]);
        assert!(parse_runs("{}").is_err());
        for u in [
            "https://github.com/ThinkersJournal/releaseant.git",
            "git@github.com:ThinkersJournal/releaseant.git",
            "https://github.com/ThinkersJournal/releaseant/",
        ] {
            assert_eq!(
                slug_of(u).as_deref(),
                Some("ThinkersJournal/releaseant"),
                "{u}"
            );
        }
        assert_eq!(slug_of("C:/Projects/origin.git"), None);
    }
}
