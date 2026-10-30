//! Publish metadata, checked before anything is written (ROADMAP R6).
//!
//! A multi-crate publish that dies at crate 7 of 13 leaves a half-published set, which is what this
//! tool exists to prevent. So what the registry itself refuses is checked for **every** planned crate
//! first, and all the problems are reported together, each with what to do. The split is by who
//! refuses (PM, 2026-10-29):
//!
//! **Errors**, which stop `plan`, `verify` and `publish` before anything is written: a missing
//! `description`; neither `license` nor `license-file`; a `license-file` or a named `readme` that is
//! not there; more than five `keywords` or one that crates.io refuses (ASCII, at most 20 characters,
//! starting alphanumeric, only letters, digits, `_`, `-` or `+`); more than five `categories` or one
//! that is not a crates.io slug. Cargo already refuses a malformed `rust-version` when it loads the
//! manifest, so that never reaches here. (The rules are the Cargo reference's, "The `keywords` field"
//! and "The `categories` field"; not tested against the registry.)
//!
//! **Warnings**, listed and not stopping anything, because the registry accepts the crate without:
//! no `repository`, no readme, no `keywords`, no `categories`, no `rust-version`. A caller turns
//! chosen ones into errors with [`parse_require`] (`--require-metadata readme,keywords,...`).
//!
//! **Opt-in only**: `homepage` and `documentation` are never warned about, because crates.io links
//! docs.rs and the repository when they are absent and most crates set neither. They can still be
//! named in `--require-metadata` or `releaseant.toml`, and then a missing one is an error.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::categories;
use crate::metadata::{Metadata, Package};
use crate::plan::Plan;

/// Most keywords (and categories) crates.io allows on one crate.
const MOST: usize = 5;
/// Longest keyword crates.io allows.
const LONGEST_KEYWORD: usize = 20;

/// The fields that are only warnings unless asked for, as `--require-metadata` names them.
pub const REQUIRABLE: &[&str] = &[
    "readme",
    "keywords",
    "rust-version",
    "categories",
    "repository",
    "homepage",
    "documentation",
];

/// One thing wrong with one crate's publish metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The crate.
    pub krate: String,
    /// The `Cargo.toml` field.
    pub field: &'static str,
    /// What is wrong and what to do about it.
    pub detail: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.krate, self.field, self.detail)
    }
}

/// What the check found: errors stop the run, warnings are listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Problems that stop the run.
    pub errors: Vec<Problem>,
    /// Problems that are only listed.
    pub warnings: Vec<Problem>,
}

/// The set of warnings to treat as errors, from the names given on the command line or in
/// `releaseant.toml`.
///
/// # Errors
/// A message naming the unknown field and the ones that exist.
pub fn parse_require(names: &[String]) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for name in names {
        let name = name.trim();
        if !REQUIRABLE.contains(&name) {
            return Err(format!(
                "unknown metadata field {name:?}; the fields that can be required are {}",
                REQUIRABLE.join(", ")
            ));
        }
        out.insert(name.to_owned());
    }
    Ok(out)
}

/// Every problem with the publish metadata of every crate in `plan`, in plan order, with the
/// warnings named in `require` counted as errors. Crates outside the plan (excluded, or not
/// publishable) are not looked at. File checks resolve paths against each package's own directory.
#[must_use]
pub fn problems(meta: &Metadata, plan: &Plan, require: &BTreeSet<String>) -> Report {
    let mut report = Report::default();
    for planned in &plan.order {
        if let Some(pkg) = meta.packages.iter().find(|p| p.name == planned.name) {
            check_fields(pkg, require, &mut report);
            check_files(pkg, &mut report);
        }
    }
    report
}

fn blank(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|v| v.trim().is_empty())
}

/// A keyword crates.io accepts: ASCII, at most 20 characters, starting alphanumeric, only letters,
/// digits, `_`, `-` or `+` (Cargo reference, "The `keywords` field").
fn keyword_ok(k: &str) -> bool {
    k.len() <= LONGEST_KEYWORD
        && k.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
}

