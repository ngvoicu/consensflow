# What Node's ledger did: the traces

Each file is a ledger that one test of Node's suites opened, as a trace of what
it was asked and what it answered: `<suite>-<NNN>.json.gz`, numbered from 001 in
the order the suite's tests ran. A trace holds every value its ledger was given
or drew, the clock's readings and the session names included, so it replays the
same whatever ran it, and then the database the ledger left, to be compared
exactly.

They were recorded from Node's ledger and from the suites that opened it, which
are gone, and they are fixed. Nothing records them again: they hold the Rust
ledger to what Node's did, and a change that moves one is made by hand, on
purpose, with the reason named in `../replay.rs` (its `DEPARTED` list says which
traces the Rust ledger departs from, and at which call). The recorder ran Node's
ledger module and went with it; it is in the flip release `v3.0.0-alpha.82`, and
in `21297242`, the commit before it was deleted.

`../replay.rs` plays them all; the daemon's traces (`crates/cf-daemon/tests/goldens`)
and the engine's (`crates/cf-engine/tests/traces`) are built on the same kind of
step.

## A trace

A trace is JSON, gzipped, and holds `test` (`{file, line}`: the test that opened
the ledger), `number`, `file` (`«ledger»`, which no player reads: each replay opens
a file of its own), `options` (whether the test gave the ledger its own `now`,
`names` and `trace`), `calls` and `final`. A call is `{method, args, clock, names,
events, callbacks, result}`: the method called and its arguments, the clock
readings and session names it took, the events it logged, what its callbacks were
given, and its result, or the error it threw. `final` is the database after the
ledger's `close`: `{userVersion, schema, tables}`, each value as SQLite quotes it.
What JSON cannot hold is tagged as in the daemon's traces
(`crates/cf-daemon/tests/goldens/FORMAT.md`): `{"$undefined":true}`,
`{"$number":"NaN"}`, `{"$bigint":"…"}`, `{"$date":"…"}`, `{"$set":[…]}`,
`{"$map":[[key,value]…]}`. Each ledger read a clock that started at
`2026-01-01T00:00:00.000Z` and moved a second a reading, and drew its session
names from one fixed sequence, unless the test gave its own: that is what makes
two recordings the same bytes.
