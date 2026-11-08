# ReleaseAnt: publishing artifacts (installers and container images), design

Status: design, with slices A1, A2, A4 and A6 built and merged (0.0.61 to 0.0.65; see section 9); A3, A5 and CLI wiring open. Requested by CireSnave 2026-11-08 via the PM `[TASK]`. Extends DESIGN.md
(sections 3, 5, 6, 15, 17); read those first. Every number about a third-party service below is
**recalled, not measured** and is marked `UNVERIFIED`: each is a thing the first rehearsal slice must
measure and record with a date before any code depends on it (portfolio CLAUDE.md section 6).

## 1. What is added, and what is not

A second kind of thing ReleaseAnt can publish, beside crates: a **named artifact** built elsewhere.
Two kinds first.

| Kind | Target | First customers (PM) |
|---|---|---|
| `file` | a GitHub release asset (installer, archive, checksum file) | Humboldt installer |
| `image` | an OCI image in a registry (Docker Hub `thinkersjournal/*`) | Fuel, Unpopped |

WebHostAnt is not a customer (PM, 2026-11-08, Q1): it only verifies checksums and digests.

**The tool does not build and does not hold signing keys.** CI builds. A signing step, if any, is a
separate step behind `with-secret`, and runs BEFORE ReleaseAnt: the tool is handed the final bytes.
This keeps the DESIGN.md section 9 rule (the tool uses credentials the environment already provides;
it never reads, stores or prints one).

## 2. The five rules (the PM's shape, made checkable)

1. **Declare.** Each artifact is named in the manifest with the SHA-256 it must have. No declared hash,
   no upload.
