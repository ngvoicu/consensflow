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
