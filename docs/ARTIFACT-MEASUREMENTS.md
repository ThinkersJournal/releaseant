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