fn check_fields(pkg: &Package, require: &BTreeSet<String>, report: &mut Report) {
    let krate = pkg.name.clone();
    let mut error = |field: &'static str, detail: String| {
        report.errors.push(Problem {
            krate: krate.clone(),
            field,
            detail,
        });
    };
    if blank(&pkg.description) {
        error(
            "description",
            "missing: add `description = \"...\"` under [package] (crates.io requires it)".into(),
        );
    }
    if blank(&pkg.license) && blank(&pkg.license_file) {
        error(
            "license",
            "missing: add `license = \"MIT OR Apache-2.0\"` (an SPDX expression) or `license-file = \"LICENSE\"` (crates.io requires one)".into(),
        );
    }
    if pkg.keywords.len() > MOST {
        error(
            "keywords",
            format!(
                "{} keywords; crates.io allows at most {MOST}",
                pkg.keywords.len()
            ),
        );
    }
    for k in pkg.keywords.iter().filter(|k| !keyword_ok(k)) {
        error(
            "keywords",
            format!(
                "{k:?} is refused by crates.io (ASCII, at most {LONGEST_KEYWORD} characters, starting with a letter or digit, only letters, digits, `_`, `-` or `+`)"
            ),
        );
    }
    if pkg.categories.len() > MOST {
        error(
            "categories",
            format!(
                "{} categories; crates.io allows at most {MOST}",
                pkg.categories.len()
            ),
        );
    }
    for c in pkg.categories.iter().filter(|c| !categories::is_valid(c)) {
        error(
            "categories",
            format!(
                "`{c}` is not a crates.io category (the slugs are at https://crates.io/category_slugs)"
            ),
        );
    }

    // The quality fields the registry accepts without: a warning, or an error when asked for.
    let mut soft = |field: &'static str, missing: bool, detail: &str| {
        if !missing {
            return;
        }
        if require.contains(field) {
            report.errors.push(Problem {
                krate: pkg.name.clone(),
                field,
                detail: format!("{detail} (required by --require-metadata or releaseant.toml)"),
            });
        } else {
            report.warnings.push(Problem {
                krate: pkg.name.clone(),
                field,
                detail: detail.to_owned(),
            });
        }
    };
    soft(
        "repository",
        blank(&pkg.repository),
        "missing: add `repository = \"https://...\"` under [package]",
    );
    soft(
        "readme",
        blank(&pkg.readme),
        "no README: put a README.md next to the Cargo.toml (cargo picks it up) or set `readme = \"path\"`",
    );
    soft(
        "keywords",
        pkg.keywords.is_empty(),
        "missing: add 1 to 5 `keywords = [\"...\"]` under [package]",
    );
    soft(
        "categories",
        pkg.categories.is_empty(),
        "missing: add `categories = [\"...\"]` (slugs at https://crates.io/category_slugs)",
    );
    soft(
        "rust-version",
        blank(&pkg.rust_version),
        "missing: add `rust-version = \"1.88\"` (the oldest Rust the crate builds with) under [package]",
    );

    // Opt-in fields: never a warning, an error only when asked for.
    for (field, missing, detail) in [
        (
            "homepage",
            blank(&pkg.homepage),
            "missing: add `homepage = \"https://...\"` under [package]",
        ),
        (
            "documentation",
            blank(&pkg.documentation),
            "missing: add `documentation = \"https://...\"` under [package]",
        ),
    ] {
        if missing && require.contains(field) {
            report.errors.push(Problem {
                krate: pkg.name.clone(),
                field,
                detail: format!("{detail} (required by --require-metadata or releaseant.toml)"),
            });
        }
    }
}

