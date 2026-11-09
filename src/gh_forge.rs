//! The real [`Forge`]: GitHub releases through the `gh` CLI (`docs/ARTIFACTS-DESIGN.md` 4.1, slice A3).
//!
//! Every behaviour that shapes this code was measured on a real private repository on 2026-11-08
//! (`docs/ARTIFACT-MEASUREMENTS.md`, "A3"):
//! - a draft release is NOT returned by `GET releases/tags/<tag>` (404), so releases are found by
//!   listing and matching `tag_name`;
//! - a draft creates no git tag; an annotated tag made first, then the undraft, keeps the annotated tag;
//! - GitHub rewrites an uploaded name (spaces become dots), so a returned name that differs from the
//!   requested one is an error, never a silent rename;
//! - a second upload under an existing name is refused with HTTP 422 `already_exists`.
//!
//! The pure parsing is unit tested; the calls themselves run only against a real repository in
//! `tests/forge_runner.rs` (feature `runner-tests`).

use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::stage::{AssetInfo, Forge, Release};

/// GitHub releases of one repository, reached through `gh` (which supplies the credentials).
#[derive(Debug, Clone)]
pub struct GhForge {
    exe: PathBuf,
    repo: String,
    /// Ids of drafts this process created, by tag. The listing can MISS a draft for a moment after it
    /// is created (measured: the first listing right after the create lacked it in 2 of 5 runs), so a
    /// miss in the listing is not "absent" for a release we just made: ask for it by id.
    created: HashMap<String, u64>,
}

/// The id of a release and what the forge says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) id: u64,
    pub(crate) release: Release,
    pub(crate) asset_ids: Vec<(String, u64)>,
}

/// Percent-encode a query value (everything but unreserved characters).
pub(crate) fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Find the release of `tag` in a (possibly concatenated-pages) releases listing.
///
/// `gh api --paginate` prints one JSON array per page, back to back; both that and a single array are
/// accepted. More than one release with the tag is an error: which one is ours is a human's question.
///
/// # Errors
/// Unparseable output, or an ambiguous tag.
pub(crate) fn find_release(listing: &str, tag: &str) -> Result<Option<Found>, String> {
    let mut found: Vec<Found> = Vec::new();
    for page in serde_json::Deserializer::from_str(listing).into_iter::<Value>() {
        let page = page.map_err(|e| format!("releases listing is not JSON: {e}"))?;
        let items = page
            .as_array()
            .ok_or_else(|| "releases listing is not an array".to_owned())?;
        for r in items {
            if r.get("tag_name").and_then(Value::as_str) != Some(tag) {
                continue;
            }
            let id = r
                .get("id")
                .and_then(Value::as_u64)
                .ok_or("release without an id")?;
            let draft = r.get("draft").and_then(Value::as_bool).unwrap_or(false);
            let mut assets = Vec::new();
            let mut asset_ids = Vec::new();
            for a in r
                .get("assets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = a
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("asset without a name")?
                    .to_owned();
                let aid = a
                    .get("id")
                    .and_then(Value::as_u64)
                    .ok_or("asset without an id")?;
                assets.push(AssetInfo {
                    size: a.get("size").and_then(Value::as_u64).unwrap_or(0),
                    complete: a.get("state").and_then(Value::as_str) == Some("uploaded"),
                    name: name.clone(),
                });
                asset_ids.push((name, aid));
            }
            found.push(Found {
                id,
                release: Release { draft, assets },
                asset_ids,
            });
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        n => Err(format!(
            "{n} releases carry the tag {tag}; a human must say which is ours"
        )),
    }
}

/// The name the forge stored, from an upload response, compared with the one asked for.
///
/// # Errors
/// An unparseable response, or a stored name that differs from the requested one (the asset now
/// exists under the forge's name; the message says so).
pub(crate) fn check_stored_name(response: &str, wanted: &str) -> Result<(), String> {
    let v: Value =
        serde_json::from_str(response).map_err(|e| format!("upload response is not JSON: {e}"))?;
    let got = v
        .get("name")
        .and_then(Value::as_str)
        .ok_or("upload response has no name")?;
    if got == wanted {
        Ok(())
    } else {
        Err(format!(
            "the forge stored the asset as {got:?}, not {wanted:?}; it now exists under that name and was not renamed back"
        ))
    }
}

impl GhForge {
    /// Releases of `repo` (`owner/name`), through the `gh` named by `$GH`, else `gh` on the PATH.
    #[must_use]
    pub fn new(repo: &str) -> Self {
        Self {
            exe: std::env::var_os("GH").map_or_else(|| PathBuf::from("gh"), PathBuf::from),
            repo: repo.to_owned(),
            created: HashMap::new(),
        }
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let mut cmd = Command::new(&self.exe);
        cmd.args(args).stdin(Stdio::null());
        let out = cmd
            .output()
            .map_err(|e| format!("{} could not be run: {e}", self.exe.display()))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            // gh prints the API's JSON body on stdout and its summary on stderr: keep both.
            let mut msg = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            let body = String::from_utf8_lossy(&out.stdout);
            if !body.trim().is_empty() {
                if !msg.is_empty() {
                    msg.push_str(" | ");
                }
                msg.push_str(body.trim());
            }
            Err(msg)
        }
    }

    fn text(&self, args: &[&str]) -> Result<String, String> {
        let bytes = self.run(args)?;
        String::from_utf8(bytes).map_err(|e| format!("gh output is not UTF-8: {e}"))
    }

    fn find(&self, tag: &str) -> Result<Option<Found>, String> {
        let path = format!("repos/{}/releases?per_page=100", self.repo);
        let listing = self.text(&["api", "--paginate", &path])?;
        if let Some(found) = find_release(&listing, tag)? {
            return Ok(Some(found));
        }
        match self.created.get(tag) {
            None => Ok(None),
            Some(id) => {
                let one = self.text(&["api", &format!("repos/{}/releases/{id}", self.repo)])?;
                find_release(&format!("[{one}]"), tag)
            }
        }
    }

    fn need(&self, tag: &str) -> Result<Found, String> {
        self.find(tag)?
            .ok_or_else(|| format!("no release for {tag} on {}", self.repo))
    }

    /// Whether `gh` can be run and is signed in, with its first version line.
    ///
    /// # Errors
    /// The reason it cannot.
    pub fn version(&self) -> Result<String, String> {
        let t = self.text(&["--version"])?;
        Ok(t.lines().next().unwrap_or("").trim().to_owned())
    }
}

