# P5-B's randomized gate

A reloaded page against a continuous xterm, in WebKit and Chromium, through the pane host's real API
(`crates/cf-panes/examples/gate_bridge.rs`: a real PTY window, the app's consumer, hub and journal).
Not a Playwright spec: run it by hand when the journal, the hub's boundary, `attach_page` or `history` change.

    cargo build -p cf-panes --example gate_bridge
    node app/tests/p5-gate/gate.mjs <webkit|chromium> <random cases> <first seed> [past-cap cases] [out.jsonl]

`cases51.mjs` holds T-2096's 51 cases, `gen.mjs` the 18 named cases and the seeded generator (random cases from
the first seed up; past-cap cases from first seed + 100000).
