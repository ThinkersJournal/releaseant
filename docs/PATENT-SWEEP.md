# Patent sweep: releaseant 0.0.11 @ 5501221

Date: 2026-10-29; Run by: releaseant lane, Sonnet 5.5; Method: AI-assisted web search (US-only), then independent claims read in full with `tools/patent-claims.py` (Google Patents pages); NOT legal advice; NOT a freedom-to-operate opinion. There is no attorney.

**Status: first pass, complete for the routes run; the sweep stays INCOMPLETE until the PM accepts the routes not run as sufficient.** Twelve queries were run (six general web, six restricted to patents.google.com and patents.justia.com). The USPTO full-text scan, Espacenet, WIPO, assignee searches and direct Google Patents query URLs were NOT run. Ideas 6, 7, 8 and 9 had no patent-site query (idea 6 and 7 only the general query Q6; 8 and 9 none). **No hit needs review.** Thirteen patents were read; one more family's continuations and several results were not (listed under "Seen, not read").

## Ideas swept
What ReleaseAnt does that might be new or unusual (mechanisms, not features), from `docs/DESIGN.md` and the code at 5501221:
1. The publish order is computed from Cargo manifests: a dependency counts as an edge when it names a version, in any position (normal, build, dev, optional); a path-only dev-dependency is not an edge; a cycle is an error that names its members.
2. An append-only ledger gets an event before each action; running the same command again resumes, skipping what the registry already holds; a changed plan at the same commit is refused by comparing a fingerprint.
3. After each upload the tool polls the registry's index until the version is visible and compares the index's checksum with the SHA-256 of the file it uploaded.
4. A rate-limit response is read for the server's own retry time; the run waits up to a total budget, then stops in a resumable state.
5. Before any upload every crate is packaged as one set, each `.crate` is extracted and built; a crate that cannot build here is skipped only by a named exception with a reason, and is built as a consumer project after upload.
6. The files come from `git archive` of a full commit SHA (a branch name is refused), independent of the user's line-ending configuration.
7. Before anything is written, a dependency outside the set that the registry cannot satisfy stops the run.
8. A crate can be excluded from a run with a recorded reason; crates that need it must find it on the registry.
9. A yank is allowed only for a version the ledger shows this tool published, with a reason of a recorded kind. (Not searched.)

## Queries (source, query, date 2026-10-29)
General web search, US-only:
| # | Idea | Query |
|---|---|---|
| Q1 | 1 | `patent determining order to publish software packages to a registry from dependency graph topological order publish set release` |
| Q2 | 2 | `patent append-only log written before each action resume interrupted multi-package software release skip already published packages` |
| Q3 | 3 | `patent after uploading software package to repository verify by reading back registry index and comparing checksum of uploaded artifact` |
| Q4 | 4 | `patent rate limit server retry-after time automatically pause and resume bulk upload of packages to package registry` |
| Q5 | 5 | `patent build and test packaged archive extracted from release tarball before publishing to package repository verify contents ship correctly consumer project check` |
| Q6 | 6, 7 | `patent publish packages from source control commit archive instead of working directory dependency not yet available in registry block release` |

Restricted to `patents.google.com` and `patents.justia.com`:
| # | Idea | Query |
|---|---|---|
| Q7 | 1, 2, 3 | `publishing software packages dependency order package registry resume interrupted release verify published` |
| Q8 | 1, 2 | `release orchestration multiple software packages publish in dependency order ledger checkpoint resume after failure` |
| Q9 | 3 | `verify package published to repository by retrieving metadata checksum and comparing with checksum of uploaded package` |
| Q10 | 4, 1 | `rate limited upload to package repository wait indicated time then continue publishing remaining packages dependent packages published first` |
| Q11 | 5 | `build extracted package archive in clean environment before release to repository ensure packaged files sufficient to build` |
| Q12 | 4 | `automatically retry publishing artifact to repository after rate limit response waiting time specified by server, dependency ordered release of multiple artifacts` |

