# ReleaseAnt: project instructions

Applies in addition to `C:\Projects\CLAUDE.md` (portfolio-wide rules; read it first, and AGENT-ONBOARDING.md once).

- This repo is a **shared building block** of the ant family (one lane, one repo, MIT OR Apache-2.0, CI and branch protection from day one).
- Read `docs/SCOPE.md` and `ROADMAP.md` before anything else. Survey and design come before code.
- Version rule: every pushed change bumps the version (docs and CI included); breaking = the second number before 1.0. The PM allocates the number at gate time.
- Dependencies at their newest versions; cross-repo dependencies are crates.io versions, never paths or git.
- Every claim needs evidence and a positive control (see the portfolio CLAUDE.md section 6).
- The lane may not be able to push to this repo: if a push is refused, send the PM the FULL commit SHA.
- Models: run on Sonnet only.
