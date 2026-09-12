# macOS pane process ownership

2026-09-12. The existing process-group kill misses a native descendant after
`setsid`, and current PPID ancestry is lost when an intermediate parent exits.
Four real-PTY regression tests reproduced the missing cleanup before changes.

The implementation combines the exact root process identity, original parent
identities supplied by the kernel, and a fresh per-pane inherited ownership
marker. Never select a process by executable name, project directory, or marker
text occurring in arguments. Snapshot only same-user processes, recheck process
birth/unique identity before signaling, stop owned parents while collecting their
children, then kill the collected processes. Always kill already stopped owned
processes if a later scan fails. This applies to pane close, suspend, delete and
application teardown through the existing shared `PaneTable` termination path.

The initial implementation exposed a native limitation rather than a test bug:
macOS hides environment variables of restricted executables such as `/bin/sleep`.
An environment marker alone did not find a reparented restricted child. The
unchanged regression now passes using the kernel's original parent unique ID.

Primary contracts:

- [XNU process identity API](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info_private.h)
  declares the fixed 56-byte identity structure and `PROC_PIDUNIQIDENTIFIERINFO`.
- [Original parent identity](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_internal.h)
  survives reparenting. PID reuse cannot establish ancestry.
- [Native environment visibility](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sysctl.c)
  explains why protected system executables may omit environment values.
- [Process event flags](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h)
  explicitly retire NOTE_TRACK/NOTE_CHILD support; no unsupported fork watcher
  or entitlement workaround is added.

Scope: normal inherited harness descendants, detached groups, and retained
kernel parent links. This is process cleanup, not an OS sandbox against programs
deliberately escaping every ownership signal through an exited ancestor chain,
cleared environment, another user or an external service manager. Unproven
ownership must never authorize killing an unrelated process.

All probes used newly created local processes; no current ConsensFlow process,
user session, native installation or global configuration was changed.
