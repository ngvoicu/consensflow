# What Node answered: cf-harness's recordings

These files were recorded from Node's harness code (launch and channels, the
harnesses' record readers and quota, the harness admin and the detection beside
it, the stale Claude hooks), which is gone, and are fixed. Nothing records them
again: they hold the Rust harness code to what Node answered, and a change that
moves one is made in the file by hand, on purpose, with the reason in the commit.
The recorders ran Node's modules and went with them; they are in the flip release
`v3.0.0-alpha.82`, and in `21297242`, the commit before they were deleted.

| Folder | What Node answered | Read by |
|---|---|---|
| `launch/` | `tables.json`: the pure functions of the launch (windows' command lines, admissions, the record states, the console text and the rest). `scenarios.<platform>.json`: each adapter's scenarios played (Claude Code, Codex, Pi, Devin, OpenCode), a set a platform, since a launch names the platform's own paths and programs. `coverage.json` is written by hand: it counts every table and every harness's scenarios, and says which Rust test answers each, or the landing it waits for | `../launch/` |
| `records/` | `sequences.json.gz`: the scenarios of the chunked suite, each record written a piece at a time with a look after each. `suite.json.gz`: the cases of the completion and harness-version suites. `sweep.json.gz`: each JSONL fixture with one part of a record changed. `tables.json`: quota's functions and `localeCompare`, as tables. A scenario file holds one scenario a line, gzipped; readings are compared after unzipping, never as bytes (zlib's header names the platform that wrote it). The recorder set `TZ` first, for the resets that name no zone. The fixtures the scenarios read are `tests/engine/fixtures/completion` | `../records/` |
| `admin/` | `tables.json`: the layouts that are no more than a text, which hold on every system. `scenarios.<platform>.json`: each scenario played, one set a platform, since a CLI is found at the platform's own places under its own names and a path is joined with its own separator; the Windows one was recorded on Windows | `../admin/` |
| `claude/` | `stale-hooks.json`: what `staleClaudeHooks` reported over settings files given as `text`, as the `hex` of bytes that are no text, or as neither for none: the `events` it named, or what it `throws` | `../host_payloads.rs` |

## The launch scenarios

A scenario of `launch/scenarios.<platform>.json` is data, and the Rust player
(`../launch/scenarios.rs`) plays it step by step. Some steps set the scene and
record nothing: stand-in CLIs (`executable`, found and never run; `standIn`,
answering by its arguments), files and folders (`write`, `append`, `mkdir`,
`remove`), a SQLite store of a harness's own (`db`: `open`, `exec`, `run` with its
`params`, `close`), a harness's own status files, what the engine tells a window
(`opened`, `follow`), and whether looks at a harness's record wait to be released
(`holdLooks`). The others are recorded:

- `prepare`, `observe`, `ready`, `deliver` and `started` begin that work on the
  adapter or its window, through a pane host that answers each request as the
  step scripts it (`answers`), at once or held (`{held: true}`), for the pane
  `p1-zeus` of generation 1 unless the step names its `pane`; a peer on loopback
  that answers each `fetch` by its route as the step scripts it (`served`); and
  the programs `spawn` starts (`children`), a prepare's too.
- `release` answers a held host request (`release: op, answer`), a held fetch
  (`release: 'GET /session', answer`) or a held look (`release: 'look'`);
  `releaseBody` ends a held body (`releaseBody: route, body`).
- `advance` moves the clock by that many milliseconds, firing the timers due on
  the way one at a time.
- `close` closes the window as the engine does: its launch's files go, and the
  work still waiting on it keeps its hold.

After each, the work begun runs until it settles or waits on something a step
controls: a held request, a held look, a timer.

