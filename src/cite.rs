//! `cite-update`: after a minor (or pre-1.0 breaking) bump is published and read back, find every place
//! that still cites the old `X.Y` of the project's own crates, and produce the edit as a patch.
//!
//! It runs at publish time and never at stamp time: the harness self-test compares with the version the
//! registry serves, so a citation moved at the stamp makes it fail. It changes files only with `apply`
//! (a worktree, never `origin/main`); by default it prints a unified diff.
//!
//! What counts as a citation (the project's own crate names come from its manifests, so `serde = "0.27"`
//! is never touched): a dependency requirement (`name = "X.Y"`, `version = "X.Y"` on a line naming an own
//! crate, or in an own `[dependencies.name]` table), `name@X.Y`, and a `docs.rs/name/X.Y` link, in
//! `Cargo.toml`, in Markdown and in Rust doc comments (`//!`, `///`). A Rust string literal is code and is
//! left alone. Refused, listed and left alone: history (`CHANGELOG*`, `changelog.d/`, `docs/design/`),
//! a Markdown quote (`>`), a line that says it is narrating (`was`, `previously`, `->`), a range
//! requirement, and a pin to a patch version (that is `tools/stamp-extra.txt`'s).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What to do.
pub struct Request<'a> {
    /// Tree to read (a `git archive` copy, or a worktree).
    pub root: &'a Path,
    /// The `X.Y` being left, e.g. `0.27`.
    pub old: &'a str,
    /// The `X.Y` being moved to, e.g. `0.28`.
    pub new: &'a str,
    /// The version the registry served before the bump (its `X.Y` must be `old`).
    pub served: &'a str,
    /// Write the files (otherwise only report).
    pub apply: bool,
}

/// One place that cites the old `X.Y` and was not changed.
#[derive(Debug, Clone)]
pub struct Refusal {
    /// Path relative to the root, `/` separated.
    pub file: String,
    /// 1-based line.
    pub line: usize,
    /// Why.
    pub why: &'static str,
}

/// The result of a run.
#[derive(Debug)]
pub struct Report {
    /// Unified diff of every change (empty when none).
    pub patch: String,
    /// Files that change (relative paths).
    pub changed: Vec<String>,
    /// Citations seen and refused.
    pub refused: Vec<Refusal>,
    /// How the files were listed: `git ls-files` or `directory walk`.
    pub listing: &'static str,
    /// The own crate names found.
    pub names: BTreeSet<String>,
    /// Own crates at the new `X.Y`.
    pub at_new: usize,
}

/// Why nothing was done.
#[derive(Debug)]
pub enum CiteError {
    /// The inputs disagree with each other or with the tree: exit 3, nothing written.
    Mismatch(String),
    /// Reading or writing failed.
    Io(String),
}

