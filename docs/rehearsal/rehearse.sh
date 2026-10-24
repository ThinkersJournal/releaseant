#!/usr/bin/env bash
# ReleaseAnt R4: real-registry rehearsal. See docs/REHEARSAL.md for what it does and why.
#
# PUBLISHES three placeholder crates to crates.io (irreversible):
#   releaseant-rehearsal-core 0.0.0 -> mid -> top, then all three again at 0.0.1.
#
# The token is read by cargo from CARGO_REGISTRY_TOKEN, which the caller sets in this shell.
# This script never prints, logs or tests the value (it only checks the variable is non-empty).
#
# Usage:  RELEASEANT_BIN=/path/to/releaseant ./rehearse.sh 2>&1 | tee rehearsal-all.txt
set -u

BIN="${RELEASEANT_BIN:?set RELEASEANT_BIN to a releaseant binary built from a main that includes the CLI (PR 5)}"
REPO="${REHEARSAL_REPO:-/c/Projects/releaseant-rehearsal}"
OUT="${REHEARSAL_OUT:-$PWD/rehearsal-out}"
# Commits in the rehearsal repository: A = version 0.0.0, B = version 0.0.1.
A=513371646eb3eb0c5523bcc5f6c2e03729d21c8c
B=dc8bd420dcba8b683dfd2dcdea1015332265f66b
CORE=releaseant-rehearsal-core

say() { echo; echo "=== $(date -u +%Y-%m-%dT%H:%M:%SZ) $*"; }

[ -n "${CARGO_REGISTRY_TOKEN:-}" ] || { echo "CARGO_REGISTRY_TOKEN is not set in this shell; stop."; exit 2; }
[ -x "$BIN" ] || { echo "not executable: $BIN"; exit 2; }
for c in "$A" "$B"; do
  git -C "$REPO" cat-file -e "$c^{commit}" 2>/dev/null || { echo "commit $c is not in $REPO"; exit 2; }
done
mkdir -p "$OUT"
LEDGER="$OUT/ledger.jsonl"

say "versions"
"$BIN" --version
cargo --version

say "1. readback before anything (expect ABSENT x3)"
"$BIN" readback --repo "$REPO" --sha "$A"; echo "exit $?"

say "2. run A: publish 0.0.0 for real (expect 3x 'published and read back')"
"$BIN" publish --repo "$REPO" --sha "$A" --execute --ledger "$LEDGER" 2>&1 | tee "$OUT/runA.log"
echo "exit ${PIPESTATUS[0]}"

say "3. readback after A (expect present x3)"
"$BIN" readback --repo "$REPO" --sha "$A"; echo "exit $?"

say "4. run B, to be INTERRUPTED once $CORE 0.0.1 has been read back"
"$BIN" publish --repo "$REPO" --sha "$B" --execute --ledger "$LEDGER" >"$OUT/runB1.log" 2>&1 &
PID=$!
WINPID=$(cat "/proc/$PID/winpid" 2>/dev/null || true)
interrupted=no
for _ in $(seq 1 3000); do
  if grep -q "\"crate\":\"$CORE\",\"version\":\"0.0.1\",\"event\":\"visible\"" "$LEDGER" 2>/dev/null; then
    if [ -n "$WINPID" ]; then taskkill //F //T //PID "$WINPID" >/dev/null 2>&1; else kill -9 "$PID" 2>/dev/null; fi
    interrupted=yes
    break
  fi
  kill -0 "$PID" 2>/dev/null || break
  sleep 0.2
done
wait "$PID" 2>/dev/null
echo "interrupted by this script: $interrupted (if 'no', the run finished or failed before the kill; see runB1.log)"
echo "--- output of the interrupted run"
cat "$OUT/runB1.log"

say "5. what the registry says now (an interrupted run may have uploaded more than it recorded)"
"$BIN" readback --repo "$REPO" --sha "$B"; echo "exit $?"

say "6. run B again = resume (expect $CORE already published; the rest published and read back)"
"$BIN" publish --repo "$REPO" --sha "$B" --execute --ledger "$LEDGER" 2>&1 | tee "$OUT/runB2.log"
echo "exit ${PIPESTATUS[0]}"

say "7. final readback (expect present x3)"
"$BIN" readback --repo "$REPO" --sha "$B"; echo "exit $?"

say "8. the ledger, whole"
cat "$LEDGER"

say "done. Files in $OUT. Return ALL of this output verbatim, including any 429 text."