/// The readme and the licence file, when the manifest names them, must exist in the package directory
/// (a named file that is not there is an error; no readme at all is a warning, above). Skipped when
/// the manifest's own path is unknown (it is always known for real `cargo metadata` output).
fn check_files(pkg: &Package, report: &mut Report) {
    let Some(dir) = pkg
        .manifest_path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    else {
        return;
    };
    let mut missing = |field: &'static str, rel: &str| {
        if !Path::new(dir).join(rel).is_file() {
            report.errors.push(Problem {
                krate: pkg.name.clone(),
                field,
                detail: format!(
                    "`{field} = \"{rel}\"` names a file that is not in the package directory"
                ),
            });
        }
    };
    if let Some(readme) = pkg.readme.as_deref().filter(|r| !r.trim().is_empty()) {
        missing("readme", readme);
    }
    if let Some(file) = pkg.license_file.as_deref().filter(|f| !f.trim().is_empty()) {
        missing("license-file", file);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::plan::plan;

    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    fn check_with(packages: Vec<Package>, require: &BTreeSet<String>) -> Report {
        let m = Metadata { packages };
        problems(&m, &plan(&m).unwrap(), require)
    }

    fn check(packages: Vec<Package>) -> Report {
        check_with(packages, &none())
    }

    fn fields(ps: &[Problem]) -> Vec<(&str, &'static str)> {
        ps.iter().map(|p| (p.krate.as_str(), p.field)).collect()
    }

    fn pkg() -> Package {
        Package::test("a", "1.0.0", vec![])
    }

    // The control for every test below: a crate with everything is fine, with nothing to warn about.
    #[test]
    fn a_crate_with_complete_metadata_has_no_problems_and_no_warnings() {
        assert_eq!(check(vec![pkg()]), Report::default());
    }

    // The mutation that started this row: deleting `description` must still fail.
    #[test]
    fn a_missing_or_blank_description_is_an_error_that_says_what_to_add() {
        for bad in [None, Some("   ".to_owned())] {
            let mut p = pkg();
            p.description = bad;
            let r = check(vec![p]);
            assert_eq!(fields(&r.errors), [("a", "description")]);
            assert!(
                r.errors[0].detail.contains("description = "),
                "{}",
                r.errors[0]
            );
            assert!(r.warnings.is_empty());
        }
    }

    #[test]
    fn a_licence_is_a_license_or_a_license_file_and_neither_is_an_error() {
        let mut none_at_all = pkg();
        none_at_all.license = None;
        assert_eq!(fields(&check(vec![none_at_all]).errors), [("a", "license")]);

        let mut file_only = pkg();
        file_only.license = None;
        file_only.license_file = Some("LICENSE".into());
        // The file's own existence is the file check's business, not the field check's.
        assert_eq!(check(vec![file_only]), Report::default());

        let mut blank = pkg();
        blank.license = Some(String::new());
        assert_eq!(fields(&check(vec![blank]).errors), [("a", "license")]);
    }

    // What the registry accepts without is a warning: listed, never stopping anything.
    #[test]
    fn quality_fields_the_registry_accepts_without_are_warnings() {
        let mut p = pkg();
        p.repository = None;
        p.readme = None;
        p.keywords = vec![];
        p.categories = vec![];
        p.rust_version = None;
        let r = check(vec![p]);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(
            fields(&r.warnings),
            [
                ("a", "repository"),
                ("a", "readme"),
                ("a", "keywords"),
                ("a", "categories"),
                ("a", "rust-version")
            ]
        );
    }

    // `--require-metadata` turns chosen warnings into errors, and only those.
    #[test]
    fn required_fields_turn_their_warnings_into_errors_and_leave_the_rest() {
        let mut p = pkg();
        p.readme = None;
        p.keywords = vec![];
        p.rust_version = None;
        let require: BTreeSet<String> = ["keywords".to_owned(), "rust-version".to_owned()].into();
        let r = check_with(vec![p.clone()], &require);
        assert_eq!(
            fields(&r.errors),
            [("a", "keywords"), ("a", "rust-version")]
        );
        assert_eq!(fields(&r.warnings), [("a", "readme")]);
        // Control: required but present is no problem at all.
        let r = check_with(vec![pkg()], &require);
        assert_eq!(r, Report::default());
        // And nothing required: the same crate has only warnings.
        assert!(check(vec![p]).errors.is_empty());
    }

    #[test]
    fn homepage_and_documentation_are_never_warnings_and_errors_only_when_required() {
        let mut p = pkg();
        p.homepage = None;
        p.documentation = Some(String::new());
        // Not required: nothing at all, not even a warning.
        assert_eq!(check(vec![p.clone()]), Report::default());
        // Required: both are errors.
        let require: BTreeSet<String> = ["homepage".to_owned(), "documentation".to_owned()].into();
        let r = check_with(vec![p], &require);
        assert_eq!(
            fields(&r.errors),
            [("a", "homepage"), ("a", "documentation")]
        );
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        // Control: required and present is no problem.
        assert_eq!(check_with(vec![pkg()], &require), Report::default());
    }

    #[test]
    fn require_names_are_checked_and_the_unknown_one_is_named() {
        let ok = parse_require(&["readme".into(), "rust-version".into()]).unwrap();
        assert_eq!(
            ok,
            BTreeSet::from(["readme".to_owned(), "rust-version".to_owned()])
        );
        let err = parse_require(&["readme".into(), "licence".into()]).unwrap_err();
        assert!(err.contains("licence"), "{err}");
        for f in REQUIRABLE {
            assert!(err.contains(f), "lists {f}: {err}");
        }
        assert_eq!(parse_require(&[]).unwrap(), none());
    }

    #[test]
    fn keywords_over_five_or_refused_by_crates_io_are_errors() {
        let with = |k: &[&str]| {
            let mut p = pkg();
            p.keywords = k.iter().map(|s| (*s).to_owned()).collect();
            check(vec![p])
        };
        assert_eq!(
            with(&["a", "b", "c", "d", "e"]),
            Report::default(),
            "five is the limit"
        );
        let six = with(&["a", "b", "c", "d", "e", "f"]);
        assert_eq!(fields(&six.errors), [("a", "keywords")]);
        assert!(
            six.errors[0].detail.contains("at most 5"),
            "{}",
            six.errors[0]
        );
        // The characters crates.io allows: ASCII letters (either case), digits, `_`, `-`, `+`.
        assert_eq!(
            with(&["Fine-kw", "with_us", "c++", "x9"]),
            Report::default()
        );
        for bad in [
            "has space",
            "bang!",
            "ünï",
            "-leading",
            "_leading",
            "+leading",
            "",
            " ",
        ] {
            let r = with(&["ok", bad]);
            assert_eq!(fields(&r.errors), [("a", "keywords")], "{bad:?}");
            assert!(
                r.errors[0].detail.contains(&format!("{bad:?}")),
                "{}",
                r.errors[0]
            );
        }
        assert_eq!(
            with(&["abcdefghijklmnopqrst"]),
            Report::default(),
            "20 characters is fine"
        );
        assert_eq!(
            fields(&with(&["abcdefghijklmnopqrstu"]).errors),
            [("a", "keywords")],
            "21 is not"
        );
    }

    #[test]
    fn categories_over_five_or_not_crates_io_slugs_are_errors() {
        let with = |c: &[&str]| {
            let mut p = pkg();
            p.categories = c.iter().map(|s| (*s).to_owned()).collect();
            check(vec![p])
        };
        assert_eq!(
            with(&["development-tools", "aerospace::drones"]),
            Report::default()
        );
        let bad = with(&["development-tools", "not-a-real-category"]);
        assert_eq!(fields(&bad.errors), [("a", "categories")]);
        assert!(
            bad.errors[0].detail.contains("not-a-real-category"),
            "{}",
            bad.errors[0]
        );
        let six = with(&[
            "accessibility",
            "aerospace",
            "algorithms",
            "api-bindings",
            "asynchronous",
            "authentication",
        ]);
        assert_eq!(fields(&six.errors), [("a", "categories")]);
    }

    #[test]
    fn every_problem_of_every_planned_crate_is_reported_in_plan_order() {
        let mut a = Package::test("a", "1.0.0", vec![]);
        a.description = None;
        a.rust_version = None;
        let mut b = Package::test("b", "1.0.0", vec![]);
        b.license = None;
        b.keywords = vec!["has space".into()];
        let r = check(vec![b, a]);
        assert_eq!(
            fields(&r.errors),
            [("a", "description"), ("b", "license"), ("b", "keywords")]
        );
        assert_eq!(fields(&r.warnings), [("a", "rust-version")]);
    }

    #[test]
    fn crates_outside_the_plan_are_not_looked_at() {
        let mut private = Package::test("private", "1.0.0", vec![]);
        private.publish = Some(vec![]);
        private.description = None;
        let m = Metadata {
            packages: vec![Package::test("a", "1.0.0", vec![]), private],
        };
        assert_eq!(problems(&m, &plan(&m).unwrap(), &none()), Report::default());
        // And an excluded crate is cut from the plan before the check, so it is not looked at either.
        let mut held = Package::test("held", "1.0.0", vec![]);
        held.description = None;
        let m = Metadata {
            packages: vec![Package::test("a", "1.0.0", vec![]), held],
        };
        let cut = plan(&m)
            .unwrap()
            .without(&BTreeSet::from(["held".to_owned()]));
        assert_eq!(problems(&m, &cut, &none()), Report::default());
        // Control: not excluded, the same crate is an error.
        assert_eq!(
            fields(&problems(&m, &plan(&m).unwrap(), &none()).errors),
            [("held", "description")]
        );
    }

    #[test]
    fn a_readme_or_license_file_that_is_named_but_not_there_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = pkg();
        p.manifest_path = dir.path().join("Cargo.toml");
        p.readme = Some("NOPE.md".into());
        p.license = None;
        p.license_file = Some("NOLICENSE".into());
        let r = check(vec![p.clone()]);
        assert_eq!(fields(&r.errors), [("a", "readme"), ("a", "license-file")]);
        assert!(r.errors[0].detail.contains("NOPE.md"), "{}", r.errors[0]);

        // Control: the same package once both files exist.
        fs::write(dir.path().join("NOPE.md"), "x").unwrap();
        fs::write(dir.path().join("NOLICENSE"), "x").unwrap();
        assert_eq!(check(vec![p]), Report::default());
    }
}
