@echo off
REM Installed by ConsensFlow. Runs THIS bundle's copy of the CLI, so a pane the
REM app opened never reaches an older `cf` left on PATH by an earlier install.
REM CONSENSFLOW_NODE names the runtime absolutely, and the app sets it; an unset
REM runtime is a refusal, not a guess (see bin/cf for why).
if "%CONSENSFLOW_NODE%"=="" (
  echo cf: CONSENSFLOW_NODE is not set - this launcher runs ConsensFlow's own runtime, which the app sets when it opens a pane. Open a pane from the app, or run bin\cf.mjs with a node of your choosing. 1>&2
  exit /b 1
)
"%CONSENSFLOW_NODE%" "%~dp0cf.mjs" %*
