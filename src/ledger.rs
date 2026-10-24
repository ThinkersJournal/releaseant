//! The append-only JSON Lines ledger (DESIGN section 5).
//!
//! The ledger is a record, not the source of truth: whether a crate is published is the registry's
//! answer. Each event is written and synced **before** the action it announces.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A crate is in the plan of this run.
    Planned,
    /// The package was extracted and built.
    Verified,
    /// A recorded exception to a rule (for example skipped verification), with its reason.
    Exception,
    /// An upload is about to start.
    UploadStarted,
    /// An upload returned.
    UploadFinished,
    /// A result was classified.
    Classified,
    /// Read-back saw the crate on the registry.
    Visible,
    /// Read-back timed out without seeing the crate.
    Missing,
    /// The run is waiting (rate limit, index lag).
    Waiting,
    /// The crate was not attempted because a dependency did not succeed.
    Skipped,
    /// The run ended.
    Finished,
}

/// One ledger line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// UTC timestamp text supplied by the caller.
    pub ts: String,
    /// Run identifier.
    pub run: String,
    /// Full commit SHA the run publishes from.
    pub sha: String,
    /// Crate name, when the event concerns one.
    #[serde(rename = "crate", default, skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    /// Crate version, when the event concerns one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The kind of event.
    pub event: EventKind,
    /// Free text: reason, class, plan fingerprint.
    #[serde(default)]
    pub detail: String,
}

/// Ledger failures.
#[derive(Debug)]
pub enum LedgerError {
    /// The file system refused.
    Io(io::Error),
    /// A line other than a torn final line is not a valid event.
    Corrupt {
        /// 1-based line number.
        line: usize,
        /// The parse error text.
        why: String,
    },
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "ledger I/O: {e}"),
            Self::Corrupt { line, why } => write!(f, "ledger line {line} is corrupt: {why}"),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<io::Error> for LedgerError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// An open ledger file.
#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    torn_bytes_dropped: usize,
}

impl Ledger {
    /// Open (creating if needed) the ledger at `path`.
    ///
    /// A final line without a newline is a write the crash interrupted. Its event was never
    /// completed, and events are written before their actions, so the action it announced never
    /// started: the partial line is dropped (its size is reported by [`Self::torn_bytes_dropped`]).
    ///
    /// # Errors
    /// I/O errors, or [`LedgerError::Corrupt`] if a complete line is not a valid event.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, LedgerError> {
        let path = path.into();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let mut dropped = 0;
        if path.exists() {
            let bytes = fs::read(&path)?;
            if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
                let keep = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
                dropped = bytes.len() - keep;
                let f = OpenOptions::new().write(true).open(&path)?;
                f.set_len(keep as u64)?;
                f.sync_all()?;
            }
        } else {
            File::create(&path)?.sync_all()?;
        }
        let ledger = Self {
            path,
            torn_bytes_dropped: dropped,
        };
        ledger.read()?; // refuse to append to a ledger that is already corrupt
        Ok(ledger)
    }

    /// Bytes of a torn final line removed by [`Self::open`].
    #[must_use]
    pub fn torn_bytes_dropped(&self) -> usize {
        self.torn_bytes_dropped
    }

    /// The ledger path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one event and sync it to disk before returning.
    ///
    /// # Errors
    /// I/O errors.
    pub fn append(&self, event: &Event) -> Result<(), LedgerError> {
        let mut line = serde_json::to_string(event).expect("an Event always serializes");
        line.push('\n');
        let mut f = OpenOptions::new().append(true).open(&self.path)?;
        f.write_all(line.as_bytes())?;
        f.sync_data()?;
        Ok(())
    }

    /// Read every event.
    ///
    /// # Errors
    /// I/O errors, or [`LedgerError::Corrupt`] naming the first invalid line.
    pub fn read(&self) -> Result<Vec<Event>, LedgerError> {
        let text = fs::read_to_string(&self.path)?;
        let mut events = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let event = serde_json::from_str(line).map_err(|e| LedgerError::Corrupt {
                line: i + 1,
                why: e.to_string(),
            })?;
            events.push(event);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: EventKind, krate: &str) -> Event {
        Event {
            ts: "2026-10-24T00:00:00Z".into(),
            run: "r1".into(),
            sha: "a".repeat(40),
            crate_name: Some(krate.into()),
            version: Some("1.0.0".into()),
            event: kind,
            detail: String::new(),
        }
    }

    // T16: round trip, torn last line tolerated, corrupt middle line is an error.
    #[test]
    fn round_trip_and_torn_tail_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/ledger.jsonl");
        let l = Ledger::open(&path).unwrap();
        l.append(&ev(EventKind::Planned, "a")).unwrap();
        l.append(&ev(EventKind::UploadStarted, "a")).unwrap();
        // Simulate a crash mid-write of the third line.
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(br#"{"ts":"x","run":"r1","sha":"abc","cra"#)
            .unwrap();
        drop(f);

        let l = Ledger::open(&path).unwrap();
        assert!(l.torn_bytes_dropped() > 0);
        assert_eq!(l.read().unwrap().len(), 2);
        // The ledger stays appendable and readable after the repair.
        l.append(&ev(EventKind::Visible, "a")).unwrap();
        let kinds: Vec<_> = l.read().unwrap().iter().map(|e| e.event).collect();
        assert_eq!(
            kinds,
            [
                EventKind::Planned,
                EventKind::UploadStarted,
                EventKind::Visible
            ]
        );
    }

    #[test]
    fn corrupt_complete_line_is_an_error_naming_the_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let l = Ledger::open(&path).unwrap();
        l.append(&ev(EventKind::Planned, "a")).unwrap();
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"this is not json\n").unwrap();
        drop(f);
        l.append(&ev(EventKind::Planned, "b")).unwrap();
        match Ledger::open(&path) {
            Err(LedgerError::Corrupt { line, .. }) => assert_eq!(line, 2),
            other => panic!("expected Corrupt at line 2, got {other:?}"),
        }
    }
}