Nothing waits on the machine. The clock starts at 2026-09-19T12:00:00Z and moves
only when a step advances it; randomness is the stream the Rust fakes hand out,
byte `i` being `(i * 7 + 3) % 256`; a free port on loopback is the next from 41000
up; and `fetch` is the scripted peer. What an adapter makes with the system's
default modes comes out as the mask 022 leaves it, on every machine. Timers fire
when due, one at a time, the first armed of those due together first, and the work
runs until it holds still before the next fires. (Node fired timers due together
in one go; the recorder refused a scenario where that would differ. One
difference stays, on purpose: on a slow disk Node could fire a later timer while
file work was in flight, and Rust never does.)

A step's record is the same on every run. Every path under the root is `$ROOT/…`
(in a file URL, a URL's query and JSON text as well), the bundle's `bin` is
`$ROOT/bundle/bin`, the hash that names OpenCode's bundle is `$HASH`, Node itself
`$NODE`, the process the run was `$PID`, one long dead `$DEAD`, and a second live
one a scenario names `$OTHER`. It lists the work that settled (its step, and what
it answered or threw), the work still waiting, the requests asked of the host and,
when there were any, of the peer (`fetches`), the size of every random draw, and
what the step did to the tree under the root. A step may carry `kept`: a
difference Rust keeps from Node on purpose, why, and how Rust's own work settles
instead.

## The records scenarios

A scenario of `records/` is data: its steps write files and stores under a root,
move the clock and look, and the Rust player (`../records/play.rs`) plays the same
steps against its own readers. Everything a reader reads of time is the
scenario's: the clock is stubbed and moves only by a step (it starts at
2026-09-21T12:26:40.000Z), and every file step sets the file's mtime to the clock,
one millisecond after the step before it. What a look read is written compactly:
each item once, in `items`, and each reading once, in `readings`; a look names its
reading, and when the cached reader handed back an object it had handed back
before, the look it handed it to (`sameAs`, and `quotaSameAs` for the quota). A
reason that is ConsensFlow's own sentence is kept as it is; one that is the
platform's (an errno line, SQLite's words, V8's) is written `unreadable:
«platform»`, since only its prefix is promised (`tables.json` carries the list of
ConsensFlow's own).

## The admin scenarios

A scenario of `admin/` is `{ name, env, files, effects, steps, kept }`:

- `env`: the environment the admin is given; `$ROOT` in any text is the
  scenario's own folder, made afresh.
- `files`: what the folder holds before the first step: `{ path, text,
  executable, mode }`, `{ path, link }` or `{ dir }`.
- `effects`: what the world answers, never the machine's own. `run`: the
  programs the admin runs (a version probe, an update), by the program's name and
  its arguments, each a list of answers in the order asked: `{ stdout, stderr }`,
  `{ error: { message, killed, stdout, stderr } }` as `execFile` rejects, or `{
  held: true }`. `latest`: the release each harness's feed says (`{ value }`,
  `{ error }` or `{ held }`). `fetch`: the answers of the network, in the order
  asked: `{ status, chunks }` (a redirect is a status `fetch` refuses), or `{
  failure }` (`refused`, `cut`, `stall`, `stall-body`).
- `steps`: `check` (`id`, `refresh`), `update` (`id`), `source` (`id`,
  `executable`), `detect`; each begun under its `name` and run until it waits on
  something a step controls; `release` (`kind`, `call`, `answer`) answers a held
  call; `advance` (`ms`) moves the clock, firing the timeouts due on the way;
  `write` and `remove` change the files.
- `kept`: a difference Rust keeps from Node on purpose: `{ step, name, why, rust
  }`, `rust` the answer Rust gives where Node's is recorded.

Nothing waits on the machine: the clock starts at 2026-09-19T12:00:00Z and moves
only when a step advances it, and `execFile` and `fetch` answer as scripted. A
step's record lists what settled (in the order begun: by name, with its result or
its `error`), what still waits, and the calls it made, by program (`capture`), by
harness (`latest`) and by address (`fetch`), each in its own order: which of two
harnesses asks first is up to the runtime. A path under the folder is `$ROOT/…`,
and the hash that names a bundle of an extension is `$HASH`.
