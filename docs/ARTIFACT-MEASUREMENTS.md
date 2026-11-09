# Artifact measurements

What was actually observed, with date, tool version and what it was run against. Companion to
`docs/ARTIFACTS-DESIGN.md`, whose `UNVERIFIED` marks are cleared here only when a row below says so.
Nothing in this file is a claim about Docker Hub or GitHub: every row names its target.

## A4, 2026-11-08: `crane` against a local `registry:2`

- **Tool:** `crane` 0.22.1 (`google/go-containerregistry` v0.22.1, Windows x86_64), installed by the
  PM at `C:/Projects/.claude-hooks/tools-crane/crane.exe`. Checked here: the tarball's SHA-256 equals
  its line in `checksums.txt`, and the unpacked `crane.exe` equals the installed one
  (`e22e4720...c935d`). Both files came from one download, so that shows no corruption, not
  independent provenance.
- **Target:** Docker's `registry:2` image (pulled 2026-11-08, version not recorded) as a container on
  `127.0.0.1:5055`, plain HTTP. **This is the reference distribution registry, not Docker Hub.**
  Nothing here says what Docker Hub does.
- **Fixture:** a two-platform index built with crane only (no daemon): two one-file layers
  (`crane append`), `--set-platform linux/amd64` and `linux/arm64` (`crane mutate`), then
  `crane index append`. Index digest `sha256:db0d0c14...271b`, 639 bytes, children 423 bytes each.

| # | Question | Observed |
|---|---|---|
| 1 | Does `crane manifest repo@digest` return the exact stored bytes? | Yes: its stdout (no trailing newline) hashes to the digest; the blob in the layout hashes to the same value. So "hash the bytes the registry serves and compare to the declared digest" is implementable with crane. |
| 2 | Can a layout be exported with no daemon? | `crane pull --format oci <ref> <dir>` writes `oci-layout`, `index.json` and `blobs/sha256/*`; `index.json` has one entry, the index's digest. |
| 3 | Can crane push a layout BY DIGEST with no tag? | Yes: `crane push <dir> <host>/<repo>@sha256:<index digest>` pushed both child manifests and the index by digest. The repo then had no tags (`crane ls` says `NAME_UNKNOWN`) but `GET /v2/<repo>/manifests/<digest>` returned 200 with `Docker-Content-Digest`. This is the staging step of design section 4.2. |
| 4 | Push by tag | `crane push <dir> <repo>:1.0.0` pushes children by digest, then the index under the tag. The tag read back to the declared digest. |
| 5 | What does "absent" look like? | `HEAD` of an unknown tag: `404`. `crane digest`: exit 1 and `MANIFEST_UNKNOWN: manifest unknown`; for an unknown repository `NAME_UNKNOWN: repository name not known to registry`. |
| 6 | What does "could not ask" look like? | Nothing listening: exit 1, `dial tcp 127.0.0.1:5999: connectex: No connection could be made because the target machine actively refused it`. No `MANIFEST_UNKNOWN`, so it can be told from absent. |
| 7 | Are tags mutable? | Yes on this registry: `crane tag <repo>@<other digest> 2.0.0` moved an existing tag to a different image with exit 0 and no warning. So the design's named hole (a writer between pre-check and PUT) is real here, and the pre-check plus read-back is the only guard. |
| 8 | Can crane push an OCI-layout TARBALL? | No: `tar -cf x.tar -C <layout> .` then `crane push x.tar ...` fails with `loading x.tar as tarball: file manifest.json not found in tar` (it wants a docker-save tarball). So `source = "oci-archive:..."` cannot be pushed with crane; `skopeo` can, but has no Windows build. **Decision (A4): only `dir:` sources are accepted.** |

### Not measured (still `UNVERIFIED` in the design)

Docker Hub in every respect: push and pull rate limits and their 429 wording, whether a bare
unknown repository answers `NAME_UNKNOWN` or `UNAUTHORIZED` there, tag immutability settings, layer
and size limits; large-layer upload behaviour and timeouts; `crane`'s behaviour when a push dies
midway (the exit status and what is left in the registry); pushing an index whose declared reference
digest differs from the layout's; crane's own retry behaviour. Slice A5 measures the Docker Hub ones.

### A4 end to end, 2026-11-08: `images::Crane` against the same local `registry:2`

A throwaway test (not committed: it needs Docker and `crane`, which CI does not have; the committed
tests use `FakeImages` and the captured crane outputs above) ran `stage_images`, `promote_images` and
the registry calls through the real `Crane` on the fixture of the table above. Observed:

- staging a fresh repository: `Visible`, and the tag `1.0.0` was then still absent (nothing pullable
  by name);
