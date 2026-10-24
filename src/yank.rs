//! Yanking (DESIGN section 7): a deliberate command with a recorded reason, never a side effect.
//!
//! [`crate::pipeline::publish`] has no path to this module: a failed run is resumed or abandoned,
//! never cleaned up by yanking.

use std::fmt;
use std::path::PathBuf;

use semver::Version;

use crate::cargo::Cargo;
use crate::ledger::{Event, EventKind, Ledger, LedgerError};
use crate::registry::Clock;

/// Why a version is being yanked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonKind {
    /// A vulnerability.
    Security,
    /// It does not build for consumers.
    BrokenBuild,
    /// The published contents are not what was meant.
    WrongContents,
    /// A licence problem.
    Licence,
    /// Anything else; needs a longer explanation.
    Other,
}

/// Minimum text length for [`ReasonKind::Other`].
pub const OTHER_MIN_CHARS: usize = 40;

/// Why a yank was refused or failed.
#[derive(Debug)]
pub enum YankError {
    /// The reason is not `kind:text` with a known kind and non-blank text.
    BadReason(String),
    /// `other` needs at least [`OTHER_MIN_CHARS`] characters of explanation.
    OtherTooShort,
    /// The ledger has no record that this tool published and read back that version.
    NotInLedger(String),
    /// The ledger failed.
    Ledger(LedgerError),
    /// `cargo yank` failed (the attempt is in the ledger).
    Cargo(String),
}

impl fmt::Display for YankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadReason(s) => write!(
                f,
                "reason must be `kind:text` with kind one of security, broken-build, wrong-contents, licence, other: {s:?}"
            ),
            Self::OtherTooShort => write!(
                f,
                "an `other` reason needs at least {OTHER_MIN_CHARS} characters of explanation"
            ),
            Self::NotInLedger(s) => write!(f, "{s} is not recorded as published by this tool"),
            Self::Ledger(e) => write!(f, "{e}"),
            Self::Cargo(e) => write!(f, "cargo yank failed: {e}"),
        }
    }
}

impl std::error::Error for YankError {}

impl From<LedgerError> for YankError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e)
    }
}

/// Parse `kind:text`.
///
/// # Errors
/// [`YankError::BadReason`] or [`YankError::OtherTooShort`].
pub fn parse_reason(s: &str) -> Result<(ReasonKind, String), YankError> {
    let (k, text) = s
        .split_once(':')
        .ok_or_else(|| YankError::BadReason(s.to_owned()))?;
    let text = text.trim();
    let kind = match k.trim() {
        "security" => ReasonKind::Security,
        "broken-build" => ReasonKind::BrokenBuild,
        "wrong-contents" => ReasonKind::WrongContents,
        "licence" => ReasonKind::Licence,
        "other" => ReasonKind::Other,
        _ => return Err(YankError::BadReason(s.to_owned())),
    };
    if text.is_empty() {
        return Err(YankError::BadReason(s.to_owned()));
    }
    if kind == ReasonKind::Other && text.chars().count() < OTHER_MIN_CHARS {
        return Err(YankError::OtherTooShort);
    }
    Ok((kind, text.to_owned()))
}

/// Performs the yank.
pub trait Yanker {
    /// Yank `name@version`.
    ///
    /// # Errors
    /// The failure text.
    fn yank(&mut self, name: &str, version: &Version) -> Result<(), String>;
}

/// `cargo yank`, run in `dir`.
pub struct CargoYank {
    /// The cargo to use.
    pub cargo: Cargo,
    /// Where to run it.
    pub dir: PathBuf,
}

impl Yanker for CargoYank {
    fn yank(&mut self, name: &str, version: &Version) -> Result<(), String> {
        let v = version.to_string();
        let out = self
            .cargo
            .run(&self.dir, &["yank", "--version", &v, name])
            .map_err(|e| e.to_string())?;
        if out.success { Ok(()) } else { Err(out.text) }
    }
}

/// What a yank request did.
#[derive(Debug, PartialEq, Eq)]
pub enum YankOutcome {
    /// Dry-run: nothing was written or yanked.
    WouldYank,
    /// Yanked, with the event written first.
    Yanked,
}

