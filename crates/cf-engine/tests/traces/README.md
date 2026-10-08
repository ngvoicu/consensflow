# What Node's dispatcher did: the traces

Each file is a test of Node's dispatcher, as a trace of what the engine asked of
its seams in order and the database it left: `core-dispatcher-<NNN>.json.gz`,
numbered from 001 in the order the suite's tests ran.

They were recorded from Node's dispatcher and its suite, which are gone, and they
are fixed. Nothing records them again: they hold the Rust engine to what Node's
did, and a change that moves one is made by hand, on purpose. The recorder ran
Node's dispatcher with a recording one in its place and went with it; it is in
the flip release `v3.0.0-alpha.82`, and in `21297242`, the commit before it was
deleted.

A trace is JSON, gzipped, and holds `test` (`{suites, name, line}`: the test of
Node's dispatcher suite it was recorded from), `events` and `finals`. `events` is
what the engine asked of its seams, in order: each is `{op, args, answer}` (or
`threw`) where one of the dispatcher's operations began, and the calls of the
ledger, the pane host and the adapters, each with what it was given and answered.
`finals` is the database each ledger of the test left, its temporary folder
written `«dir»`. The launch ids a test drew were a stream of their own, the n-th
`00000000-0000-4000-8000-00000000000n`, so the fake agents' conversations, which
are named after them, are the same at every recording.

`../dispatcher/traces.rs` plays them, through one projection of what is the
engine's behaviour. Where the engine departs from Node's trace on purpose, the
test is named in its `DEPARTED` list, and held to a trace of its own in
`../departures/`: those are recorded from the engine itself, in the shape of
Node's (`npm run departures` records them again).
