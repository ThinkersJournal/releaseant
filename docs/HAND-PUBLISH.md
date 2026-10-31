# Hand-publish checklist (a crate published with `cargo publish`, then a release run that must not stop on it)

For the PM, and for a lane that has been told to do it. Written 2026-10-31 from the fuel and Vulkane partial publishes
(the PM published some crates by hand with `cargo publish -p`; the first real use is in `docs/FIRST-REAL-USE.md`, the
fuel seam trio in `docs/R5-ADOPTION-DRYRUN.md`, update 1). **Prefer the tool**: `releaseant publish --only NAME --execute`
(0.0.29 and later) does all of this and keeps the ledger itself. Hand-publish only when it cannot (the tool is not
installed, a crate must go out alone before the set is ready).

**Why a ledger line is needed.** Since 0.0.35 a version that is on the registry but that the ledger does not show this
tool uploading from this commit is `HELD`: the run stops, dry runs included, no flag overrides it. The ledger is how
the tool knows a hand publish is ours. A crate published by hand without the line holds every later run for that crate
and version. The line is written by the PM only (README, "Publishing for real").

## Before

1. **Fix one commit.** A full 40-hex SHA that is the tip of origin's default branch (`git ls-remote origin main`).
   Never a branch name, never the working tree. Write the SHA down; every later step uses it.
2. **Make a clean archive**: `git archive <SHA> | tar -x -C <new empty dir>`; run cargo only inside it.
3. **Order**: `releaseant plan --sha <SHA>` is a valid order. Any order that puts a dependency before its dependents
   is valid (the fuel seam trio went types, announce, seam while the plan says announce, types, seam: the only edge is
   types before seam).
4. **Check what is already there**: `releaseant plan --sha <SHA> --registry` marks each crate's registry state. A crate
   that is already on the registry is not published again by hand.

## Publish

5. In the archive, per crate: `cargo package --no-verify -p NAME`, then `sha256sum target/package/NAME-X.Y.Z.crate`.
   Write the hash down **before** uploading.
6. `cargo publish -p NAME --locked` (add `--no-verify` only if you verified the package another way; say which in the
   ledger detail). Wait for it to print `Published` and for the index to show the version.

## After

7. **Read the registry back**: `releaseant readback --sha <SHA>` prints `present (<sha256>)` per crate (the index
   checksum), or `absent`.
8. **Compare** that checksum with the one written down in step 5. Equal: the registry holds the bytes `cargo package`
   makes of this commit. **Not equal, or absent: stop. Add no line.** Tell the lane; do not publish again.
   An equal checksum does not say who uploaded it and was measured only on one machine; cross-machine reproducibility of
   packaged bytes is NOT measured.
9. **Add one line per crate** to the ledger the real run will use (`.releaseant/ledger.jsonl` in the repository, or the
   `--ledger` file). Append only; never edit or delete a line. Exact text, one JSON object per line:

```
{"ts":"<UTC>","run":"manual","sha":"<40-hex>","crate":"NAME","version":"X.Y.Z","event":"visible","detail":"published by hand; checked by <who>: registry index checksum <first 12 hex>... equals sha256 of cargo package of this commit"}
```

   `sha` must be the commit the later run publishes: a line naming another commit does not clear a run at this one
   (unit test `a_ledger_from_another_commit_does_not_vouch_for_a_held_version`). A line vouches for its own crate and
   version only; the next crate needs its own line. `<UTC>` is the time of adding, `<who>` the person, and the detail says
   any flag that differs (for example `--no-verify`).
10. **Prove it clears**, on a copy first if in doubt: `releaseant publish --sha <SHA> --only NAME --ledger <ledger>`
    (dry run) must exit 0 and print `NAME: already published`. Without the line the same command stops with
    `HELD ... ask the PM before going on` and exit 1; that stop is the tool working.
11. **Tag a crate published alone**: `releaseant tag --sha <SHA> --crate NAME --version X.Y.Z --execute` makes the annotated
    `NAME-vX.Y.Z` on that commit, pushes only it and reads it back. The project tag `vX.Y.Z` comes only from the run that
    publishes the whole plan (a project tag after a partial publish would claim more than was published).

## Don't

- Don't add a line for a crate you did not read back and compare, or for another person's publish you have not checked.
- Don't edit, reorder or delete ledger lines; a wrong line is corrected by a later line that says so, and the PM is told.
- Don't run the full release before the lines exist: it will stop at the first held crate, by design.
- Don't use `--allow-not-tip` to get around a stale commit; re-choose the SHA (step 1).
- Don't publish the rest of a half-published set by hand after an `INTERRUPTED` stop; tell the lane first
  (`docs/FIRST-REAL-USE.md`).

## Worked case: the fuel seam trio, 0.32.0 at `59bcbd3d4c99cbc8c8ccdab4db3f5d881c65baee`, 2026-10-31

Hand-published by the PM. Dry run `--only` the three: `HELD` at `fuel-kernel-seam-announce`, the other two
"not attempted", exit 1. Registry index checksums equal `sha256sum` of `cargo package --no-verify` of the same commit
for all three (types `49546dde16b8...`, announce `98217ff5bbe5...`, seam `c3d437a5a35c...`; full values in
`docs/R5-ADOPTION-DRYRUN.md`). Three lines as in step 9 on a copy of the ledger: the same run exits 0, all three
"already published". The real ledger was not touched by the rehearsal.
