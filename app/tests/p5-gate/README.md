# P5-B's randomized gate

A reloaded page against a continuous xterm, in WebKit and Chromium, through the pane host's real API
(`crates/cf-panes/examples/gate_bridge.rs`: a real PTY window, the app's consumer, hub and journal).
Not a Playwright spec: run it by hand when the journal, the hub's boundary, `attach_page` or `history` change.

    cargo build -p cf-panes --example gate_bridge
    node app/tests/p5-gate/gate.mjs <webkit|chromium> <random cases> <first seed> [past-cap cases] [out.jsonl]

`cases51.mjs` holds T-2096's 51 cases, `gen.mjs` the 31 named cases and the seeded generator (random cases from
the first seed up; past-cap cases from first seed + 100000).

It exits with a failure where any stream differed. What must be cut is worked out from what a case printed and sized
(the host's frame says what was cut; the page's notice must agree), every piece must hold the printed bytes from its
offset, and a cut history is compared with an xterm built here from the host's frame and those bytes. A `{ hold, point }`
op holds a `pane_resize` on its way and lets it go at p1 (before the old sink refuses), p2 (after, the old page still
current), p3 (after the new subscription, before its first pull), p4 (between two pulls) or p5 (after the history is
in). One case takes a second lane away while its real program runs.

What it checks beyond the bytes (T-2289):

- A history is held to what the case promised: where the journal may start again (past 256 sizes), it starts no later than the
  journal's end when the last size that could start it was taken; the pieces follow one another and the last reaches the
  boundary. The answers a page gave since a cut load are compared with an xterm built from the host's frame, cursor reports with
  their places, so that an ordinary enquiry is never answered with a private report.
- A reply goes through the real `InputQueue` (`enqueue_page`: its page, pane, generation and call number), and the program keeps
  what reaches its input; the gate compares it with what the host admitted. A reply or an acknowledgement the host refuses (another
  generation, a window that is gone) is a failure.
- Every wait has a bound (`CF_GATE_WAIT_S`, 180 s by default). A bound that passes, or a host that says its program stopped at its
  room, ends the gate at once with the cause and exit 1.

`node app/tests/p5-gate/selftest.mjs <webkit|chromium>` is the gate's own test: it runs the gate on one case at a time with a fault put
into what the host or the page hands over (`CF_GATE_FAULT`: an empty frame, a private cursor report, a report for another place, every
acknowledgement of a retired lane lost, a docked reply or a last acknowledgement for another generation) and states the exit and the
cause each must give; a clean run of the same case must exit 0. `CF_GATE_ONLY=<regex>` runs the cases whose names match.