impl std::fmt::Display for CiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mismatch(s) | Self::Io(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for CiteError {}

fn xy(v: &str) -> Option<String> {
    let mut p = v.split(['.', '-', '+']);
    let (a, b) = (p.next()?, p.next()?);
    (a.parse::<u64>().is_ok() && b.parse::<u64>().is_ok()).then(|| format!("{a}.{b}"))
}

/// Enumerate from the index when the root is a git tree; otherwise walk (and say so).
fn list_files(root: &Path) -> Result<(Vec<String>, &'static str), CiteError> {
    if root.join(".git").exists() {
        let o = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "-z"])
            .output()
            .map_err(|e| CiteError::Io(format!("git ls-files: {e}")))?;
        if o.status.success() {
            let v = String::from_utf8_lossy(&o.stdout)
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            return Ok((v, "git ls-files"));
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)
        .map_err(|e| CiteError::Io(format!("walk {}: {e}", root.display())))?;
    out.sort();
    Ok((out, "directory walk"))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if p.is_dir() {
            if name != ".git" && name != "target" {
                walk(root, &p, out)?;
            }
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Own crate names and their versions, from every `Cargo.toml` with a `[package]`.
fn own_crates(root: &Path, files: &[String]) -> Vec<(String, Option<String>)> {
    let ws_version = std::fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| {
            t.get("workspace")?
                .get("package")?
                .get("version")?
                .as_str()
                .map(str::to_owned)
        });
    let mut out = Vec::new();
    for f in files
        .iter()
        .filter(|f| f.rsplit('/').next() == Some("Cargo.toml"))
    {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        let Ok(t) = text.parse::<toml::Table>() else {
            continue;
        };
        let Some(p) = t.get("package") else { continue };
        let Some(name) = p.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let ver = match p.get("version") {
            Some(v) if v.is_str() => v.as_str().map(str::to_owned),
            Some(_) => ws_version.clone(),
            None => None,
        };
        out.push((name.to_owned(), ver));
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Manifest,
    Markdown,
    RustDoc,
}

fn kind_of(path: &str) -> Option<Kind> {
    let base = path.rsplit('/').next().unwrap_or(path);
    if base == "Cargo.toml" {
        Some(Kind::Manifest)
    } else if base.to_ascii_lowercase().ends_with(".md") {
        Some(Kind::Markdown)
    } else if base.ends_with(".rs") {
        Some(Kind::RustDoc)
    } else {
        None
    }
}

fn is_history(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    let base = p.rsplit('/').next().unwrap_or(&p);
    ["changelog", "history", "releases", "news"]
        .iter()
        .any(|h| base.starts_with(h))
        || p.starts_with("changelog.d/")
        || p.contains("/changelog.d/")
        || p.starts_with("docs/design/")
        || p.starts_with(".lane-state/")
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Does `line` contain `name` as a whole token?
fn has_token(line: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(i) = line[from..].find(name) {
        let s = from + i;
        let e = s + name.len();
        let before = line[..s].chars().next_back();
        let after = line[e..].chars().next();
        if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) {
            return true;
        }
        from = s + 1;
    }
    false
}

/// The identifier that ends `s` (after trimming blanks and a single `=`), e.g. `vulkane` in `vulkane = `.
fn ident_before_eq(s: &str) -> Option<&str> {
    let t = s.trim_end();
    let t = t.strip_suffix('=')?.trim_end();
    let start = t.rfind(|c: char| !is_ident(c)).map_or(0, |i| i + 1);
    (start < t.len()).then(|| &t[start..])
}

const NARRATIVE: [&str; 5] = ["was", "were", "formerly", "previously", "until"];

fn narrates(line: &str) -> bool {
    line.contains("->")
        || line.contains('\u{2192}')
        || line
            .split(|c: char| !c.is_ascii_alphabetic())
            .any(|w| NARRATIVE.contains(&w.to_ascii_lowercase().as_str()))
}

enum Hit {
    Replace(usize, usize),
    Refuse(&'static str),
}

/// Every citation of `old` on one line.
fn scan_line(
    line: &str,
    old: &str,
    names: &BTreeSet<String>,
    table_dep_own: bool,
    kind: Kind,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    let line_has_own = names.iter().any(|n| has_token(line, n));
    let mut from = 0;
    while let Some(i) = line[from..].find(old) {
        let s = from + i;
        let e = s + old.len();
        from = e;
        let before = line[..s].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        let rest = &line[e..];
        let mut r = rest.chars();
        let patch = matches!((r.next(), r.next()), (Some('.'), Some(c)) if c.is_ascii_digit());
        if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let tail_end = if rest.starts_with(".x") || rest.starts_with(".*") {
            e + 2
        } else {
            e
        };
        let head = &line[..s];
        // The shape in front of the number.
        let (op_len, op) = match head.chars().next_back() {
            Some(c @ ('^' | '~')) => (1, Some(c)),
            Some(c @ ('>' | '<' | '=')) => (1, Some(c)),
            _ => (0, None),
        };
        let qpos = s - op_len;
        let in_quote = line[..qpos].ends_with('"') && line[tail_end..].starts_with('"');
        let shape_ok = if in_quote {
            let before_quote = &line[..qpos - 1];
            match ident_before_eq(before_quote) {
                Some("version") => line_has_own || table_dep_own,
                Some(name) => names.contains(name),
                None => false,
            }
        } else if let Some(n) = head.strip_suffix('@') {
            let start = n.rfind(|c: char| !is_ident(c)).map_or(0, |k| k + 1);
            names.contains(&n[start..])
        } else if let (Some(n), None) = (head.strip_suffix('/'), op) {
            let start = n.rfind(|c: char| !is_ident(c)).map_or(0, |k| k + 1);
            names.contains(&n[start..]) && head.contains("docs.rs/")
        } else {
            false
        };
        if !shape_ok {
            continue;
        }
        if kind != Kind::Manifest && narrates(line) {
            hits.push(Hit::Refuse("narrative line (was, previously, ->)"));
        } else if kind != Kind::Manifest && line.trim_start().starts_with('>') {
            hits.push(Hit::Refuse("quoted text"));
        } else if matches!(op, Some('>' | '<' | '=')) {
            hits.push(Hit::Refuse("range or exact requirement"));
        } else if patch {
            hits.push(Hit::Refuse(
                "pinned to a patch version (tools/stamp-extra.txt)",
            ));
        } else {
            hits.push(Hit::Replace(s, e));
        }
    }
    hits
}

/// `[dependencies.NAME]`-style header: the own crate it names, if any.
fn table_dep(header: &str, names: &BTreeSet<String>) -> bool {
    let h = header.trim().trim_start_matches('[').trim_end_matches(']');
    let Some(i) = h.find("dependencies.") else {
        return false;
    };
    names.contains(h[i + "dependencies.".len()..].trim_matches('"'))
}

/// Run it.
///
/// # Errors
/// [`CiteError::Mismatch`] when the served version is not on `old`, or no own crate is at `new`
/// (nothing is written); [`CiteError::Io`] when a file cannot be read or written.
pub fn run(req: &Request<'_>) -> Result<Report, CiteError> {
    let (old, new) = (req.old, req.new);
    if xy(old).as_deref() != Some(old) || xy(new).as_deref() != Some(new) {
        return Err(CiteError::Mismatch(format!(
            "--old {old} and --new {new} must each be X.Y"
        )));
    }
    if old == new {
        return Err(CiteError::Mismatch(format!(
            "--old and --new are both {old}: not a minor bump"
        )));
    }
    match xy(req.served) {
        Some(s) if s == old => {}
        other => {
            return Err(CiteError::Mismatch(format!(
                "the registry served {} (X.Y {}) before the bump, not the old {old}: nothing written",
                req.served,
                other.unwrap_or_else(|| "unreadable".into())
            )));
        }
    }
    let (files, listing) = list_files(req.root)?;
    let crates = own_crates(req.root, &files);
    let names: BTreeSet<String> = crates.iter().map(|(n, _)| n.clone()).collect();
    let at_new = crates
        .iter()
        .filter(|(_, v)| v.as_deref().and_then(xy).as_deref() == Some(new))
        .count();
    if at_new == 0 {
        return Err(CiteError::Mismatch(format!(
            "no crate of this tree is at {new}.x ({} crates read): the stamp has not run here, or the wrong tree",
            crates.len()
        )));
    }
    let mut patch = String::new();
    let mut changed = Vec::new();
    let mut refused = Vec::new();
    let mut writes: Vec<(PathBuf, String)> = Vec::new();
    for f in &files {
        let Some(kind) = kind_of(f) else { continue };
        let Ok(bytes) = std::fs::read(req.root.join(f)) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let history = is_history(f);
        let mut out = String::with_capacity(text.len());
        let mut any = false;
        let mut table_own = false;
        for (n, raw) in text.split_inclusive('\n').enumerate() {
            let body = raw.trim_end_matches(['\n', '\r']);
            let eol = &raw[body.len()..];
            if kind == Kind::Manifest && body.trim_start().starts_with('[') {
                table_own = table_dep(body, &names);
            }
            let scan = match kind {
                Kind::RustDoc => {
                    let t = body.trim_start();
                    t.starts_with("//!") || t.starts_with("///")
                }
                _ => true,
            };
            let mut line = body.to_owned();
            if scan && body.contains(old) {
                let mut hits = scan_line(body, old, &names, table_own, kind);
                // Replace from the right so earlier offsets stay valid.
                hits.reverse();
                for h in hits {
                    match h {
                        Hit::Refuse(why) => refused.push(Refusal {
                            file: f.clone(),
                            line: n + 1,
                            why,
                        }),
                        Hit::Replace(..) if history => refused.push(Refusal {
                            file: f.clone(),
                            line: n + 1,
                            why: "historical file",
                        }),
                        Hit::Replace(s, e) => line.replace_range(s..e, new),
                    }
                }
            }
            any |= line != body;
            out.push_str(&line);
            out.push_str(eol);
        }
        if any {
            patch.push_str(&unified(f, &text, &out));
            changed.push(f.clone());
            writes.push((req.root.join(f), out));
        }
    }
    if req.apply {
        for (p, t) in &writes {
            std::fs::write(p, t)
                .map_err(|e| CiteError::Io(format!("write {}: {e}", p.display())))?;
        }
    }
    Ok(Report {
        patch,
        changed,
        refused,
        listing,
        names,
        at_new,
    })
}

/// A unified diff with three lines of context for a same-line-count rewrite of `a` into `b`.
fn unified(path: &str, a: &str, b: &str) -> String {
    let al: Vec<&str> = a.split_inclusive('\n').collect();
    let bl: Vec<&str> = b.split_inclusive('\n').collect();
    let diff: Vec<usize> = (0..al.len()).filter(|&i| al[i] != bl[i]).collect();
    let mut s = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n");
    let mut i = 0;
    while i < diff.len() {
        let mut j = i;
        while j + 1 < diff.len() && diff[j + 1] - diff[j] <= 6 {
            j += 1;
        }
        let lo = diff[i].saturating_sub(3);
        let hi = (diff[j] + 3).min(al.len() - 1);
        let _ = writeln!(
            s,
            "@@ -{},{} +{},{} @@",
            lo + 1,
            hi - lo + 1,
            lo + 1,
            hi - lo + 1
        );
        for k in lo..=hi {
            let push = |s: &mut String, sign: char, l: &str| {
                s.push(sign);
                s.push_str(l);
                if !l.ends_with('\n') {
                    s.push_str("\n\\ No newline at end of file\n");
                }
            };
            if al[k] == bl[k] {
                push(&mut s, ' ', al[k]);
            } else {
                push(&mut s, '-', al[k]);
                push(&mut s, '+', bl[k]);
            }
        }
        i = j + 1;
    }
    s
}
