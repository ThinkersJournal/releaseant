# ReleaseAnt roadmap

Order matters: survey, then design, then code. Every code row ships with a negative control (a test that fails when the behaviour is removed).

R0. Survey: read the publish scripts this portfolio already hand-wrote (tools/pub-one.cmd and /tmp/claim-retry.sh in the PM's tree, scripts/publish.ps1 in baracuda, the fuel and auth-framework publish loops described in PORTFOLIO-ROADMAP) and list every behaviour they share. Deliver docs/SURVEY.md.
R1. Design: docs/DESIGN.md with the publish-set computation, the ledger format, resume semantics, rate-limit policy, read-back, yank policy, and a test plan with a negative control per trap above.
R2. Core library crate `releaseant` (graph, plan, ledger) with tests against a fake registry.
R3. CLI `releaseant plan|verify|publish|resume|readback` (dry-run default).
R4. Real-registry rehearsal against a throwaway set of placeholder crates, then first real use on dirty-ranges.
R5. Adopt in fuel (43 crates) and baracuda (10-crate set); retire the hand-written loops.
