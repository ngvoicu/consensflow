# Interview — decisions (2026-08-24)

**The directive** (Gabriel, verbatim): "we said that we move away from that
with consensflow cmux; we always run everything attached, no way to run
detached." Reaffirms the original "can't we attach from the start? so all
sessions are attached?" — which v-threads shipped only half of (catchup
reading; attach stayed opt-in `--attach`). This spec ships the other half.

**Turn-1 shape** (asked, answered): *TUI from the very first turn* — chosen
over stream-then-attach, accepting the named costs: codex still streams its
first answer (it physically must — no pre-set interactive id), opencode's id
is discovered from its store after launch, and window turns leave no run
directory of ours, so `cf catchup` is the read, not `cf last`.

**Consequences stated, not asked:**

1. **A pipe cannot host a TUI.** A window opens only where a terminal exists:
   `cf run` from a program (the lead's tool call, a test, `--json`) streams
   exactly as today. That is physics, not a retained detached mode — every
   human terminal attaches, every pipe streams.
2. **Host modes unchanged.** Attached-always is cmux mode only.
3. **Follow-ups go into the window.** Once the pane is the agent's own
   interface, the lead sends plain text into it (`cmux send`) and reads with
   `cf catchup` — `cf run` is how a conversation *opens*, not how every turn
   travels.
4. **`--attach` flag retires.** In a terminal it is the default and only
   behaviour; the flag would be a no-op lie.
