@echo off
rem Stand-in for $CARGO that fails the FIRST `cargo publish` it sees and passes everything else to the real cargo.
rem It injects a failure at the cargo boundary; the registry, the upload and the index read-back stay real.
rem Needs REAL_CARGO (full path of cargo) and FAILFIRST_STATE (a path that does not exist yet).
if "%~1"=="publish" if not exist "%FAILFIRST_STATE%" (
  echo failed once > "%FAILFIRST_STATE%"
  echo error: simulated failure injected by fail-first-publish.cmd, nothing was sent 1>&2
  exit /b 101
)
"%REAL_CARGO%" %*
