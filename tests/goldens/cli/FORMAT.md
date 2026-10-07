# What Node answers on the CLI's standalone verbs: the recordings and their format

Step 4 flips the app to Rust, and the verbs Node's CLI answers by itself
(`--help`, the version, `catalog`, `agent`, `setup`, `doctor`) go with it. They
are recorded first, from `node src/cli.js` as it is (the CLI `bin/cf.mjs` runs
for a home that has taken the way back to Node; `bin/cf.mjs` itself hands every
command of any other home to the native `cf`, so it is no oracle), and `crates/cf`
(`standalone/`) is held to the recording, `setup` and `doctor` too.

    npm run goldens:cli                 runs every scenario against Node, writes the files
    node tests/goldens/cli/record.mjs --check    records again, says which files differ, writes nothing
    npm run test:clis                   the suites of the CLI against Node's cf.mjs and then the native cf
    npm run plants:cli                  plants bugs in the Rust and in the recorder, says what caught each

`npm run goldens:cli` is run once on each platform the tests run on, since a path
is joined with its own separator and a stand-in for a harness's CLI is a script
or a `.cmd`: the Windows file is recorded on Windows. `tests/cli-goldens.test.mjs`
holds the checked-in files equal to a recording made now, and the Rust players
(`crates/cf/tests/cli_goldens/`, `crates/cf-base/tests/args.rs`) hold Rust to them.

## What is in the files

| Path | What | Read by |
|---|---|---|
| `crates/cf/tests/goldens/cli.<platform>.json` | every scenario played: `darwin` for macOS (and any system but Windows), `win32` for Windows | `crates/cf/tests/cli_goldens/` |
| `crates/cf-base/tests/goldens/args.json` | what `util.parseArgs` answers for the words after each verb, for lists of up to three words from the ones people get wrong (`parse-args.mjs`); the same on every system | `crates/cf-base/tests/args.rs` |

## A scenario

`world.mjs` plays each scenario in a folder of its own, made afresh, with an
environment of its own (nothing is inherited: `HOME`, `CONSENSFLOW_HOME`,
`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME`, `PATH` and
`CONSENSFLOW_BIN_DIR`, all under the folder, as `tempEnv` makes them, apart from
what the scenario changes and what Node cannot start without on Windows,
`SystemRoot`, which a player gives alike), the clock fixed, and the CLI run as
`node --import clock.mjs src/cli.js <args>`.

A case of `cases` is:

| Key | |
|---|---|
| `name` | what it does, unique |
| `args` | the words after `cf`, each as the system gave it to the process |
| `env` | the environment it was given, with `$ROOT` for the folder; a variable the scenario took away is not there |
| `stdin` | the text it was given to read, or `null` (it reads nothing, and none of the verbs asks) |
| `before` | the files of the folder before the run: `{path, text}`, with `executable: true` where the file is one (POSIX only), `{dir}` for a folder with nothing in it; the paths relative to the folder and written with `/`, in order of path |
| `stdout`, `stderr`, `code` | what it printed, said on the error output, and exited with; for the cases of `pipe`, see below |
| `after` | the files of the folder after the run, as `before` |
| `pipe` | only for a case whose output goes to a pipe: `closed` (nobody reads it, the pipe is closed before the CLI starts to write: `cf … \| false`) or `first-line` (its first line is read and the pipe closed: `cf … \| head -1`). Its `stdout` is `null` for `closed` and the first line for `first-line`, and `stderr` and `code` are the CLI's. On POSIX the CLI is the first of a shell's pipeline, so that what it writes to is a pipe and not the socket Node's `spawn` makes |
| `kept` | only where Rust keeps a difference from Node on purpose: `{why, rust}`, `rust` the `{stdout, stderr, code}` it is recorded to give, and `after` where the files are not what Node left: `"before"`, as they were |

The clock the CLI read (`clock`, at the top of the file) is one instant, whenever
it is read: the time an agent is stamped with is the same in every recording.

## Names for what differs from one run to the next

