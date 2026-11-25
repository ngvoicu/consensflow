# Recorded connection failures

These are the native JSONL records from T-1939's synthetic, loopback-only
connection-refusal and cut-stream cases. The four complete Pi recordings and
the first-error prefix are copied from that task's scrubbed fixtures. The
Claude Code and Codex recordings include both platforms, plus two Mac Claude
print-mode sessions. A refused session that was stopped before its last retry
is identified by its name; it is not a recording of the final outcome.

The fixture audit checked every JSON string for owner names and paths, common
credential shapes, email addresses and user skill names. It redacted email
addresses and replaced entire fields containing a user skill listing with a
placeholder in the Claude and Codex records. Record order and the boundaries,
roles, IDs, failures and turn events used by the readers were kept. No Pi
fixture needed further redaction. The Pi success in the corresponding test is
constructed from the cut record, not recorded.