- staging again: `AlreadyPublished` with no second push (the digest was found by asking);
- promoting `1.0.0` and `latest`: `Visible`; `latest` then resolved to the declared digest;
- a tag already pointing at another digest (set with `crane tag`): `Held`, "nothing was pushed or
  moved";
- a registry with nothing listening: the run stopped with crane's `actively refused` text (not read as
  absent). The whole test took 110 s; where the time went was not measured (the unreachable-port
  step is a suspect, unconfirmed), so no per-failure latency is claimed.

Not run for real: a push that dies midway, a layout whose child blob the registry refuses, a registry
that answers `UNAUTHORIZED`. Those paths are covered only by the fake and by `classify_digest`'s
fail-safe default.

## A3, 2026-11-08: GitHub releases through `gh` (`src/gh_forge.rs`)

- **Target:** the standing PRIVATE repository `ciresnave-bot/releaseant-artifact-rehearsal` (created
  2026-11-08 on CireSnave's word; one README commit so a tag has something to point at). **This is
  GitHub.com as reached by the `ciresnave-bot` account, nothing else.** Calls were made with `gh api`
  by hand first (rows 1 to 8), then by `GhForge` in `tests/forge_runner.rs` (rows 9 to 10).
- **Cleanup:** after each run only that run's release and tag were deleted; read back: 0 releases and
  0 tags remained, the asset id answered 404, `refs/heads/main` still existed (the control). The repo
  itself cannot be deleted (no `delete_repo` scope), so it stays as the standing rehearsal target.

| # | Question | Observed |
|---|---|---|
| 1 | Does a draft release create the git tag? | No. After `POST releases` with `draft=true` for a tag that did not exist, `GET git/ref/tags/<tag>` was 404. |
| 2 | Is a draft found by tag? | No: `GET releases/tags/<tag>` was 404 for a draft. It is found by listing `GET releases` (drafts included for a writer) and matching `tag_name`. `GhForge` therefore lists. |
| 3 | Name rewriting | Uploading `m 2.txt` (query `name=m%202.txt`) stored the asset as `m.2.txt`: spaces become dots. The response carries the stored name; `GhForge` refuses a mismatch and says the asset now exists under the forge's name. |
| 4 | Same name twice | `HTTP 422`, `errors:[{resource:ReleaseAsset, code:already_exists, field:name}]`. Never replaced. |
| 5 | What does the API serve for an asset? | `state: uploaded`, `size`, and a `digest: sha256:...` field that equalled the local file's SHA-256. `GhForge` does NOT trust that field: it downloads the bytes (`Accept: application/octet-stream` on `releases/assets/<id>`, works for a draft) and hashes them; that hash also equalled the file's. |
| 6 | Annotated tag first, then undraft (design Q3) | Allowed. `POST git/tags` + `POST git/refs` for `refs/tags/<tag>`, then `PATCH releases/<id> draft=false`: the release published and the tag ref was still `object.type == "tag"` (annotated), same object sha. The ruling holds on a real forge. |
| 7 | By-tag lookup after publish | `GET releases/tags/<tag>` then returns it (`draft:false`). |
| 8 | Upload with `gh api --input -` and a file as stdin | `HTTP 400 "Bad Content-Length"`. `--input <path>` works. `GhForge` passes the path. |
| 9 | Does the listing see a draft immediately after the create? | **Not always.** In 5 create-then-list-8-times runs, 2 had the first listing miss the draft (`0` then `1` on every later read); the other 3 showed it at once. Fetching by id was always right. So a miss in the listing right after our own create is not "absent": `GhForge` remembers the id it created and asks for it by id when the listing lacks the tag. A release this process did not create is only ever looked up by listing, so a stale miss there would still read as absent: not measured beyond the five runs, and the draft release it would then create is caught by `find_release` refusing two releases with one tag. |
| 10 | End to end | `tests/forge_runner.rs` (create draft, upload, read back by hash, second upload refused, name rewrite refused, annotated tag, undraft, tag still annotated) passed 3 of 3 times after the id fix (about 26 to 37 s each). Before the fix it failed once on row 9. Three passes cannot show the lag fix works, because the lag itself is intermittent (2 in 5); no mutation check of that line was done. |

### Still `UNVERIFIED` after A3

Per-asset size cap and per-release asset count cap (recalled as 2 GiB and 1000, not measured: a
2 GiB upload was not attempted); secondary rate limit and 429/403 wording and `retry-after`; what a
dropped upload leaves behind (state of the partial asset); whether a published asset can be deleted
or replaced; behaviour on a public repository (this one is private; visibility of drafts to others
not tested); whether `gh` retries on its own.

## A5, 2026-11-09: Docker Hub (`thinkersjournal/releaseant-a5-test`), runbook and first finding

**Status: slice A5 is OPEN.** Only the first row below has been observed; everything under "Runbook" is
a plan for the PM to run, and none of its outcomes is known. Nothing in the runbook is a claim about
Docker Hub until a dated row records what it printed.

### Target and who runs what

- **Target:** `thinkersjournal/releaseant-a5-test` on Docker Hub, created by CireSnave as a PRIVATE
  throwaway repository; he deletes the whole repository when A5 is done. **Never touch
  `thinkersjournal/synapse`** (a real public repository: the Hub API lists it as the only public
  `thinkersjournal` repository on 2026-11-09).
- **Who pushes:** the PM, with the `ciresnave` Docker Desktop login. The ReleaseAnt lane's session cannot
  push (its permission layer refuses a push to a Hub surface, even a private one) and never handles the
  credentials. The lane writes the commands; the PM runs them and pastes the raw output back.
- **Tool:** `crane` 0.22.1 at `C:/Projects/.claude-hooks/tools-crane/crane.exe` (see A4 above).
  `docker.io` references below are written as `thinkersjournal/releaseant-a5-test`; crane resolves them
  to Docker Hub.

### First finding (reported by the PM, 2026-11-09; not re-run by the lane)

| # | Question | Observed |
|---|---|---|
| 1 | Can the `ciresnave` login push to `thinkersjournal/*`? | Yes, per the PM's own run: `docker push thinkersjournal/releaseant-a5-test:a5-1` (a retagged `hello-world`) succeeded with digest `sha256:d1a8d0a4eeb63aff09f5f34d4d80505e0ba81905f36158cc3970d8e07179e59e`; an authenticated `crane digest` of the same tag printed the same digest; the anonymous Hub API answered 404 for the repository (so it is private). This is one push of one single-platform image through `docker`, not through `crane`, and says nothing yet about index pushes, push by digest, or limits. |

### Runbook (PM runs; every step against the throwaway repository only)

Record for each step: the exact command, the exit status, and stdout and stderr verbatim. If a step
prints something unexpected, paste it unedited and stop; do not retry in a loop. `R` below is
`thinkersjournal/releaseant-a5-test`.

1. **Known tag reads back.** `crane digest R:a5-1`. Expect the digest in row 1. Confirms crane's auth
   path reads a private Hub repository.
2. **Absent tag.** `crane digest R:no-such-tag`. Record the exit status and the exact error code text
   (A4 saw `MANIFEST_UNKNOWN` on `registry:2`; whether Hub says the same is unknown).
3. **Unknown repository.** `crane digest thinkersjournal/releaseant-a5-does-not-exist:x`. Record whether
   Hub answers `NAME_UNKNOWN` or `UNAUTHORIZED` (the design lists this as `UNVERIFIED`; the classifier
   must not treat an unknown code as absent).
4. **Build the A4 fixture without a daemon** (two one-file layers with `crane append`, platforms set
   with `crane mutate --set-platform linux/amd64` and `linux/arm64`, then `crane index append`), as in
   the A4 section, and export it with `crane pull --format oci <ref> <dir>` or build it as a layout.
   Record the index digest `D`.
5. **Push by digest, no tag.** `crane push <dir> R@D`. Record whether Hub accepts a digest-only push
   (A4 row 3 saw `registry:2` accept it). Then `crane ls R` and `crane digest R@D`: is the index
   readable by digest while no tag names it? Record all output.
6. **Manifest bytes hash to the digest.** `crane manifest R@D` hashed with SHA-256 must equal `D`
   (A4 row 1). Record the hash and the digest.
7. **Push by tag.** `crane push <dir> R:a5-2`, then `crane digest R:a5-2`: equals `D`?
8. **Tag mutability.** `crane tag R@<digest of a5-1> a5-2` (a different image) and then
   `crane digest R:a5-2`. Record whether Hub moved the tag silently (A4 row 7 on `registry:2`) or
   refused. Do this on the throwaway only.
9. **Tag-immutability setting.** Read the repository's settings page on Hub and copy what it shows for
   tag immutability (name of the setting, current value, whether it can be changed for this plan).
   No guessing: if the page does not show it, write that.
10. **Rate limits and 429 wording.** Do NOT provoke a limit. Record only a 429 or `TOOMANYREQUESTS`
    text if one appears verbatim during the steps above, with the command that drew it. If none
    appears, the row stays "not observed".
11. **Optional: the lane's own code path.** The uncommitted A4 end-to-end test that drives
    `images::Crane` (`stage_images`, `promote_images`, the held-tag case) could be pointed at `R`
    instead of `127.0.0.1:5055`; this needs the lane to build it and the PM to supply the login, so it
    is a separate request, not part of this runbook.

### What this runbook does not cover

A push that dies midway, layer and size limits, large-layer timeouts, crane's retry behaviour and
`UNAUTHORIZED` from an expired login stay `UNVERIFIED`; they are not cheap or safe to provoke on a shared
Hub account.