impl Forge for GhForge {
    fn release(&mut self, tag: &str) -> Result<Option<Release>, String> {
        Ok(self.find(tag)?.map(|f| f.release))
    }

    fn create_draft(&mut self, tag: &str) -> Result<(), String> {
        let path = format!("repos/{}/releases", self.repo);
        let tag_arg = format!("tag_name={tag}");
        let name_arg = format!("name={tag}");
        let body = self.text(&[
            "api",
            "-X",
            "POST",
            &path,
            "-f",
            &tag_arg,
            "-f",
            &name_arg,
            "-F",
            "draft=true",
        ])?;
        let id = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("id").and_then(Value::as_u64))
            .ok_or_else(|| format!("create response has no release id: {body}"))?;
        self.created.insert(tag.to_owned(), id);
        Ok(())
    }

    fn upload_asset(&mut self, tag: &str, name: &str, file: &Path) -> Result<(), String> {
        let id = self.need(tag)?.id;
        let url = format!(
            "https://uploads.github.com/repos/{}/releases/{id}/assets?name={}",
            self.repo,
            encode_query(name)
        );
        // The path, not `--input -` on a file handle: gh answers that with HTTP 400 "Bad Content-Length"
        // (measured 2026-11-08), because it cannot know the length of a stream.
        let file_arg = file.to_string_lossy().into_owned();
        let bytes = self.run(&[
            "api",
            "-X",
            "POST",
            &url,
            "-H",
            "Content-Type: application/octet-stream",
            "--input",
            &file_arg,
        ])?;
        check_stored_name(&String::from_utf8_lossy(&bytes), name)
    }

    fn served_sha256(&mut self, tag: &str, name: &str) -> Result<String, String> {
        let found = self.need(tag)?;
        let aid = found
            .asset_ids
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
            .ok_or_else(|| format!("no asset {name:?} on the release of {tag}"))?;
        let path = format!("repos/{}/releases/assets/{aid}", self.repo);
        let bytes = self.run(&["api", "-H", "Accept: application/octet-stream", &path])?;
        let mut out = String::with_capacity(64);
        for b in Sha256::digest(&bytes) {
            write!(out, "{b:02x}").expect("writing to a String cannot fail");
        }
        Ok(out)
    }

    fn publish_draft(&mut self, tag: &str) -> Result<(), String> {
        let id = self.need(tag)?.id;
        let path = format!("repos/{}/releases/{id}", self.repo);
        self.text(&["api", "-X", "PATCH", &path, "-F", "draft=false"])
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"[{"id":7,"tag_name":"v1","draft":true,"assets":[
        {"id":9,"name":"a.zip","size":12,"state":"uploaded"},
        {"id":10,"name":"b.zip","size":3,"state":"starter"}]},
        {"id":8,"tag_name":"v0","draft":false,"assets":[]}]"#;

    #[test]
    fn finds_a_draft_by_tag_with_asset_state() {
        let f = find_release(ONE, "v1").unwrap().unwrap();
        assert_eq!(f.id, 7);
        assert!(f.release.draft);
        assert_eq!(f.release.assets.len(), 2);
        assert!(f.release.assets[0].complete);
        assert!(
            !f.release.assets[1].complete,
            "a non-`uploaded` state is not complete"
        );
        assert_eq!(f.asset_ids[1], ("b.zip".to_owned(), 10));
    }

    #[test]
    fn no_match_is_none_not_an_error() {
        assert_eq!(find_release(ONE, "v9").unwrap(), None);
        assert_eq!(find_release("[]", "v1").unwrap(), None);
    }

    #[test]
    fn paginated_output_is_one_array_per_page() {
        let two = "[{\"id\":1,\"tag_name\":\"a\",\"draft\":false,\"assets\":[]}]\n\
                   [{\"id\":2,\"tag_name\":\"b\",\"draft\":true,\"assets\":[]}]";
        assert_eq!(find_release(two, "b").unwrap().unwrap().id, 2);
    }

    #[test]
    fn a_tag_on_two_releases_is_an_error() {
        let dup = "[{\"id\":1,\"tag_name\":\"a\",\"assets\":[]},{\"id\":2,\"tag_name\":\"a\",\"assets\":[]}]";
        assert!(find_release(dup, "a").unwrap_err().contains("2 releases"));
    }

    #[test]
    fn garbage_is_an_error_never_none() {
        assert!(find_release("not json", "a").is_err());
        assert!(find_release("{\"message\":\"x\"}", "a").is_err());
    }

    #[test]
    fn a_rewritten_name_is_refused() {
        assert!(check_stored_name(r#"{"name":"m.2.txt"}"#, "m.2.txt").is_ok());
        let e = check_stored_name(r#"{"name":"m.2.txt"}"#, "m 2.txt").unwrap_err();
        assert!(e.contains("not renamed back"), "{e}");
        assert!(check_stored_name("{}", "a").is_err());
    }

    #[test]
    fn query_encoding_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(encode_query("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode_query("m 2&x.txt"), "m%202%26x.txt");
    }
}
