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

## What moved after Node

Migration 0012 (the tester joins the roles a member holds and the pools a task
is for) moved two things that Node's recordings hold. Every `final` says
`userVersion: 11` and a role check and a pool check without the tester: they
are not rewritten, and the replay holds the database this ledger left to them
as Node's schema had it (`cf_ledger::testing::hold_apart_what_node_never_had_in_its_schema`).
And `ledger-008`, a ledger one version past what the build knows, which it refuses,
says 14 and 13 where Node's said 12 and 11: it was changed by hand, in those two
numbers, to stay the probe it was (each migration moves it again). And the four traces
whose refusals list the roles a member may hold or the pools a task may be for
(`ledger-projects-005`, `-008`, `ledger-staff-001` and `ledger-tiered-003`) list the
tester too, in the words they quote, as the ledger's refusals now do: changed by
hand, in those words alone.

Migration 0013 (a project has no mark that it comes back by itself: the daemon
suspends what was open when it starts, and the human resumes it) took away what
Node's projects had of it. Every answer that holds a project says `resumeOnStart`,
the event that suspended a project at a start says `resumeOnStart: true`, and every
`final` has the column `resume_on_start` and its lines in the schema of `project`:
none of them is rewritten, and the replay takes them out of what Node recorded,
where this ledger's has none to take out (`cf_ledger::testing::hold_apart_the_resume_mark`
and `hold_apart_the_resume_mark_in_a_dump`). The one call that cannot be held so is
`forgetResume` of a project that is not there, which Node refused as an unknown
project and this ledger has no mark to refuse for: `ledger-projects-011` is in
`DEPARTED` at that call.