A recording holds nothing of the machine that made it. Where a text of it would
have the machine's place or the build's, it has a name, and a player puts its own
(a text with any other of the machine's places in it is refused as it is
recorded: `world.mjs` checks every case).

| Name | Where | A player |
|---|---|---|
| `$ROOT` | any text: the folder the scenario was played in, as the system names it (`realpath`) | puts the folder of its own, as the platform writes a path, and writes its own back as `$ROOT` in what it reads |
| `$VERSION` | any text: the version in `package.json` | writes the build's version as `$VERSION` in what it reads |
| `$NODE`, `$REPO` | the files and output of `setup` and `doctor`: the runtime that ran the CLI, and this repository, which a launcher names (`$REPO/bin/cf.mjs`) | makes a launcher that names them with a program that is there (its own) and the `cf.mjs` beside the binary under test, and writes what the binary says of them back as the names (`names.rs`) |
| `$HASH` | the folder an extension's bundle is published in (`extensions/pi/$HASH/…`) | writes the 64 hex digits of the folder as `$HASH` |
| `$PAYLOAD` | the text of every file of an extension: its bytes are the repository's own, which `pi-install.test.mjs` and `opencode-install.test.mjs` hold | writes `$PAYLOAD` for a file whose bytes are the repository's file's at the path after the hash, as they must be; OpenCode's `tui.json`, which is made where it is put, for one that is `{"plugin":["file:///…/consensflow-session.mjs"]}` and nothing else |
| `$CF` | not in a recording: the player's own, for the binary under test | writes the path of the binary as `$CF` in what it reads: the launcher this build writes runs it |

## What the Rust player does with a recording

It makes the case's folder from `before` (`$ROOT` in a file's text is the folder),
runs the binary with exactly the environment of `env` (no folder of a recording
has the way back's `use-node` file in it, so the binary answers by itself), and
compares the output, the error output, the exit code and the files after, byte
for byte.

- **The clock.** The binary cannot be given the clock Node was. Each `createdAt`
  and `updatedAt` of an agents file that holds an instant of the run's own (as
  `toISOString` writes it, within a few seconds of the run) is read as the
  recorder's fixed instant, once it is known to be one instant: a run that stamps
  two is a difference. A stamp that was in the file before the run is not touched.
- **`kept`.** The case is held to what `rust` says, and to the files as `before`
  where `after` says so.
- **`setup` and `doctor`** are played as every verb is, with the names above put
  as this run's, and two differences that step 4 makes, stated once each with
  its reason in `paired.rs` (the recording holds Node's answer and not Rust's, so
  nothing is re-recorded for them, and the player asserts how many cases hold each):
  - **The launcher's new shape.** What `setup` writes names only the native `cf`
    (step 4's decision: the bundle after the deletion has no Node and no `cf.mjs`).
    Node's text with the two lines that name what it runs replaced by ours is
    what is held in the files after, for every case that leaves a launcher of
    ours.
  - **Whose a command is.** A native `cf` has no runtime of its own to compare a
    command's with, so it tells whose a command is by the file the command runs.
    The recording's launcher of another runtime runs `$REPO/bin/cf.mjs`, which
    is this copy's own: `doctor` says no more of it than it says of this
    copy's, where Node said it was another ConsensFlow (one case).

## What is not recorded

- A home that no variable names (neither `CONSENSFLOW_HOME` nor `HOME`): Node asks
  the system for the user's home, which is the machine's own and the real
  `~/.consensflow`. Rust refuses with the daemon's words, `setup` and `doctor`
  before they make or say anything (`crates/cf/src/standalone/tests.rs`).
- What `doctor` says of a command in the new shape (`command:`, which Node cannot
  read: it says nothing of such a command), and which copy a command is: the
  process tests of `crates/cf/tests/standalone.rs` run copies of the binary.
- A folder or a file the CLI is not allowed to write: the roster's write
  failures are recorded at the roster (`goldens:unwritable`), and the CLI says
  what the roster says.
- A window's token (`CONSENSFLOW_TOKEN`): the CLI hands everything to the native
  `cf`, which is then the board (`board.json`).