2. **Verify before upload.** The tool hashes the local file (or reads the local image's digest) and
   refuses if it differs from the declaration.
3. **Read back by hash.** After upload the tool fetches what the registry now serves and hashes it. Only
   equal hashes are `visible`. A status code, a listing or an upload exit code is not a read-back
   (DESIGN section 6's "anything weaker is a different word").
4. **Never overwrite.** A name that already exists (release asset, image tag) is never replaced, never
   deleted by the tool, never "fixed". Existing with the same hash and a ledger entry for this SHA is
   `already-published` (resume); existing otherwise is `Held` (DESIGN section 17): the run stops,
   resumable, naming the artifact.
5. **Tag last.** The project tag (`v<version>`) is created only when every crate AND every artifact of
   the plan is `visible` (DESIGN section 15's rule, widened from "crates" to "everything in the plan").

What rule 1 does NOT give (named hole, stated so nobody reads more into it): the declared hash comes
from CI, so the check catches corruption, truncation and a wrong file between CI and the upload; it
does not catch a compromised CI that declares the hash of a bad file. Provenance of the build is a
different mechanism (attestations, signing) and out of scope here.

## 3. Manifest

In the existing `releaseant.toml` at the repository root, read from the commit being published (same
rule as `[metadata]`: unknown keys are errors).

```toml
[[artifact]]
name   = "humboldt-installer-x86_64-windows"   # unique in the plan; becomes the ledger key
kind   = "file"
path   = "dist/humboldt-0.3.1-x86_64.msi"       # relative to the CI output dir given by --artifacts-dir
sha256 = "<64 hex>"                             # or sums = "dist/SHA256SUMS" (see below)
target = "github-release"                       # release for tag v<version> of this repo

[[artifact]]
name   = "fuel-image"
kind   = "image"
source = "dir:dist/fuel-oci"                    # a local OCI layout directory, never a daemon (A4: oci-archive is not pushable by crane)
digest = "sha256:<64 hex>"                      # digest of the manifest (or index) to publish
target = "docker.io/thinkersjournal/fuel"
tags   = ["0.3.1"]                              # tags to point at the digest, after confirmation
platforms = ["linux/amd64", "linux/arm64"]      # must equal the platforms in the index
```

- `sums = "<file>"` reads the hash from a `SHA256SUMS` file CI wrote. The sums file is itself an
  artifact (uploaded and read back like any other) so a consumer can verify downloads.
- The digest or hash is in the manifest **at the commit being published**, so it must be known when
  that commit exists. For a build whose output hash is only known after CI runs, the manifest cannot
  hold it. Two ways, Q2 for the PM: (a) `--artifacts-dir` plus a sums file produced by CI in the same
  run, the sums file's own hash passed on the command line and recorded in the ledger (recommended:
  nothing in git changes per build); (b) hash committed back to the repo (a second commit; rejected,
  it breaks "publish the tip" in DESIGN section 16).
- Plan hash (DESIGN section 5) now includes the artifact list, names, kinds and declared hashes, so
  `resume` refuses a changed declaration.

## 4. Per-kind behaviour

### 4.1 `file` to a GitHub release

Recommended mechanism: **upload to a DRAFT release, verify there, publish the release last.**

1. Ask the forge: does a release for the tag exist? Is each asset name already on it?
   (`UNVERIFIED`: a draft release does not create the git tag until it is published, and an asset
   cannot be replaced without deleting it first.)
2. Create the release as a draft (or find our own draft recorded in the ledger).
3. Upload each asset not already present. Read back: download the asset (API `browser_download_url`
   with auth for drafts) and hash it. Equal to the declaration means `visible`.
4. When the whole plan is `visible`: create the annotated tag with the existing `tag` module, then
   undraft the release pointing at that tag. The ordering of undraft and tag is Q3 and a measured
   question: a release created without an existing tag makes a lightweight tag at the default branch's
   head, which is the wrong object (DESIGN section 15 requires an annotated tag on the exact commit).

Why a draft: it is the one reversible stage. A half-uploaded set is invisible to users, so a kill or a
size failure never leaves a public release with missing installers. Nothing in a draft is deleted by
the tool either (rule 4); the PM or CireSnave discards a dead draft by hand.

Failure modes (`UNVERIFIED` numbers, to be measured in slice A3):

| Mode | Handling |
|---|---|
| Asset too large (GitHub documents a per-asset cap, recalled as 2 GiB; a count cap per release, recalled as 1000) | `plan` refuses BEFORE any upload when the local size exceeds the measured cap; classified `Permanent` if the forge refuses anyway |
| Secondary rate limit / 403 or 429 with `retry-after` | `RateLimited`: wait the advised time plus margin, same policy as crates (DESIGN section 4); unbounded retries inside `--max-wait`, then stop resumable |
| Upload connection dropped mid-asset | the forge may keep a half-asset under the name in a non-final state (`UNVERIFIED`). Next attempt sees the name exists: it is NOT treated as present; resolve by state + size + hash. A partial of ours (ledger `upload-started`, no `upload-finished`, state not final) is reported `Held-partial`, and removing it is a recorded, explicit `--discard-partial NAME` the PM asks for, never automatic (rule 4, DESIGN section 7's "a failed run is resumed or abandoned, never auto-cleaned") |
| Name differs only by case/encoding (the forge rewrites names, recalled: spaces become dots) | the manifest name is checked against the forge's returned name; mismatch is an error, not a silent rename |
| API returns 5xx / timeout on lookup | `Unknown`, never "absent" (DESIGN section 5) |
| Asset from a previous run with the same hash | `already-published` only with a ledger entry for this SHA, else `Held` (section 17) |

### 4.2 `image` to an OCI registry (Docker Hub first)

**What a builder must produce (PM ruling, 2026-11-08).** The input is an **OCI image layout
directory**: `oci-layout`, `index.json` with exactly one entry (the image index, or the single image
manifest, to publish), and `blobs/sha256/<hex>` holding that manifest, every child manifest it lists,
and their configs and layers, each blob named by its own SHA-256 (the tool re-hashes them and refuses a
layout where one is not). It is declared as `source = "dir:<path>"`, relative to the artifacts
directory. The declared `digest` is the digest of that one top-level manifest and the declared
`platforms` are exactly the platforms it lists (a single-platform image: the one in its config). An
**OCI tar** (`oci-archive:`) is refused at `plan` with the named error `an OCI tar is not accepted
(crane cannot push one)`, because `crane push` of a tarball of a layout fails
(`docs/ARTIFACT-MEASUREMENTS.md` row 8); the builder unpacks it into a directory instead. A layout
`crane pull --format oci` wrote is known to be accepted (measurements rows 2 to 4); other builders'
layouts have not been tried.

The tool moves an image **by digest** and never talks to a Docker daemon. Mechanism candidates (Q4):
shell out to `skopeo` or `crane` (recommended: both push an OCI layout to a registry and print the
digest; both exist as single binaries), or implement the OCI distribution API directly (more code, no
external tool, more to get wrong). The choice is made by measuring both against a local `registry:2`
in slice A4; until then the design is written against the OCI distribution spec's operations only
(HEAD/GET manifest by digest, push blobs, PUT manifest by digest, PUT manifest by tag).

Order, per image:

1. **Pre-check** every tag in `tags`: `HEAD /manifests/<tag>`. A tag that exists and resolves to a
   different digest is `Held` and stops BEFORE any push. A tag that exists at the declared digest is
   `already-published`, subject to the ledger rule.
2. **Push by digest** (no tag): blobs, then each child manifest, then the index. Pushing by digest
   changes nothing a user can pull by name, which makes it the reversible stage.
3. **Read back**: `GET /manifests/<digest>` and hash the returned bytes; they must equal the declared
   digest. For an index, also read every child manifest by digest and confirm the platform set equals
   `platforms` (a missing arm64 child is the classic half-published multi-arch image).
4. **Promote (last)**: PUT the manifest under each tag, then read the tag back and confirm it resolves
   to the declared digest. This is after the crates and every other artifact are `visible`
   (rule 5): a tag is the user-visible commit point of an image.

Multi-platform: the declared digest is the **index** digest. Children are pushed before the index so
the index never references a child the registry lacks. `plan` lists the child digests it will push.
Per-platform tags (`0.3.1-amd64`) are not created unless declared; a tool that invents tags invents
claims.

Failure modes (`UNVERIFIED`, measure in A4 and A5):

| Mode | Handling |
|---|---|
| Docker Hub pull/push rate limits (documented per-user/IP pull limits exist; push-side limits and 429 wording to be captured) | `RateLimited` with advised wait; the real response text is captured as a fixture before the classifier may name it (DESIGN section 4: unverified patterns fall to `Unknown`) |
| Large layers: upload timeouts, chunked-upload sessions expiring | blobs are resumable by digest: ask `HEAD /blobs/<digest>` and re-push only absent ones; never assume a blob is present from the ledger |
| Registry-side size/layer limits | `Permanent`; recorded with the response body |
| Tag immutability: Docker Hub tags are mutable unless the repo is configured otherwise (`UNVERIFIED`, check the current setting) | the tool's pre-check plus the read-back after promote is the only guard; the race between pre-check and PUT is a **named hole** (another writer can tag in between). The PUT is not conditional on the old state in the OCI spec |
| Digest mismatch on read-back (registry re-encoded the manifest) | `failed`: print both digests; never retagged |
| Auth: expired bearer token mid-push | refresh and retry the blob, `Transient`; repeated 401 is `Permanent` |
| Image index lists a platform not in `platforms`, or the reverse | refused at `plan`, before any push |

## 5. Ledger

DESIGN section 5's format, same file, new fields and events. One event per line, `fsync`ed first.

- New events: `artifact-planned`, `artifact-verified` (local hash equals declaration),
  `artifact-upload-started` (with `target` and, for a file, the release id), `artifact-upload-finished`,
  `artifact-visible` (read-back hash equal), `artifact-held`, `promoted` (image tags, release undraft),
  `artifact-skipped`.
- New fields: `kind` (`crate`|`file`|`image`), `name` (replaces `crate` for artifacts), `sha256`/`digest`
  (declared), `observed` (what the read-back returned), `size`, `target`.
- **Decided in A2 (built, `src/stage.rs`):** to avoid touching every `Event` constructor, an artifact
  event reuses the `crate` field for the artifact's name (no version) and is told apart by its event kind:
  `artifact_verified`, `artifact_upload_started`, `artifact_upload_finished`, `artifact_visible`,
  `artifact_held`, `artifact_failed`. The separate `kind`/`name`/`observed` fields above are not added; the
  hashes go in `detail` (`sha256=<hex>`). `ours` (the Held rule) looks only at `artifact_upload_started` and
  `artifact_visible` for this SHA, so a crate event can never vouch for an artifact of the same name.
  A2 stages `file` artifacts only; `promoted` and the release tag are A6.
- Old readers: events of an unknown `kind` are an error naming the version that wrote them, not
  ignored (a ledger that old code half-understands would skip artifacts silently).
- The terminal states of the per-crate machine (DESIGN section 3) carry over: `pending -> verified ->
  uploaded -> visible`, plus for images `-> promoted`. A run is `success` only when every item is
  `visible` (and images `promoted`) with hashes equal.

## 6. Resume after a kill between upload and read-back

The rule from DESIGN section 5 stands: an `artifact-upload-started` with no `artifact-upload-finished`
is resolved by **asking the target**, never by assuming. Concretely:

| Kill point | Resume action |
|---|---|
| file: after the asset POST returned, before read-back | list the draft's assets; find our name; if its state is final and a download hashes equal: write `visible`. If hash differs: `Held` (stop). If present but not final: `Held-partial` (section 4.1) |
| file: kill mid-upload | as above; the half-asset is the only case that needs a human |
| image: kill after blobs, before manifest | `HEAD` each blob and the manifest by digest; push only what is absent |
| image: kill after the digest PUT, before read-back | `GET` by digest and hash; equal means `visible` |
| image: kill after promote PUT, before tag read-back | read the tag; at the declared digest means `promoted`; at another digest means `Held`, never a retag |
| any: kill after all `visible`, before tag | `resume` runs only the tag step (existing behaviour for crates) |
| any: ledger lost | the target is the truth: no ledger entry for this SHA makes every pre-existing name `Held` (section 17's rule), so a lost ledger is a stop, not an overwrite |

Test for each row: a fake forge or registry that dies at exactly that point, with a control run that
does not die and reaches `success`.

## 7. Order of a mixed release

```
plan -> verify crates -> stage artifacts (draft release, image pushed by digest, NOTHING public)
     -> publish crates -> read back crates
     -> read back artifacts (already done at stage; re-read, hashes again)
     -> promote (image tags) -> annotated tag -> undraft release
```

Staging before publishing crates is a deliberate choice: crates.io is irreversible, the staged
artifacts are not, so everything that can fail cheaply (size, auth, rate limit, a held name) fails
BEFORE the irreversible step. The cost: artifacts sit staged while crates publish, which for a
44-crate set can take hours (docs/R5-LIVE.md), and a rate-limit wait on the crates side leaves a
staged draft for that long. Alternative (crates first, artifacts after) is simpler but leaves a
published crate set with no installer if the artifact stage fails; Q5 for the PM. `--artifacts-only`
and `--crates-only` selectors (like `--only` today) keep both orders available.

## 8. Credentials and permissions

- GitHub: the tool shells out to the `gh` CLI the environment already authenticates (no token read).
  It acts as whichever account `gh` is on; lanes must not switch accounts (portfolio CLAUDE.md
  section 0), so a real release is run by the PM, as crate publishes are today.
- Docker Hub: `docker login` credentials or `skopeo`/`crane` auth files the environment already holds;
  the tool never reads or logs them. Org-write rights for `thinkersjournal/*` are a PM question (Q6).
- Signing: out of scope (section 1).

## 9. Slices (each a PR, each with a negative control)

| Slice | Delivers | Negative control |
|---|---|---|
| A0 | this note reviewed by the PM; Q1-Q6 answered (Q6 still open) | n/a |
| A1 | `[[artifact]]` parsing, validation, plan output, plan-hash change; dry run only | a manifest with a bad hash length, duplicate name or unknown kind is refused; removing the check fails the test |
| A2 | `file` kind against a fake forge: verify, stage, read back, held, resume rows of section 6 | an asset whose served bytes differ from the declaration is NOT `visible` |
| A3 | `file` against a real throwaway GitHub repo; MEASURE the `UNVERIFIED` items of 4.1 and record them in a dated docs section | the measurements are the control; anything unmeasured stays labelled |
| A4 | `image` against a local `registry:2`: pre-check, push by digest, multi-platform read-back, promote, held tag | an index missing one child platform is refused; a tag pointing elsewhere is `Held` and not retagged |
| A5 | `image` against a throwaway Docker Hub repo (PM-run, needs org rights); capture the real 429/error wording as fixtures | as A3 |
| A6 | mixed ordering (section 7), project tag only when crates and artifacts are all visible, `--artifacts-only`/`--crates-only`; ALSO (PM, 2026-11-08) the real-`crane` check against a local `registry:2` (the one run by hand in A4) as a committed test that runs on the self-hosted runner under the same conditions as the other runner tests, never `#[ignore]`d | a run with one artifact `Held` creates no tag even though all crates are visible |

Built status at `origin/main` 20aec7f (0.0.65): A0 done (0.0.60), A1 done (0.0.61), A2 done (0.0.62), A4 done (0.0.63; as built it pushes `dir:` OCI layouts through crane, not a `registry:2` push of a tar), A6 done in two parts (0.0.64 orchestration against fakes, 0.0.65 the runner-only real-crane test). OPEN: A3, A5, and the CLI wiring of `release()` that A6 names (`--artifacts-only` and `--crates-only` are not yet flags).

A patent re-sweep for the new families (artifact read-back by content hash before tagging; staged
draft then promote) is owed before any ReleaseAnt release that ships them, as for earlier families
(ROADMAP R6). The sweep stays INCOMPLETE.

## 10. Questions and the PM's answers (2026-11-08, `[ASK]` over Synapse)

- **Q1 (answered).** WebHostAnt is not a customer: it only verifies checksums and digests. Dropped from section 1.
- **Q2 (answered).** A CI sums file passed per run; no hash committed back to the repo.
- **Q3 (accepted, conditional).** Annotated tag first, then undraft the release onto it, subject to slice A3 confirming the forge allows it.
- **Q4 (answered).** Shell out to `skopeo` or `crane`; one external binary is fine and is reported by `--check-externals` (slice A4 adds it to that list).
- **Q5 (answered).** Stage artifacts BEFORE the irreversible crate step (section 7 as written).
- **Q6 (OPEN, CireSnave's).** Who holds Docker Hub org-write rights for `thinkersjournal/*`, and may a throwaway repo be made for the slice A5 rehearsal? The PM takes it to the board when A5 is next. Slices A1 to A4 need neither.

## 11. Unmeasured, stated once

GitHub per-asset and per-release limits and rate-limit wording; whether a draft release holds no tag;
the state a dropped upload leaves; Docker Hub push limits, 429 wording and tag-immutability setting;
whether `skopeo`/`crane` print a stable digest and exit code on partial push; reproducibility of an
OCI archive across machines (a rebuilt image hashes differently, so the declared digest must come from
the one build that is published, never a rebuild). None of these is quoted as fact anywhere above.
