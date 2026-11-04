Recorded on the Mac of the work (macOS 27.0.1, 2026-11-03, T-1194), unchanged but for the files' names:

- `media-library-prompt.ndjson`: the three tccd lines (`AUTHREQ_CTX`, `AUTHREQ_ATTRIBUTION`, `AUTHREQ_PROMPTING`)
  of a real Apple Music dialog, raised for `ls ~/Music/Music` run in a window of a stand-in app
  (`dev.ngvoicu.tccprobe`, its pid 22911; `ls` is 22913). `log show --style ndjson`.
- `own-client-prompt.ndjson`: a real Downloads dialog raised for a bare program that was its own
  responsible process (no `responsible` in the attribution; pid 57904).
- `background-queries.ndjson`: the queries sandboxd made for `ls ~/Music/Music` in a Background
  session, where no dialog can be drawn (`preflight=yes`, no `AUTHREQ_PROMPTING`).
- `app-preflight.ndjson`: a real `log stream` of a Screen Recording check made in the live app's
  name (`preflight=yes`, no dialog), as the system's own tccd (pid 414) wrote it.
- `stream-banner.txt`: the first line `log stream` prints, which is no JSON.