Positive control: Q7 to Q11 returned patent pages on adjacent topics, so the restriction reaches Google Patents and Justia. Q1 to Q6 returned open-source tools and documentation and almost no patents; **0 patents on ideas 3, 4 and 6 is a statement about these queries, not about the world.** Q4 and Q12 found only generic retry/backoff patents, none read.

Open-source prior art seen in the general results (not patents, not relied on for the verdicts): multi-package publish tools that keep a progress file, skip already-published versions or validate before publishing (a Dart `gg_multi_do_publish`, a Rust `cyrup_release`, an npm SDK publish script), `git archive` for source tarballs in the Linux kernel build, and documented `Retry-After` handling in registry and API documentation.

## Hits
Claims read: every independent claim, in full, from Google Patents text (machine-converted; check the official record before quoting). Status is Google Patents' field, not the official record. Where an application was granted, **both** the application's and the grant's claims were read: the grants are narrower, and the Red Hat "Package audit tool" application claim 1 alone would have looked like real overlap.

| # | patent | title | assignee | status | claims read | overlap | our difference | verdict |
|---|---|---|---|---|---|---|---|---|
| 1 | US8978028B2 (application US20090100421A1) | Transactional multi-package installation | Microsoft | Active, est. expiry 2032-04-25 | grant 1, 9, 14; application 1, 10, 15 | several packages handled as one set | every granted independent claim needs each package's actions partitioned into execution, commit and rollback phases, a commit or rollback of the whole transaction, and delegation to a second process tagged with a transaction identifier. We have no commit or rollback phase: an upload is final, and a yank is a separate, manual, recorded step. The broader application claim 15 ("execution phase of each package, then a commit phase of each package") is not what was granted | clear (watch: no "stage all, then commit all or roll back all") |
| 2 | US9916133B2 | Software release workflow management | Microsoft | Active, est. 2033-09-27 | 1, 14, 20 | a release run as a series of instructions that can resume | claims 1, 14 and 20 each require a scalable plurality of orchestrator or workflow-engine instances in a cloud-based framework and a gating rule from a policy schema. ReleaseAnt is one local process with no policy schema and no gating rule | clear (watch: no multi-instance cloud orchestrator, no policy-gated steps) |
| 3 | US8838964B2 (application US20120137138A1) | Package audit tool | Red Hat | Active, est. 2031-11-15 | grant 1, 16, 18; application 1, 18, 21 | user names packages for a release; tool validates them | granted claims also require a requirement specifying a type of external source, a package database that tracks the package after import, and a certification list that raises two different errors when a file differs or is missing from it. We have none of these. The application's claim 1 was much broader (any compliance requirement) | clear (watch: no certification list, no package database of tracked changes) |
| 4 | US9235410B2 (application US20130326481A1) | Tracking software package dependencies using a graph model | Red Hat | Active, est. 2033-01-20 (fees paid 2019, 2023) | grant 1, 7, 13; application 1, 8, 15 | a graph of a release's packages and their dependencies | package and **function** nodes, function dependency information, a second release's graph, and a comparison for an incompatibility under a policy. Our plan graph has package nodes only, one release, no comparison | clear |
| 5 | US11580232B2 | Building a package dependencies data structure | Snyk | Active, est. 2042-07-12 | 1, 7, 13 | graph of package dependencies | a package instance identifier built from the package's and its dependency sub-tree's unique identifiers, with node de-duplication by that identifier. We key by crate name and do not identify sub-trees | clear (watch: no sub-tree identity hashing) |
| 6 | US8615768B2 | Dependency-ordered resource synchronization across multiple environments | Microsoft | Expired, fee related (lapsed) | 1, 11, 17 | topological sort so dependencies go first | joining resource descriptions from several environments and a create/delete/modify change list to make a target environment match a source. We order one set for upload. Also lapsed | clear |
| 7 | US9813450B1 | Metadata-based verification of artifact quality policy compliance | Amazon | Active, est. 2035-11-01 | 1, 6, 16 | checks artifacts in a repository | assigning different priorities to policy-violation detection operations from artifacts' activity-status metadata. We do no prioritised policy scanning | clear |
| 8 | US8856724B2 (application US20120324417A1) | Systems and methods for incremental software development | eBay | Active, est. 2032-03-14 | grant 1, 6, 11; application 1, 9, 16 | none that matters (binary libraries, a repository) | an IDE flow: modify a library's source, notify while editing, rebuild, test, tag with a code version identifier. Not publishing. Continuations US9201632B2 and US10019256B2 not read | clear for this patent; continuations unread |
| 9 | US8949788B2 | Building and packaging software | Red Hat | Active, est. 2033-01-18 | 1, 5, 10 | builds from source-controlled project files | scanning a local plugin repository to record build dependencies in a database. Not ours | clear |
| 10 | US8156086B2 | Systems and methods for stored data verification | Commvault | Expired, fee related | 1, 10 | compares stored metadata with a second metadata to verify a copy | backup copies in a storage-management system with chunk headers and a second storage operation. Also lapsed | clear |
| 11 | US5721824A | Multiple-package installation with package dependencies | Sun Microsystems | Expired (lifetime) | 1, 8, 14 | install order from dependency lists | install-time action list and trailer script. Expired | clear (expired) |
| 12 | US6847970B2 | Methods and apparatus for managing dependencies in distributed systems | IBM | Expired (lifetime) | 1, 31, 41 | relationships between components | very broad claim, expired (est. 2023-05-03) | clear (expired) |
| 13 | EP3748494A1 | A tool for analysing a software distribution package | BlackBerry | Withdrawn | 1 | analyses a built package | compares required and declared device permissions. Withdrawn | clear (withdrawn) |

