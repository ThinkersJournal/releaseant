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

/// One check run on a commit (a job, as branch protection names it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    /// The check's name, as `required_status_checks.contexts` lists it.
    pub name: String,
    /// `completed`, `in_progress`, `queued` ...
    pub status: String,
    /// `success`, `failure`, `cancelled` ...; empty until the check completes.
    pub conclusion: String,
}

/// Where the runs on a commit come from.
pub trait CiProbe {
    /// The runs of `slug` (`owner/name`) on the commit `sha`.
    ///
    /// # Errors
    /// If the forge cannot be asked, as text.
    fn runs(&mut self, slug: &str, sha: &str) -> Result<Vec<CiRun>, String>;

    /// The check runs of `slug` on the commit `sha`, by name.
    ///
    /// # Errors
    /// If the forge cannot be asked, as text.
    fn check_runs(&mut self, _slug: &str, _sha: &str) -> Result<Vec<CheckRun>, String> {
        Err("this probe cannot read check runs".to_owned())
    }

    /// The check names that branch protection of `branch` requires; empty if it requires none.
    ///
    /// # Errors
    /// If the forge cannot be asked, or the answer is not a list of names, as text.
    fn required_contexts(&mut self, _slug: &str, _branch: &str) -> Result<Vec<String>, String> {
        Err("this probe cannot read branch protection".to_owned())
    }
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

/// Which checks to judge, and the line to print saying which rule ran. `explicit` (from
/// `--require-checks`) wins; else the names branch protection of `branch` requires; else `None`,
/// which means every run on the commit, as before. The line always says which of the three ran and,
/// for the last, why.
pub fn choose_checks(
    probe: &mut dyn CiProbe,
    slug: &str,
    branch: &str,
    explicit: &[String],
) -> (Option<Vec<String>>, String) {
    if !explicit.is_empty() {
        let line = format!(
            "judging the {} check(s) named by --require-checks: {}",
            explicit.len(),
            explicit.join(", ")
        );
        return (Some(explicit.to_vec()), line);
    }
    match probe.required_contexts(slug, branch) {
        Ok(names) if !names.is_empty() => {
            let line = format!(
                "judging the {} check(s) that branch protection of {branch} requires: {}",
                names.len(),
                names.join(", ")
            );
            (Some(names), line)
        }
        Ok(_) => (
            None,
            format!(
                "branch protection of {branch} requires no checks: judging every run on the commit"
            ),
        ),
        Err(why) => (
            None,
            format!(
                "could not read branch protection of {branch} ({why}): judging every run on the commit"
            ),
        ),
    }
}

/// Like [`wait_green`], but judges only the checks called `names`, and fails closed: a named check
/// that is absent or not finished is no answer, one that finished badly is red (reported at once),
/// and every other check on the commit is ignored. A name is satisfied only if every check run
/// carrying it succeeded.
pub fn wait_checks(
    probe: &mut dyn CiProbe,
    clock: &mut dyn Clock,
    slug: &str,
    sha: &str,
    names: &[String],
    wait: Duration,
    poll: Duration,
) -> CiVerdict {
    if names.is_empty() {
        return CiVerdict::TimedOut("no check names to judge".to_owned());
    }
    let mut waited = Duration::ZERO;
    loop {
        let state = match probe.check_runs(slug, sha) {
            Ok(runs) => {
                let mut open = Vec::new();
                for name in names {
                    let mine: Vec<&CheckRun> = runs.iter().filter(|r| &r.name == name).collect();
                    if let Some(bad) = mine
                        .iter()
                        .find(|r| r.status == "completed" && !is_ok(&r.conclusion))
                    {
                        return CiVerdict::Red(format!(
                            "check {name:?} concluded {:?}",
                            bad.conclusion
                        ));
                    }
                    if mine.is_empty() {
                        open.push(format!("{name} (absent)"));
                    } else if let Some(r) = mine.iter().find(|r| r.status != "completed") {
                        open.push(format!("{name} ({})", r.status));
                    }
                }
                if open.is_empty() {
                    return CiVerdict::Green;
                }
                format!(
                    "{} of {} named check(s) not finished: {}",
                    open.len(),
                    names.len(),
                    open.join(", ")
                )
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

impl GhProbe {
    fn gh(args: &[&str]) -> Result<String, String> {
        let out = Command::new(std::env::var_os("RELEASEANT_GH").unwrap_or_else(|| "gh".into()))
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl CiProbe for GhProbe {
    fn runs(&mut self, slug: &str, sha: &str) -> Result<Vec<CiRun>, String> {
        let text = Self::gh(&[
            "run",
            "list",
            "-R",
            slug,
            "--commit",
            sha,
            "--limit",
            "50",
            "--json",
            "status,conclusion",
        ])?;
        parse_runs(&text)
    }

    fn check_runs(&mut self, slug: &str, sha: &str) -> Result<Vec<CheckRun>, String> {
        let path = format!("repos/{slug}/commits/{sha}/check-runs?per_page=100");
        let text = Self::gh(&[
            "api",
            "--paginate",
            &path,
            "--jq",
            ".check_runs[] | {name, status, conclusion}",
        ])?;
        parse_check_runs(&text)
    }

    fn required_contexts(&mut self, slug: &str, branch: &str) -> Result<Vec<String>, String> {
        let path = format!("repos/{slug}/branches/{branch}");
        let text = Self::gh(&[
            "api",
            &path,
            "--jq",
            ".protection.required_status_checks.contexts // []",
        ])?;
        parse_contexts(&text)
    }
}

/// Read `gh api ... --jq '.check_runs[] | {name, status, conclusion}'`: one JSON object per check
/// (a JSON list of them is accepted too).
///
/// # Errors
/// If the text is not a stream of check objects.
pub fn parse_check_runs(text: &str) -> Result<Vec<CheckRun>, String> {
    let mut out = Vec::new();
    for v in serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>() {
        let v = v.map_err(|e| format!("{e}: {text:?}"))?;
        let items = match v {
            serde_json::Value::Array(a) => a,
            other => vec![other],
        };
        for item in items {
            let field = |k: &str| {
                item.get(k)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            };
            let name = field("name").ok_or_else(|| format!("a check without a name: {item}"))?;
            out.push(CheckRun {
                name,
                status: field("status").unwrap_or_default(),
                conclusion: field("conclusion").unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

/// Read a JSON list of check names (`null` is none).
///
/// # Errors
/// If the text is not a list of strings.
pub fn parse_contexts(text: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{e}: {text:?}"))?;
    match v {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(a) => a
            .iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("not a check name: {x}"))
            })
            .collect(),
        _ => Err(format!("not a list: {text:?}")),
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

    // --- judging named checks (fail closed) ---

    struct Checks {
        runs: Vec<Result<Vec<CheckRun>, String>>,
        contexts: Result<Vec<String>, String>,
    }
    impl CiProbe for Checks {
        fn runs(&mut self, _: &str, _: &str) -> Result<Vec<CiRun>, String> {
            Err("not used".into())
        }
        fn check_runs(&mut self, _: &str, _: &str) -> Result<Vec<CheckRun>, String> {
            if self.runs.len() > 1 {
                self.runs.remove(0)
            } else {
                self.runs[0].clone()
            }
        }
        fn required_contexts(&mut self, _: &str, _: &str) -> Result<Vec<String>, String> {
            self.contexts.clone()
        }
    }

    fn check(name: &str, status: &str, conclusion: &str) -> CheckRun {
        CheckRun {
            name: name.into(),
            status: status.into(),
            conclusion: conclusion.into(),
        }
    }

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    fn judge(want: &[&str], script: Vec<Result<Vec<CheckRun>, String>>) -> (CiVerdict, usize) {
        let mut clock = FakeClock::default();
        let v = wait_checks(
            &mut Checks {
                runs: script,
                contexts: Ok(vec![]),
            },
            &mut clock,
            "o/n",
            "sha",
            &names(want),
            Duration::from_secs(90),
            Duration::from_secs(30),
        );
        (v, clock.sleeps.len())
    }

    // The Fuel case: two self-hosted runs sit queued forever, but they are not the named checks.
    #[test]
    fn unnamed_runs_are_ignored_whatever_state_they_are_in() {
        let (v, slept) = judge(
            &["Clippy", "Rustfmt"],
            vec![Ok(vec![
                check("Clippy", "completed", "success"),
                check("Rustfmt", "completed", "success"),
                check("checkpoint-tests", "queued", ""),
                check("cuda-metal-compile-check", "queued", ""),
                check("lavapipe-vulkan", "completed", "failure"),
            ])],
        );
        assert_eq!((v, slept), (CiVerdict::Green, 0));
    }

    #[test]
    fn a_named_check_that_is_absent_is_no_answer_never_green() {
        let (v, slept) = judge(
            &["Clippy", "Packaged crates"],
            vec![Ok(vec![check("Clippy", "completed", "success")])],
        );
        assert!(
            matches!(&v, CiVerdict::TimedOut(w) if w.contains("Packaged crates (absent)")),
            "{v:?}"
        );
        assert_eq!(v.exit_code(), 4);
        assert_eq!(slept, 3, "it waited out the whole budget");
    }

    #[test]
    fn a_named_check_that_never_started_is_no_answer() {
        let (v, _) = judge(&["Clippy"], vec![Ok(vec![check("Clippy", "queued", "")])]);
        assert!(
            matches!(&v, CiVerdict::TimedOut(w) if w.contains("Clippy (queued)")),
            "{v:?}"
        );
    }

    #[test]
    fn a_named_check_that_failed_is_red_at_once_even_while_others_are_open() {
        let (v, slept) = judge(
            &["Clippy", "Rustfmt", "Packaged crates"],
            vec![Ok(vec![
                check("Clippy", "completed", "failure"),
                check("Rustfmt", "in_progress", ""),
            ])],
        );
        assert!(
            matches!(&v, CiVerdict::Red(w) if w.contains("Clippy")),
            "{v:?}"
        );
        assert_eq!((v.exit_code(), slept), (3, 0));
        for bad in ["cancelled", "timed_out", "action_required"] {
            let (v, _) = judge(
                &["Clippy"],
                vec![Ok(vec![check("Clippy", "completed", bad)])],
            );
            assert_eq!(v.exit_code(), 3, "{bad} is not success");
        }
    }

    #[test]
    fn a_name_with_several_runs_needs_every_one_to_succeed() {
        let (v, _) = judge(
            &["Test Suite"],
            vec![Ok(vec![
                check("Test Suite", "completed", "success"),
                check("Test Suite", "completed", "failure"),
            ])],
        );
        assert_eq!(v.exit_code(), 3, "{v:?}");
        let (v, _) = judge(
            &["Test Suite"],
            vec![Ok(vec![
                check("Test Suite", "completed", "success"),
                check("Test Suite", "in_progress", ""),
            ])],
        );
        assert!(matches!(v, CiVerdict::TimedOut(_)), "{v:?}");
    }

    #[test]
    fn named_checks_are_waited_for_then_decided() {
        let (v, slept) = judge(
            &["Clippy"],
            vec![
                Ok(vec![]),
                Ok(vec![check("Clippy", "in_progress", "")]),
                Ok(vec![check("Clippy", "completed", "success")]),
            ],
        );
        assert_eq!((v, slept), (CiVerdict::Green, 2));
    }

    #[test]
    fn names_match_exactly_and_skipped_counts_as_passed() {
        let (v, _) = judge(
            &["clippy"],
            vec![Ok(vec![check("Clippy", "completed", "success")])],
        );
        assert!(matches!(v, CiVerdict::TimedOut(_)), "case differs: {v:?}");
        let (v, _) = judge(
            &["Clippy"],
            vec![Ok(vec![check("Clippy", "completed", "skipped")])],
        );
        assert_eq!(v, CiVerdict::Green);
    }

    #[test]
    fn a_forge_that_cannot_be_asked_and_an_empty_list_are_never_green() {
        let (v, _) = judge(&["Clippy"], vec![Err("HTTP 502".into())]);
        assert!(
            matches!(&v, CiVerdict::TimedOut(w) if w.contains("502")),
            "{v:?}"
        );
        let (v, slept) = judge(&[], vec![Ok(vec![check("Clippy", "completed", "success")])]);
        assert!(matches!(v, CiVerdict::TimedOut(_)), "{v:?}");
        assert_eq!(slept, 0);
    }

    // --- which rule runs, and the line that says so ---

    fn pick(
        explicit: &[&str],
        contexts: Result<Vec<String>, String>,
    ) -> (Option<Vec<String>>, String) {
        choose_checks(
            &mut Checks {
                runs: vec![Ok(vec![])],
                contexts,
            },
            "o/n",
            "main",
            &names(explicit),
        )
    }

    #[test]
    fn explicit_names_win_over_branch_protection() {
        let (n, line) = pick(&["A", "B"], Ok(names(&["Clippy"])));
        assert_eq!(n, Some(names(&["A", "B"])));
        assert!(
            line.contains("--require-checks") && line.contains("A, B"),
            "{line}"
        );
    }

    #[test]
    fn with_no_names_the_branch_protection_contexts_are_used() {
        let (n, line) = pick(&[], Ok(names(&["Clippy", "Rustfmt"])));
        assert_eq!(n, Some(names(&["Clippy", "Rustfmt"])));
        assert!(
            line.contains("branch protection of main requires") && line.contains("Clippy, Rustfmt"),
            "{line}"
        );
    }

    #[test]
    fn no_protection_or_an_unreadable_one_falls_back_to_every_run_and_says_why() {
        let (n, line) = pick(&[], Ok(vec![]));
        assert_eq!(n, None);
        assert!(
            line.contains("requires no checks") && line.contains("every run"),
            "{line}"
        );
        let (n, line) = pick(&[], Err("HTTP 404".into()));
        assert_eq!(n, None);
        assert!(
            line.contains("could not read") && line.contains("404") && line.contains("every run"),
            "{line}"
        );
    }

    #[test]
    fn reads_check_runs_and_contexts_from_gh() {
        let stream = "{\"name\":\"Clippy\",\"status\":\"completed\",\"conclusion\":\"success\"}\n\
                      {\"name\":\"Test Suite (ubuntu-latest)\",\"status\":\"queued\",\"conclusion\":null}\n";
        assert_eq!(
            parse_check_runs(stream).unwrap(),
            vec![
                check("Clippy", "completed", "success"),
                check("Test Suite (ubuntu-latest)", "queued", ""),
            ]
        );
        assert_eq!(parse_check_runs("").unwrap(), vec![]);
        assert_eq!(
            parse_check_runs(
                "[{\"name\":\"A\",\"status\":\"completed\",\"conclusion\":\"success\"}]"
            )
            .unwrap(),
            vec![check("A", "completed", "success")]
        );
        assert!(
            parse_check_runs("{\"status\":\"queued\"}").is_err(),
            "no name"
        );
        assert!(parse_check_runs("not json").is_err());
        assert_eq!(
            parse_contexts("[\"Clippy\",\"Rustfmt\"]").unwrap(),
            names(&["Clippy", "Rustfmt"])
        );
        assert_eq!(parse_contexts("[]").unwrap(), Vec::<String>::new());
        assert_eq!(parse_contexts("null").unwrap(), Vec::<String>::new());
        // `gh run list` output is a list of objects, not of names: it must not be read as contexts.
        assert!(parse_contexts("[{\"status\":\"completed\"}]").is_err());
        assert!(parse_contexts("{}").is_err());
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
