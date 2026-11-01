//! Run a publish again until the registry itself shows every crate (ROADMAP R5).
//!
//! `publish` already resumes: a second run skips what the registry has and uploads the rest. What the
//! hand-written loops added was the outer loop: try again after an index timeout, a connection reset
//! or any other failure, and stop only when the registry, asked afresh, lists every crate of the plan
//! at its version and not yanked. The tool's own word that a run succeeded is not that confirmation.

use std::time::Duration;

use crate::pipeline::{PipelineError, Summary};
use crate::registry::{Clock, Registry};
use crate::run::Outcome;

/// How many runs to make and how long to wait between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Most runs to make (at least one).
    pub attempts: u32,
    /// Wait between a run that did not confirm and the next.
    pub pause: Duration,
}

/// How the loop ended.
#[derive(Debug)]
pub enum Ended {
    /// The registry shows every crate of the last run's plan at its version, not yanked.
    Confirmed,
    /// A run ended in a state that running again cannot change (a version someone else put on the
    /// registry, a checksum that differs). Needs a person.
    Stuck(String),
    /// Every attempt ended without confirmation.
    GaveUp(String),
}

/// What [`until_visible`] hands back: how it ended, how many runs it made, and the last run's summary
/// (none if every run failed before it had one).
#[derive(Debug)]
pub struct Finish {
    /// How the loop ended.
    pub ended: Ended,
    /// Runs made.
    pub attempts: u32,
    /// The last run that got far enough to have a summary.
    pub summary: Option<Summary>,
}

/// The crates of the plan that the registry does not show at their version (or shows yanked, or
/// could not be asked about; a failing lookup is never read as "present").
pub fn missing(summary: &Summary, registry: &mut dyn Registry) -> Vec<String> {
    summary
        .plan
        .order
        .iter()
        .filter(|c| match registry.versions(&c.name) {
            Ok(vs) => !vs.iter().any(|(v, yanked)| *v == c.version && !yanked),
            Err(_) => true,
        })
        .map(|c| format!("{}@{}", c.name, c.version))
        .collect()
}