## Seen, not read (so unread, never clear)
`US10628389B2` (data provenance), `US7451167B2` (verification of log data with per-entry checksums: the only result about verifying a log, by title, so read it first in a second pass), `US20120246618A1`, `US6117187A`, `US20010029605A1`, `US7363633B1`, `US20140245290A1`, `US7013461B2`, `US9141378B2`, `US20100169863A1`, `WO2017195160A1`, `US8370443B2`, and the eBay continuations named above. Family members of the other hits were not looked up. Nothing was found, and nothing read, on ideas 3 (read-back), 4 (server-advised wait), 6, 7, 8 and 9 beyond the generic patents above.

## Design constraints from this sweep (what not to add without a re-sweep)
- No staged "commit all or roll all back" publish across crates (hit 1).
- No multi-instance cloud orchestrator or policy-schema gating of steps (hit 2).
- No certification list or package database that tracks changes after import (hit 3).
- No function-level dependency graph or release-to-release graph comparison (hit 4).
- No de-duplication of graph nodes by dependency sub-tree identity (hit 5).
- No prioritised policy-violation scanning of repository artifacts (hit 7).

## Needs review (to the PM)
None.

## Limitations
- AI-assisted search and read; no attorney. Search coverage is a general web search plus patent-site-restricted queries, not a structured patent search. USPTO full text, Espacenet, WIPO, assignee and inventor searches, and classification (CPC) browsing were not run.
- Status and expiry dates are Google Patents' estimates, not the official record; a "fee related" expiry should be confirmed on USPTO.
- Claim text is machine-converted. Only independent claims were read; dependent claims narrow them and were not analysed.
- Several hits were granted applications; their continuations and other family members were not chased (eBay's two are named).
- Ideas 3, 4, 6, 7, 8 and 9 have thin or no patent-site coverage; a clear result on them means these queries found nothing readable, not that nothing exists.
- Open-source prior art is noted for context only; it is not an invalidity analysis.
- Re-sweep when ReleaseAnt adds a new family of capability, and at least yearly.
