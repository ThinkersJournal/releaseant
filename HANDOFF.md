# HANDOFF (releaseant lane)

Current state: repository seeded 2026-10-23 by the PM. No code. CI is in place (fmt, clippy, test on three OSes, doc, cargo deny, advisory CodeRipper portfolio job).

## First task
Start ROADMAP R0 (the survey): read the named existing implementations at `origin/main` of each project (never a shared working tree), write `docs/SURVEY.md`, open a PR (version bump 0.0.2), send the PM `[READY]` with the FULL SHA.
Then R1 (design). Do not write library code before the design is merged.

## Rules to remember
- Messages to the PM are short, compact format (`C:\Projects\MESSAGE-FORMATS.md`).
- The check named "Portfolio checks" is ADVISORY (do not silence rules; fix or put a row on the roadmap).
