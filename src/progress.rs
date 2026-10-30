//! One line of progress per step of a long run (DESIGN section 13).
//!
//! The lines are a mirror of the ledger: [`observer`] is handed to [`crate::ledger::Ledger::observed_by`]
//! and turns every event it sees into exactly one line, so the lines cannot disagree with the
//! ledger. Elapsed time since the command started is the caller's to add; the per-upload duration
//! is measured here with a monotonic clock and is display only, never recorded.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use crate::ledger::{Event, EventKind, Observer};

/// Receives one line of text per step.
pub type ProgressSink = Rc<dyn Fn(&str)>;

fn subject(e: &Event) -> String {
    match (&e.crate_name, &e.version) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n.clone(),
        _ => String::new(),
    }
}

/// The line for one ledger event. `took` is how long the upload took, for `UploadFinished`.
#[must_use]
pub fn describe(e: &Event, took: Option<u64>) -> String {
    let who = subject(e);
    let d = &e.detail;
    let with = |text: &str| {
        if d.is_empty() {
            text.to_owned()
        } else {
            format!("{text}: {d}")
        }
    };
    match e.event {
        EventKind::Planned => "run planned".to_owned(),
        EventKind::Verified if d.is_empty() => format!("verified {who}"),
        EventKind::Verified => format!("{who}: {d}"),
        EventKind::Exception => format!("{who}: exception, {d}"),
        EventKind::UploadStarted => format!("uploading {who}"),
        EventKind::UploadFinished => match took {
            Some(s) => format!("upload returned {who} ({s} s)"),
            None => format!("upload returned {who}"),
        },
        EventKind::Classified => format!("{who}: {}", with("classified")),
        EventKind::Visible => format!("{who}: read back, visible"),
        EventKind::Missing => format!("{who}: NOT visible after the read-back wait"),
        EventKind::Waiting => with("waiting"),
        EventKind::Skipped => format!("skipped {who} ({d})"),
        EventKind::Finished => format!("run finished: {d}"),
        EventKind::Yanked => format!("yanked {who}: {d}"),
        EventKind::Tagged => format!("tagged {who}: {d}"),
    }
}

/// A sink that writes `[MM:SS] line` to `out`, MM:SS being the time since the sink was made.
///
/// A write that fails (a closed pipe, a reader that exited) is ignored: progress is display only
/// and must never stop a publish between two steps. (`eprintln!` panics on such an error.)
#[must_use]
pub fn writer_sink(out: impl std::io::Write + 'static) -> ProgressSink {
    let started = Instant::now();
    let out = RefCell::new(out);
    Rc::new(move |line: &str| {
        let s = started.elapsed().as_secs();
        let _ = writeln!(out.borrow_mut(), "[{:02}:{:02}] {line}", s / 60, s % 60);
    })
}

/// An observer for a ledger that sends [`describe`]'s line for each event to `sink`.
#[must_use]
pub fn observer(sink: ProgressSink) -> Observer {
    let started: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
    Rc::new(move |e: &Event| {
        let key = subject(e);
        let took = match e.event {
            EventKind::UploadStarted => {
                started.borrow_mut().insert(key, Instant::now());
                None
            }
            EventKind::UploadFinished => started
                .borrow_mut()
                .remove(&key)
                .map(|t| t.elapsed().as_secs()),
            _ => None,
        };
        sink(&describe(e, took));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [EventKind; 13] = [
        EventKind::Planned,
        EventKind::Verified,
        EventKind::Exception,
        EventKind::UploadStarted,
        EventKind::UploadFinished,
        EventKind::Classified,
        EventKind::Visible,
        EventKind::Missing,
        EventKind::Waiting,
        EventKind::Skipped,
        EventKind::Finished,
        EventKind::Yanked,
        EventKind::Tagged,
    ];

    fn ev(kind: EventKind) -> Event {
        Event {
            ts: "t".into(),
            run: "r".into(),
            sha: "s".into(),
            crate_name: Some("foo".into()),
            version: Some("0.1.0".into()),
            event: kind,
            detail: "why".into(),
        }
    }

    // Guards the table above: an event kind added to the ledger without a line fails to compile
    // in `describe`; one missing from ALL fails here.
    #[test]
    fn every_event_kind_has_a_distinct_nonblank_line() {
        let mut seen = std::collections::BTreeSet::new();
        for k in ALL {
            let line = describe(&ev(k), Some(3));
            assert!(!line.trim().is_empty(), "{k:?}");
            assert!(seen.insert(line), "{k:?} repeats another kind's line");
        }
        assert_eq!(seen.len(), 13);
    }

    #[test]
    fn upload_duration_is_paired_by_crate_and_version() {
        let lines: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink: ProgressSink = {
            let lines = lines.clone();
            Rc::new(move |l: &str| lines.borrow_mut().push(l.to_owned()))
        };
        let o = observer(sink);
        o(&ev(EventKind::UploadStarted));
        o(&ev(EventKind::UploadFinished));
        let got = lines.borrow().clone();
        assert_eq!(got[0], "uploading foo 0.1.0");
        assert!(got[1].starts_with("upload returned foo 0.1.0 ("), "{got:?}");
        // A finish with no start (a resumed run) still gets a line, without a duration.
        o(&ev(EventKind::UploadFinished));
        assert_eq!(lines.borrow()[2], "upload returned foo 0.1.0");
    }
}
