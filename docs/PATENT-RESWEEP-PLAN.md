# Patent re-sweep: the plan (nothing in it has been run)

Prepared 2026-10-31 for the PM (`[TASK]` of 2026-10-31). **No search was made and no patent was read to write this.**
It exists so the PM's "go" is one word: on "go" the releaseant lane runs exactly this, in this order, and records the
result where section 7 says. Not legal advice; not a freedom-to-operate opinion; there is no attorney.

Why it is owed: ROADMAP R6 and `docs/PATENT-SWEEP.md` say no releaseant release before a real re-sweep of the
capability families added since the 43 reads that the PM accepted on 2026-10-30. The seventh pass (0.0.32) ran three
queries, read three claim 1s, and is recorded as low confidence. This plan replaces it for the families below.

## 1. Ideas to sweep (mechanisms, as `docs/PATENT-SWEEP.md` lists ideas 1 to 9)

10. A run is limited to named crates (`--only`); `plan --registry` marks each crate's registry state beside the order
    (already swept in the seventh pass: re-swept here).
11. A partial run tags each published crate `<crate>-v<version>`; the project tag `v<version>` only when the run
    published the whole plan (re-swept here).
12. **Held refusal (new):** a version already on the registry that the tool's own ledger does not show it uploading
    from this commit stops the run, dry runs included; the only way on is a manually written ledger line that names the
    crate, version and commit, written after the registry's checksum was compared with a local package of that commit.
13. **Tip check (new):** an upload from a commit that is not the tip of origin's default branch is refused, the tip
    being asked of origin itself (an unreachable origin is a refusal); an override flag exists.

## 2. Queries (exact text; three per family, twelve in all)

Source for every query: the lane's web search restricted to `patents.google.com` and `patents.justia.com`, as in the
second and seventh passes (US-centric; that is a limit, see section 8). Queries are numbered from Q20 (Q1 to Q19 are
taken in `docs/PATENT-SWEEP.md`).

| # | Idea | Query |
|---|---|---|
| Q20 | 10 | `publish selected subset of software packages of a multi-package release, require that the dependencies of the subset are already available in the package registry` |
| Q21 | 10 | `release plan lists each software package with whether that package version is already published in the package repository` |
| Q22 | 10 | `selectively release one component of a monorepo to a package registry while the other components remain unreleased` |
| Q23 | 11 | `create version control tag naming package and version when a package is published, release tag for the whole project only when every package of the release plan is published` |
| Q24 | 11 | `automatically create annotated tag in source control after verifying the published package is visible in the registry index, read the tag back from the remote repository` |
| Q25 | 11 | `per-package release tags in a multi-package repository created by a publishing tool, tag names derived from package name and version` |
| Q26 | 12 | `before uploading a package version check the registry for the same version and stop the release when it exists but the release log has no record of uploading it` |
| Q27 | 12 | `detect that a software package version was published outside the release tool by comparing registry state with the tool's log of uploads, halt and require operator approval to continue` |
| Q28 | 12 | `operator adds log entry attesting an externally published package after its checksum was compared with a locally built package, to let an automated release resume` |
| Q29 | 13 | `refuse to publish a software release from a commit that is not the tip of the default branch of the remote repository` |
| Q30 | 13 | `before releasing software verify that the commit to be released is the latest commit of the remote main branch by querying the remote repository, refuse when the remote cannot be reached` |
| Q31 | 13 | `release gate compares revision identifier to head of protected branch on the origin server before uploading artifacts to a package repository` |

**Positive controls (run first, before Q20):**
1. Re-run Q14 of the second pass; it must again return `US20120324417A1` or `US6847970B2` (it did on 2026-10-29, and Q15 and
   Q17 returned `US6847970B2` too). If it does not, the search tool changed: stop and tell the PM, do not run on.
2. For every query, record how many `patents.google.com` or `patents.justia.com` pages came back (ten expected). A query
   that returns none is a null about the tool, not about the world, and is reported as such, never as "nothing found".
Also say, beside every null, that the control proves the filter reaches patents and not that the query asks the only
possible question (the lesson of the first passes).

## 3. Which results get a claim read

A result is read if its title or snippet is about **software release, publication or distribution of packages or
artifacts, a package registry or repository, release tagging or versioning in source control, or a release gate**.
Physical tags, installers for a single machine, and unrelated "repository" senses are listed by number and skipped,
with the reason. Order: most relevant first, **cap 4 per family (16)**, plus the carry-over list below. If a family
has more than 4 candidates the rest are named as "seen, not read" (never as clear).