/// Yank `name@version` if the ledger shows this tool published and read it back.
///
/// Without `execute` nothing is written. With it, the `Yanked` event (kind and reason) is on disk
/// **before** `cargo yank` runs.
///
/// # Errors
/// See [`YankError`].
pub fn yank(
    ledger: &Ledger,
    clock: &mut dyn Clock,
    yanker: &mut dyn Yanker,
    name: &str,
    version: &Version,
    reason: &str,
    execute: bool,
) -> Result<YankOutcome, YankError> {
    let (kind, text) = parse_reason(reason)?;
    let v = version.to_string();
    let published = ledger
        .read()?
        .into_iter()
        .rev()
        .find(|e| {
            e.event == EventKind::Visible
                && e.crate_name.as_deref() == Some(name)
                && e.version.as_deref() == Some(v.as_str())
        })
        .ok_or_else(|| YankError::NotInLedger(format!("{name}@{v}")))?;
    if !execute {
        return Ok(YankOutcome::WouldYank);
    }
    let event = |kind_of: EventKind, detail: String, clock: &mut dyn Clock| Event {
        ts: clock.now_utc(),
        run: "yank".to_owned(),
        sha: published.sha.clone(),
        crate_name: Some(name.to_owned()),
        version: Some(v.clone()),
        event: kind_of,
        detail,
    };
    ledger.append(&event(
        EventKind::Yanked,
        format!("{kind:?}: {text}"),
        clock,
    ))?;
    match yanker.yank(name, version) {
        Ok(()) => Ok(YankOutcome::Yanked),
        Err(why) => {
            ledger.append(&event(
                EventKind::Classified,
                format!("yank failed: {why}"),
                clock,
            ))?;
            Err(YankError::Cargo(why))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::registry::fake::FakeClock;

    struct Recording {
        calls: Rc<RefCell<Vec<String>>>,
        ledger_path: PathBuf,
        saw_event_first: Rc<RefCell<Vec<bool>>>,
        result: Result<(), String>,
    }

    impl Yanker for Recording {
        fn yank(&mut self, name: &str, version: &Version) -> Result<(), String> {
            let text = std::fs::read_to_string(&self.ledger_path).unwrap();
            self.saw_event_first.borrow_mut().push(
                text.lines()
                    .any(|l| l.contains("\"yanked\"") && l.contains(name)),
            );
            self.calls.borrow_mut().push(format!("{name}@{version}"));
            self.result.clone()
        }
    }

    struct Env {
        _d: tempfile::TempDir,
        ledger: Ledger,
        y: Recording,
        calls: Rc<RefCell<Vec<String>>>,
        first: Rc<RefCell<Vec<bool>>>,
    }

    fn env(published: bool, result: Result<(), String>) -> Env {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("l.jsonl");
        let ledger = Ledger::open(&path).unwrap();
        if published {
            ledger
                .append(&Event {
                    ts: "t".into(),
                    run: "r".into(),
                    sha: "a".repeat(40),
                    crate_name: Some("x".into()),
                    version: Some("1.0.0".into()),
                    event: EventKind::Visible,
                    detail: String::new(),
                })
                .unwrap();
        }
        let calls = Rc::new(RefCell::new(Vec::new()));
        let first = Rc::new(RefCell::new(Vec::new()));
        let y = Recording {
            calls: Rc::clone(&calls),
            ledger_path: path,
            saw_event_first: Rc::clone(&first),
            result,
        };
        Env {
            _d: d,
            ledger,
            y,
            calls,
            first,
        }
    }

    fn v() -> Version {
        "1.0.0".parse().unwrap()
    }

    #[test]
    fn reasons_are_a_known_kind_with_text_and_other_needs_more() {
        assert!(parse_reason("security:RUSTSEC-2026-0001").is_ok());
        for bad in [
            "",
            "security",
            "security:",
            "security:   ",
            "nonsense:x",
            "other:too short",
        ] {
            assert!(parse_reason(bad).is_err(), "{bad}");
        }
        assert!(parse_reason(&format!("other:{}", "x".repeat(OTHER_MIN_CHARS))).is_ok());
    }

    // T15: refuses without a reason, for an unrecorded version, and without --execute.
    #[test]
    fn refusals_leave_no_trace_and_never_call_the_yanker() {
        let mut e = env(true, Ok(()));
        let mut clock = FakeClock::default();
        let before = std::fs::read_to_string(e.ledger.path()).unwrap();

        assert!(matches!(
            yank(&e.ledger, &mut clock, &mut e.y, "x", &v(), "", true),
            Err(YankError::BadReason(_))
        ));
        assert_eq!(
            yank(
                &e.ledger,
                &mut clock,
                &mut e.y,
                "x",
                &v(),
                "broken-build:no",
                false
            )
            .unwrap(),
            YankOutcome::WouldYank
        );
        let mut other = env(false, Ok(()));
        assert!(matches!(
            yank(
                &other.ledger,
                &mut clock,
                &mut other.y,
                "x",
                &v(),
                "broken-build:no",
                true
            ),
            Err(YankError::NotInLedger(_))
        ));
        assert!(e.calls.borrow().is_empty() && other.calls.borrow().is_empty());
        assert_eq!(std::fs::read_to_string(e.ledger.path()).unwrap(), before);
    }

    #[test]
    fn execute_writes_the_event_before_yanking_and_records_a_failure() {
        let mut e = env(true, Ok(()));
        let mut clock = FakeClock::default();
        assert_eq!(
            yank(
                &e.ledger,
                &mut clock,
                &mut e.y,
                "x",
                &v(),
                "security:RUSTSEC-2026-0001",
                true
            )
            .unwrap(),
            YankOutcome::Yanked
        );
        assert_eq!(*e.calls.borrow(), ["x@1.0.0"]);
        assert_eq!(*e.first.borrow(), [true], "event on disk before the yank");

        let mut f = env(true, Err("no token".into()));
        assert!(matches!(
            yank(
                &f.ledger,
                &mut clock,
                &mut f.y,
                "x",
                &v(),
                "licence:wrong licence file",
                true
            ),
            Err(YankError::Cargo(_))
        ));
        let text = std::fs::read_to_string(f.ledger.path()).unwrap();
        assert!(text.contains("yank failed: no token"), "{text}");
    }
}