/// Call `run` until the registry confirms its plan, up to `policy.attempts` times, sleeping
/// `policy.pause` between runs. `say` gets one line per attempt.
pub fn until_visible(
    policy: RetryPolicy,
    clock: &mut dyn Clock,
    registry: &mut dyn Registry,
    run: &mut dyn FnMut() -> Result<Summary, PipelineError>,
    say: &dyn Fn(String),
) -> Finish {
    let attempts = policy.attempts.max(1);
    let mut summary = None;
    let mut last = String::new();
    for n in 1..=attempts {
        match run() {
            Err(e) => last = format!("attempt {n} of {attempts} failed: {e}"),
            Ok(s) => {
                let held =
                    s.report.outcomes.iter().find(|(_, o)| {
                        matches!(o, Outcome::Held | Outcome::ChecksumMismatch { .. })
                    });
                if let Some((name, o)) = held {
                    let why = format!("{name}: {o:?}");
                    return Finish {
                        ended: Ended::Stuck(why),
                        attempts: n,
                        summary: Some(s),
                    };
                }
                let gone = if s.is_success() {
                    missing(&s, registry)
                } else {
                    vec![
                        s.report
                            .stopped
                            .clone()
                            .unwrap_or_else(|| "run not complete".into()),
                    ]
                };
                if gone.is_empty() {
                    return Finish {
                        ended: Ended::Confirmed,
                        attempts: n,
                        summary: Some(s),
                    };
                }
                last = if s.is_success() {
                    format!("attempt {n} of {attempts}: not yet on the registry: {gone:?}")
                } else {
                    format!(
                        "attempt {n} of {attempts}: the run did not complete: {}",
                        gone.join("; ").trim()
                    )
                };
                summary = Some(s);
            }
        }
        say(last.clone());
        if n < attempts {
            clock.sleep(policy.pause);
        }
    }
    Finish {
        ended: Ended::GaveUp(last),
        attempts,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::plan::{Plan, PlannedCrate};
    use crate::registry::fake::{FakeClock, FakeRegistry};
    use crate::run::{Mode, Report};

    fn summary(outcome: Outcome) -> Summary {
        Summary {
            report: Report {
                mode: Mode::Execute,
                outcomes: vec![("a".into(), outcome)],
                stopped: None,
            },
            verified: Vec::new(),
            consumer: Vec::new(),
            plan: Plan {
                order: vec![PlannedCrate {
                    name: "a".into(),
                    version: "1.0.0".parse().unwrap(),
                    depends_on: BTreeSet::new(),
                }],
            },
            warnings: Vec::new(),
        }
    }

    fn policy(attempts: u32) -> RetryPolicy {
        RetryPolicy {
            attempts,
            pause: Duration::from_secs(120),
        }
    }

    fn go(
        attempts: u32,
        reg: &mut FakeRegistry,
        script: Vec<Result<Summary, PipelineError>>,
    ) -> (Finish, usize, Vec<Duration>) {
        let mut clock = FakeClock::default();
        let mut script = script.into_iter();
        let mut calls = 0;
        let f = until_visible(
            policy(attempts),
            &mut clock,
            reg,
            &mut || {
                calls += 1;
                script.next().expect("run called more often than scripted")
            },
            &|_| {},
        );
        (f, calls, clock.sleeps)
    }

    #[test]
    fn a_failed_run_is_run_again_until_the_registry_shows_the_crate() {
        let mut reg = FakeRegistry::new().with_published("a", "1.0.0");
        let (f, calls, sleeps) = go(
            5,
            &mut reg,
            vec![
                Err(PipelineError::OnlyWithExclusions),
                Ok(summary(Outcome::Visible)),
            ],
        );
        assert!(matches!(f.ended, Ended::Confirmed), "{:?}", f.ended);
        assert_eq!((f.attempts, calls), (2, 2));
        assert_eq!(sleeps, vec![Duration::from_secs(120)]);
    }

    // The control: the tool saying "published" is not the registry saying so.
    #[test]
    fn a_run_that_says_success_but_the_registry_lacks_is_not_confirmed() {
        let mut reg = FakeRegistry::new();
        let (f, calls, sleeps) = go(
            3,
            &mut reg,
            vec![
                Ok(summary(Outcome::Visible)),
                Ok(summary(Outcome::Visible)),
                Ok(summary(Outcome::Visible)),
            ],
        );
        assert!(
            matches!(&f.ended, Ended::GaveUp(w) if w.contains("a@1.0.0")),
            "{:?}",
            f.ended
        );
        assert_eq!((f.attempts, calls, sleeps.len()), (3, 3, 2));
    }

    // The line for a run that stopped says so; it is not "not yet on the registry".
    #[test]
    fn a_stopped_run_is_reported_as_not_complete() {
        let mut reg = FakeRegistry::new();
        let mut s = summary(Outcome::Interrupted("boom".into()));
        s.report.stopped = Some("unrecognised upload result: boom".into());
        let lines = std::cell::RefCell::new(Vec::new());
        let mut clock = FakeClock::default();
        let mut once = Some(s);
        let f = until_visible(
            policy(1),
            &mut clock,
            &mut reg,
            &mut || Ok(once.take().unwrap()),
            &|l| lines.borrow_mut().push(l),
        );
        assert!(matches!(f.ended, Ended::GaveUp(_)));
        let lines = lines.into_inner();
        assert!(
            lines[0].contains("did not complete: unrecognised upload result: boom"),
            "{lines:?}"
        );
        assert!(!lines[0].contains("not yet on the registry"), "{lines:?}");
    }

    #[test]
    fn a_yanked_version_is_not_confirmed() {
        let mut reg = FakeRegistry::new().with_published("a", "1.0.0");
        reg.yanked.insert("a".into());
        let (f, _, _) = go(1, &mut reg, vec![Ok(summary(Outcome::Visible))]);
        assert!(matches!(f.ended, Ended::GaveUp(_)), "{:?}", f.ended);
    }

    #[test]
    fn a_registry_that_cannot_be_asked_is_not_confirmed() {
        let mut reg = FakeRegistry::new().with_published("a", "1.0.0");
        reg.versions_error.insert("a".into(), "HTTP 502".into());
        let (f, _, _) = go(1, &mut reg, vec![Ok(summary(Outcome::Visible))]);
        assert!(matches!(f.ended, Ended::GaveUp(_)), "{:?}", f.ended);
    }

    #[test]
    fn a_held_version_stops_the_loop_at_once() {
        let mut reg = FakeRegistry::new().with_published("a", "1.0.0");
        let (f, calls, sleeps) = go(5, &mut reg, vec![Ok(summary(Outcome::Held))]);
        assert!(matches!(f.ended, Ended::Stuck(_)), "{:?}", f.ended);
        assert_eq!((calls, sleeps.len()), (1, 0));
    }

    #[test]
    fn one_attempt_is_one_run_and_no_sleep() {
        let mut reg = FakeRegistry::new();
        let (f, calls, sleeps) = go(0, &mut reg, vec![Err(PipelineError::OnlyWithExclusions)]);
        assert!(matches!(f.ended, Ended::GaveUp(_)));
        assert_eq!((f.attempts, calls, sleeps.len()), (1, 1, 0));
    }
}