Carry-over from the seventh pass (its reads were claim 1 only, first 3,500 characters): **read all independent claims,
in full, of US9235402B2, US8739152B2 and US9043767B2** (hits 44 to 46). That closes the "later independent claims were NOT
read" gap for them.

How to read: `python C:\Projects\tools\patent-claims.py <number>` prints the claims in full without truncation (Google
Patents spells the number). Read **every independent claim in full**, not claim 1 only. For a granted application read both
the application's and the grant's claims (the grant is the narrower). Record title, assignee, status as Google Patents
shows it (an estimate, not the official record), claims read, overlap, our difference, verdict. Machine-converted text:
quote nothing from it without checking the official record.

## 4. Verdicts and the wording rule

- The only verdict words for a read patent are **"outside as read"** (every independent claim needs at least one
  element we do not have, named) or **"needs review"** (every element is plausibly ours, or the reading was not
  possible). For an expired, lapsed, abandoned or withdrawn patent write the status beside it, e.g. "outside as read
  (expired)"; the status is Google's estimate.
- **Never "clear".** Not for a patent, a family, a query, nor the sweep. "Nothing found" is said as "this query returned
  nothing relevant" with its control, and a null on a family is "found nothing, with low confidence that anything could
  have been found" unless the family got reads.
- Anything "needs review" is sent to the PM at once as `[HOLD]`-grade (`[FINDING]`), the run stops reading, and nothing is
  published; it goes in the sweep's "Needs review (to the PM)" section.
- No legal conclusion of any kind: no "does not infringe", no "free to operate", no "invalid".

## 5. Expected cost (the lane's estimates, **unmeasured**, tokens)

| step | estimate |
|---|---|
| controls + 12 queries (about 10 result lines each, plus the lane's notes) | 60-120k |
| up to 16 new reads plus 3 carry-over reads (full independent claims, about 6-12k each) | 120-230k |
| recording (new section, status line, ROADMAP, CHANGELOG, PM message) | 20-40k |
| **total** | **200-390k; ceiling 400k** |

Past estimates for single routes were 100-300k (`docs/PATENT-SWEEP.md`, "Routes not run"); the seventh pass used far
less because it read only claim 1 of three patents. If the run reaches 400k before the reads finish it stops, records
what was read and what was not, and asks the PM. These are not metered figures; THROTTLE.md decides whether to run at
all (full speed assumed). Sub-agents are not needed; if the PM wants the reads fanned out, he says so in his "go".

## 6. What "go" means

"go" = this plan as written: sections 2 to 4, Google Patents and Justia only, Sonnet, run by the lane, recorded as in
section 7, **no release in the same step** (the release is a second word from the PM). Routes A, C, D and E in
`docs/PATENT-SWEEP.md` ("Routes not run") and a Patentscope pass stay unrun unless the PM names them in the same
message: each adds the estimate shown there. The first sentence of the lane's answer to "go" says which routes it ran.

## 7. Where it is recorded

1. `docs/PATENT-SWEEP.md`: a new section **"Eighth pass: re-sweep of the partial-publish, tagging, held and tip-check
   families, <date>"** after the seventh pass: ideas 12 and 13 added to the idea list, queries Q20 to Q31 with the
   controls and the count returned, a hits table continuing at **47** (the carry-over reads update hits 44 to 46 in
   place, saying what changed), "seen, not read", new design constraints, and what the pass is worth, in the style of the
   sixth and seventh passes. The status line at the top is updated; the seventh pass's "treat ideas 10 and 11 as having
   found nothing" is replaced, not left under a caveat.
2. ROADMAP R6: the "owed before any releaseant release" sentence is rewritten to say what was done, its date and ref, and
   that the sweep is still INCOMPLETE.
3. CHANGELOG entry (docs only; version from the PM at gate), HANDOFF rewritten, `[FINDING]` to the PM with the counts and
   any "needs review".

## 8. Limits that stay

AI-assisted search and read; no attorney. US-centric general web search restricted to two patent sites; no boolean or
CPC search; status and expiry are Google's estimates; claim text is machine-converted; family members and continuations
are not chased unless a read patent names one that looks closer; open-source tools that do these things (Lerna,
Changesets, release-please, Cargo's own `cargo publish` skip logic) are practice, not patents, and no invalidity analysis
is made. A re-sweep is owed again when a new capability family is added and at least yearly. The sweep stays
INCOMPLETE after this run; the PM decides whether that is enough for a release.
