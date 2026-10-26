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

## Where the Rust code departs from what Node answered

A departure is kept on purpose, and the recordings it moves are moved by hand,
once, with the reason here and in the commit.

- **A Codex input told twice** (2026-10-10, found by the live image-designer
  test). Codex writes the input of a turn twice, in the same turn and with the
  same words: as the model's message (`response_item`, a user's message with an
  id like `msg_…`) and as the item it completes itself (`item_completed`, a
  `UserMessage` under an id of its own). Node's reader listed both (its case was
  named "native ids survive duplicate text"), so a task's transcript showed the
  brief twice. The Rust reader lists the input once, as the telling that came first
  and under its id, and counts: two inputs that say the same words are two
  items, the same words in another turn are another item, and a message with
  no twin (the environment Codex adds, a fork's inherited message) stays.
  `records/` moved for it in 39 of its 399 scenarios, all of them Codex's, in
  every reading that lists such an input: 6 of `sequences`, 13 of `suite`, 20 of
  `sweep`. Each such reading lost the repeated user item and nothing else (its
  other items, their order, `inFlight`, `failed`, `quota` and `settlement` are
  Node's); no look was added or taken, so the counts in `../records/main.rs` stand.
- **A Claude window opened again on a turn the app's quit cut** (2026-10-11, the
  human's report that the daemon seemed not to send responses; found by
  `npm run live:resume-cut` and `npm run live:restart-cut`). The quit ends a
  window with its turn at work, and the daemon that starts again resumed the
  chief on that conversation (since 2026-10-12 the human's Resume does, and a
  start opens nothing): a record whose last turn has no end. Claude 2.1.296
  resumed there waits at its prompt, status idle, and writes nothing of the turn;
  Node read such a window at work for good, since its record said the turn was
  in flight, and nothing was delivered to the chief until the human typed to it.
  The Rust adapter reads a window opened with no message of its own at rest, by
  Claude's status, while its record holds no more than it did when the window
  was opened (`shared/left_open.rs`). `launch/` moved for it in one step of
  one scenario, `claude: a conversation Claude kept is resumed on its session,
  and its window looks there`, step 5, in both platforms' recordings, which now
  carry a `kept` difference there: Node's answer `settled: false`, Rust's `true`.
  Pi has the same rule, with its extension's word for a window that has seen no
  turn begin; it moved none of the recordings.

- **A turn that ends with agents pending** (2026-10-11, found when a chief's
  result waited twelve minutes for its four research agents). Claude Code writes,
  on the `turn_duration` that ends a turn, how many background agents and
  workflows are still running (`pendingBackgroundAgentCount`,
  `pendingWorkflowCount`; left out when none). Node's reader took a record that
  named any as no end of the turn, which then stayed in flight until the last
  agent finished, and every message for the window waited. The Rust reader takes
  it for the end of the turn, as it takes a stop hook's summary (a ConsensFlow
  window's Stop hook makes Claude Code write one before every `turn_duration`),
  and says the agents are pending (`agentsPending`, said only when true, which no
  reading of Node's has). A count that is no count (a string, a fraction, a number
  below 0) is still no end of the turn. `records/` moved for it in 4 of its 399
  scenarios, all of `suite`: the readings of `claude-v263` and `claude-v265`
  (captures of Claude Code 2.1.263 and 2.1.265, whose `turn_duration` 2.1.305,
  the newest seen, still writes) whose records say a count of 1 (of agents and
  of workflows, two each) are settled, the answers a turn's end completes are
  complete, and they say `agentsPending`; the six that say a count of none (−1,
  `"0"`, `null`) are Node's still. No look was
  added or taken, so the counts in `../records/main.rs` stand.

- **An OpenCode 2, and an OpenCode that updates itself** (2026-10-15, the
  owner's OpenCode updated itself to 2.0.29, and the live suites `approval`,
  `specs` and `resume-fresh` waited for ever for the chief's window to read
  idle, while `finish` never saw the worker's window). OpenCode 2 replaced what
  a window is launched with (`--port`, `--hostname` and `--model` are gone, and
  one shared service stands where each window's server was), so a window opened
  on it never reaches ConsensFlow's plugin; and a 1.x updates itself to the
  latest release when it starts, which is how the owner met it. Node's adapter
  never asked OpenCode its version and never told a window not to update. The
  Rust adapter asks `--version` first, as Devin's does (once as the file is on
  disk), and refuses an OpenCode whose major version is 2 or more before the
  plugin, the role, a port or a server is made, in words that say what to do: an
  OpenCode that cannot be asked, or says no version, goes on as it did. The
  window's `OPENCODE_CONFIG_CONTENT` ends with `"autoupdate":false`, unless the
  configuration the human set names an `autoupdate` of its own (null is none).
  `launch/` moved for both, in both platforms' recordings, in 157 of the 158
  OpenCode scenarios (the one that did not is the OpenCode that is not
  installed, which finds no program to ask). Each of the 161 prepares that find
  one gained a `ran` of `opencode --version`, after its `fetches` and before its
  `spawned` (the stand-ins answer nothing: the question fails, and the launch
  goes on), and every `OPENCODE_CONFIG_CONTENT` the records hold gained
  `"autoupdate":false` at its end: 192 on macOS and 193 on Windows, which are the
  answers of the prepares (124 and 125) and the environments of the throwaway
  `serve` each fresh window makes its conversation on (68). Nothing else of them
  moved: with the `ran` and the `autoupdate` taken out, each of the 157 reads as
  the recording before the change did, and the other 352 scenarios (the other
  harnesses', and the OpenCode that is not installed) are as they were, byte for
  byte. `admin/` carries a `kept` difference in the six OpenCode scenarios (five on
  Windows) whose stand-in says `5.0.x`, which is an OpenCode of 2 or more: 12
  steps (10), where the Harnesses screen now says the OpenCode is not supported
  yet (a `setup` of `not-supported-yet`) and its newer release is `unknown`, not
  an update `available`. Node's stand-ins said `5.0.x` for every CLI they stood
  for, whatever its own versions are. An OpenCode of 1.x is likewise offered no
  update to a release of 2 or more, which npm's `latest` is since 2.0; no
  recording has one (the feed of the one 1.x scenario is offline), so none moved
  for it.

- **An OpenCode window keeps the human's skill paths** (2026-10-25, T-540's
  harness parity). OpenCode reads `OPENCODE_CONFIG_CONTENT` after its global and
  project files, and a list in it replaces theirs, so a window told only of the
  role's skills folder lost the `skills.paths` the human's files name, which the
  same OpenCode opened in a terminal in that folder has. Node's adapter merged the
  role into the environment's configuration alone. The Rust adapter asks OpenCode
  its configuration for the window's folder first (`opencode debug config`, run
  there with the environment's configuration and its updates off), and puts the
  paths it names before the role's; an OpenCode that cannot be asked, or answers
  no configuration with a list of texts there, gives none, and the window goes on
  as it did. `launch/` moved for it in both platforms' recordings, in 155 of the
  158 OpenCode scenarios: each of their 159 prepares that get past the version
  gained a `ran` of `opencode debug config`, right after its `opencode --version`,
  in the folder the window works in (the stand-ins answer nothing: the question
  fails, and the launch goes on). Nothing else moved: with that `ran` taken out,
  each record reads as it did, and the other 357 scenarios are as they were, byte
  for byte.

- **A Devin wire log replaced** (2026-10-16, Astra's and Sol's reviews of the fix of
  the field report B1). Node took a wire log that shrank for one replaced and read it
  from its start, and any other for the one it had read: one as long as the last was
  read as nothing new, and one emptied kept the old offset, so what was written in it
  next was read from where the old one ended. Since a window's readiness reads the
  permission mode from it, a configuration rewritten from `bypass` to `normal` at its
  size was read as `bypass`. The Rust reader takes a log that shrank, that begins
  with other bytes than the ones it read, or that was written to and did not grow,
  for another, and reads it from its start, the mode of the one before forgotten
  (`devin/wire_log.rs`). `launch/` moved for it in one step of each of two
  scenarios, in both platforms' recordings, which now carry a `kept` difference
  there: `devin: a wire log replaced by one of the same size is not seen to change`,
  step 5, and `devin: a wire log emptied and written again is read on from where
  the old one ended`, step 7, where Node's answer is the window at rest and Rust's
  the window switched to the conversation the new log names.

- **A Codex window opened on a thread waits for its broker to name it**
  (2026-10-20, zeewin's 2-hour live run of 2026-10-19: a Codex chief opened again
  at rest after the app's restart read "starting" for 78 minutes, and nothing
  reached it). Node's `started` of a window opened on a thread answered at once,
  with no question of the broker, so a broker that never named the thread was
  never noticed. The Rust adapter asks the broker as it does for a new window,
  for at most the same minute (`codex/adapter.rs`). `launch/` moved for it in
  one step of one scenario, in both platforms' recordings: `codex: a conversation
  Codex kept is resumed on its thread, and its window has nothing to learn`, step
  2, which is now served the broker's word that the window shows its thread, and
  records the one `GET /session` it asks; its answer is Node's, `null`. And the
  broker is asked once more at the deadline (the reviews of round 1: a thread named
  after the last look before it was the window's, and the chief was closed at
  work), which moved one more step: `codex: a thread never named in a minute is an
  error`, step 3, which records one `GET /session` more, at the minute; its answer
  is Node's.

- **OpenCode's store is the XDG place on Windows too** (2026-10-21, 3.0.6 item 10).
  Node looked for it in `opencode` under `%LOCALAPPDATA%` and `%APPDATA%` as well,
  after the XDG place, "where some versions keep it". OpenCode 1.19.3 keeps it in
  the XDG place on Windows (zeewin: `.local\share\opencode` in the home, and no
  `opencode` folder under `AppData`), so the Rust adapter looks nowhere else
  (`opencode/paths.rs`). `records/` moved for it in one scenario of `suite`:
  `opencode: its store is found where this OpenCode keeps it` (it ended in "the
  one holding the session first"). Its two looks that found the session in the
  store under `%LOCALAPPDATA%` (steps 34 and 40) are taken, and so is the
  Windows environment of the look that stays beside the other two; the two that
  stand read as Node's did. `suite` holds 199 looks now, 98 of OpenCode's in all
  (`../records/main.rs`), and 1510 are played.

- **A Codex record is read by the names Codex writes** (2026-10-21, 3.0.6 item 10).
  Node's reader took other names where the usual one was none: `Text` for a
  content part's `text`, a record's `id` for its `call_id` and for a sub-agent's
  `agent_thread_id`, and for a command's `aggregated_output` its `formatted_output`
  or its `stdout` and `stderr` joined. No rollout of Codex holds one of them in
  place of the usual name (619 rollouts on the Mac, 0.77 to 0.163.0, and 119 on
  zeewin), so the Rust reader reads the usual names only
  (`codex/record.rs`, `codex/record/fields.rs`). `records/` moved for it in one
  scenario of `sweep`: `sweep 7: codex/errored-task-complete.jsonl, record 8,
  payload.item.agent_thread_id nulled`. Node took the record's `id` for the
  nulled name, so the sub-agent it started was never reported done and the turn
  stayed in flight; the record now names no agent, and the turn is settled. Its
  one look read `inFlight: true` and `settlement: in-flight`, and reads `false`
  and `settled` now; its items, `failed` and `quota` are Node's.

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

- **A torn line of a record or a wire log** (2026-10-22, T-223 and T-248, from
  T-187's dry run and Calliope's reviews T-213 and T-246). A short write (a full
  disk, a process killed in a large write) leaves part of a record, and the next
  append makes one line of it and itself, which is not JSON for good. Node failed
  every look at a record holding one (`malformed JSONL at record N`), and every read
  of a Devin wire log holding one. The harnesses go on past such a line, and so does
  Rust: `shared/record/jsonl.rs` passes it over once a record follows it or its wait
  is over, and `devin/wire.rs` passes it over as `devin/wire_log.rs` did. `records/`
  moved for it in 12 looks, which `tests/records/main.rs` holds to what Rust reads
  instead. `launch/` moved for it in three steps of two scenarios, in both
  platforms' recordings, which now carry a `kept` difference there: `devin: a wire
  log with a line that is no JSON names no conversation, however many follow it`,
  step 5 (whose `kept` names the operation it settles, `op` 3, the `started` the
  minute's wait ends), where Node's answer is that Devin never said which session it
  opened and Rust's the session the line after it names; and `devin: a message is
  refused for a log that is not there, and for one that cannot be read`, steps 8 and
  10, where Node refused the message as the conversation unavailable and Rust as
  another conversation displayed, the torn line saying nothing.
