Native Devin 3000.10.21 TUI store and diagnostic wire records from the isolated loopback provider probe on 2026-09-12. Local profile paths and unrelated native system context are redacted. Session columns unrelated to chain traversal are omitted. The native chain contains three hook-collected replies and a next-prompt late reply, followed by new/resume. Raw SQLite rows include superseded revisions. Completion requires a matching streaming message UUID and cause:complete; finish_reason alone is not evidence.

`worker-tui.json` records private native probe `probe-1789245570096144000`
on Devin 3000.10.21, with a loopback mock provider and no receiver capability.
Three completed replies survive /new and /resume with the same native message IDs.
An independent session reply is excluded; a fifth, cancelled inference remains
in native history but is not a completed answer. System context is redacted.
