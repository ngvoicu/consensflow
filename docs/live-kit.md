# The live test kit

ConsensFlow's tests mostly run on **stand-in agents**: small programs that act
like a harness, with no model and no login. The **live test kit** is the other
part. It runs the real harnesses (Claude Code, Codex, Pi, Devin and Muse Code)
on real models, through the real app's daemon and pane host.

## When to run it

A live run costs real money or quota: every case talks to a real model on the
logins of the account that runs it.

- **Run the offline suites first.** They need no login and spend nothing.
- **Run a live case only** for a release's checks, or for a change that no
  stand-in can show (how a real harness draws its screen, asks a question, or
  answers a key).

The offline suites:

```sh
cargo xtask check                     # the full gate before a commit
cargo xtask test daemons              # the daemon, the rig and the regressions
cargo test -p cf-e2e --lib            # the kit's own tests
cargo test -p cf-e2e --test live_matrix   # a live file's stand-in cases
```

Live cases are marked `#[ignore]`, so a plain `cargo test` never starts one.
The stand-in cases some live files hold run offline like any other test.

## Running a live case

### What you need

- A checkout of this repository and a Rust toolchain that builds it.
- The harnesses you want to test, installed and signed in on the account you
  run from. A harness that is not on the search path, or keeps no login, is
  named with how to sign in (`claude` once, `pi` once, `codex login`,
  `devin auth login`, `muse login`).
- Claude Code's login is read from the Keychain on a Mac (the first run asks
  you once to allow it), and from `%USERPROFILE%\.claude\.credentials.json` on
  Windows.
- Enough time left on each login. A run refuses to start when an access token
  ends before the case's planned length (ten minutes unless the case says more)
  plus a ten-minute margin. Use the harness yourself to renew it, then run again.

### The command

```sh
cargo xtask live                          # no name: lists the live cases
cargo xtask live <name> [harness ...]     # one case, on the harnesses named
cargo xtask live <name> --help            # the command's help
```

`<name>` is a file `crates/cf-e2e/tests/live_<name>.rs`, written with dashes or
underscores. The harness words are `claude`, `codex`, `pi`, `devin` and `muse`.
A case that is given none runs every harness it covers, so name only the ones
you have.

```sh
cargo xtask live first-message claude codex
cargo xtask live approval claude pi --keep
cargo xtask live matrix --models pi=openai/gpt-6-luna,codex=gpt-6.1-sol:high
```

The words after the name:

| Word | What it does |
| --- | --- |
| `claude`, `pi`, ... | the harnesses to seat; the same as `--harness <word>` |
| `--keep` | leaves what the run made where it is, for you to look at (below) |
| `--models h=model[:effort],...` | each harness named runs on that model and effort; the others on their cheap one |
| `--<flag> <value>` | sets `CONSENSFLOW_LIVE_<FLAG>` for the case; a flag given twice gets both values, with a comma |
| `--offline`, `--locked`, `--frozen` | cargo's own switches: they and what follows go to cargo |
| `-- <words>` | the rest goes to the test program |

`--offline` is cargo's switch for building without the network. It does not
make the run offline: the harnesses still call their models. Cases run one at a
time (`--test-threads=1`): two live Claude Code windows at once break Claude
Code's one-session rule.

### Sets

```sh
cargo xtask live --set windows
```

A set runs several live cases one after another, says what each came to, and
fails if any did. `windows` is the set a release runs on a Windows machine.

### What it builds first

Before any case starts, `cargo xtask live` builds the programs the cases need,
each with its own `cargo build --release`: `cf`, the pane host
`consensflow-bridge`, and the test programs `harness-ask` and `engine-ask` (on
Windows, `cf-shim` as well). They go into `app/src-tauri/target/release`, the
checkout's shared build folder. Then `cargo test` compiles the live suite and
the stand-in programs into `app/src-tauri/target/debug`.
A case builds nothing itself; a program it finds missing stops it, by name.

### Where runs keep their folders

- **The temp folder** (`$TMPDIR` on a Mac, `%TEMP%` on Windows): each run's rig
  folder, its login copies and its harness homes.
- **`~/.consensflow-candidate/live`** (`%USERPROFILE%\.consensflow-candidate\live`):
  the project folders the cases work in.
- The live matrix also writes its evidence under
  `app/src-tauri/target/live-matrix/<run-id>/`.

A run that passes takes the project folders it made away. A run that fails, or
was given `--keep`, leaves them, marked as kept. `--keep` also leaves the run's
home. Claude Code's config folder still goes at the end of an acting run,
because it holds a copy of a login: its conversations and sessions are copied
into the kept home first, never the login.

### Say-only runs

```sh
CF_LIVE_AUDIT=1 cargo xtask live first-message claude          # macOS
```

```powershell
$env:CF_LIVE_AUDIT = '1'; cargo xtask live first-message claude   # Windows
```

With `CF_LIVE_AUDIT=1` the kit ends no process, closes no window and removes no
file. It writes down instead what it would have done, each act with its
reason, in an audit file. That file is `CF_LIVE_AUDIT_FILE` if you set it, or
`consensflow-live-audit-<process id>.txt` in the temp folder. It must not be
there already: the run makes it new, and stops before its builds if it exists.
With `--set`, each run gets its own file: the file's name with `-1`, `-2`, ...
added.

The file's first lines say what a say-only run **still does**. Read them. In
short: it still builds, still spends real quota, the app's own shutdown still
ends its daemon and panes, and the harnesses still run and do what they do. It
keeps everything it made, login copies included, until you remove it.

A say-only run is never a pass: it always exits with a code that is not 0.

## What the kit protects

The kit is built around one rule: **a live run never hands a harness a folder
of your account, and never makes, moves or removes anything in your harness
stores.** Two "doors" hold it. Every program the kit starts goes through one,
and every file it writes or removes goes through the other.

### A home of its own for every harness

Every harness finds its folders through environment variables. The kit sets
every one of them to a folder the run made:

`HOME`, `USERPROFILE`, `CFFIXED_USER_HOME`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
`PI_CODING_AGENT_DIR`, `PI_CODING_AGENT_SESSION_DIR`, `XDG_CONFIG_HOME`,
`XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `APPDATA`, `LOCALAPPDATA`,
and `XDG_RUNTIME_DIR` where it is set.

### The door for starting programs

Before a program starts, the kit checks the whole environment it will get (it
"seals" it). The seal holds only when:

- every home variable is there and names a folder this run made;
- no other variable, except the search path, names a path inside one of your
  harness stores;
- no variable is given twice in different case.

A program that fails the seal does not start. This applies to a program the kit
starts, a window the app opens for a case, and a question the kit asks a helper.

Each harness also gets its update switches, so it does not update or install
itself during a run: `DISABLE_AUTOUPDATER` and `DISABLE_UPDATES` for Claude
Code, `PI_OFFLINE` for Pi, `MUSE_NO_AUTO_UPDATE` for Muse Code. Codex and Devin
are told by their run config.

**On Windows**, a new terminal also picks up the variables the account stores
in the registry (what `setx` writes). The kit names each of them in the
window's request, so the window has exactly the environment the seal checked.
It reads only their names, never a value, and writes nothing there.

### The door for writing and removing files

Before every write, new folder or removal, the kit resolves the path (through
every link) and checks it:

- Nothing is made, written, moved or removed inside a harness store. A store
  that does not exist yet is protected too: nothing is made in a missing
  `~/.codex`.
- A removal, or a move, is refused where the folder holds a store.
- A write must be below a folder this run made, or be a new file the run makes.
- A removal must be of a folder this run made, or below one. Nothing is removed
  by its name alone.

Where you have moved a store with a variable (`CODEX_HOME`, `APPDATA`, the
`XDG_*` folders), both the folder it names and the default folder are protected.

The stores the kit protects:

| Whose | macOS | Windows |
| --- | --- | --- |
| Claude Code | `~/.claude`, `~/.claude.json`, the folder `CLAUDE_CONFIG_DIR` names | `%USERPROFILE%\.claude`, `%USERPROFILE%\.claude.json` |
| Codex | `~/.codex`, the folder `CODEX_HOME` names | `%USERPROFILE%\.codex`, the folder `CODEX_HOME` names |
| Pi | `~/.pi`, the folders `PI_CODING_AGENT_DIR` and `PI_CODING_AGENT_SESSION_DIR` name | the same, below `%USERPROFILE%` |
| Devin | `devin` in `~/.config`, `~/.local/share`, `~/.local/state`, `~/.cache` | `%APPDATA%\devin`, `%LOCALAPPDATA%\devin` |
| Muse Code | `muse` in the same four folders, `~/Library/Application Support/Muse`, its runtime folders | `muse` in `.config`, `.local\share`, `.local\state` and `.cache` below `%USERPROFILE%` |
| agy | `~/.gemini`, its launcher `~/.local/bin/agy`, the skills folder `~/.agents`, and gcloud's folder (`~/.config/gcloud`, `CLOUDSDK_CONFIG`, `GOOGLE_APPLICATION_CREDENTIALS`) that its other login can use | the same below `%USERPROFILE%`, and `%APPDATA%\gcloud` |
| your account | `~/Library/Keychains` | |

### Processes

The kit ends only processes the run started: what descends from its own
windows and test process, a program it started and still holds, or a leftover
of its own that runs in the run's own folder. A name alone is never enough.

### Logins

A run never uses your login files directly. The kit copies each login it needs
into the run's own home, and takes the refresh token out of the copy: every key
whose name contains `refresh` is removed. A copy can be used until its access
token ends, and cannot renew itself, so it can never use up your refresh token
and sign you out. A Devin login that still holds such a key is not copied, and
the case refuses to run.

The kit prints no login values.

On a Mac, Claude Code may move a run's access token into a Keychain item named
for the run's folder. That item outlives the run, and the kit does not delete
it.

### Checked before and after

What the kit cannot prevent, it checks. Before a run and after it, the kit
compares your harness setups:

- login files by size, change time and file identity, without reading them
  (a Keychain login is read again and compared in memory);
- settings and config files by a hash;
- records folders (Claude Code's projects and history, Pi's sessions) for names
  the run added;
- the installed harness programs, their links and version folders, by a hash.

A difference fails the run and names the path, never a login value. The full
list of what each case reads is in `cargo xtask live --help` and in the first
lines of a say-only run's audit file.

## Test machines

A few controls must use the login store of the account itself, not a copy:

- **The login renewal** (Claude Code and Pi). When a login has run short, the
  kit runs the harness once on the account's own login, with a one-word prompt,
  no tools and no saved session, so the harness renews its own token.
- **The Keychain control** (macOS, `cargo xtask live keychain`). It checks what
  Claude Code leaves in the login Keychain, by making items of its own and
  deleting them. It spends no quota: Claude Code talks to a model server of the
  case's own.
- **A real Muse Code window on a Mac.** Elsewhere on a Mac, Muse runs as its
  stand-in.

These are refused unless the machine is marked as a test machine. A renewal
writes the account's login and can rotate its refresh token. On your own daily
account that could sign you out of the harness. A say-only run never renews a login: where one would be
needed, the run ends and says so.

### Marking a test machine

Set `CONSENSFLOW_TEST_MACHINE` to any non-empty value, for example the
machine's name:

```sh
export CONSENSFLOW_TEST_MACHINE=my-test-vm          # macOS
```

```powershell
$env:CONSENSFLOW_TEST_MACHINE = 'my-test-vm'        # Windows
```

**Do this only on an account or a virtual machine of its own:** not your daily
account, with its own logins for the harnesses, which you are willing to have
the kit renew.

The marker lets the renewal, the Keychain control and a real Muse window on a
Mac run. Everything else the kit does is the same, marked or not.

The maintainers' Windows runner gives the marker only on machines it records as
test machines. It refuses the marker as an `--env` word, and on any other
machine it takes away, for the run, a marker the account itself holds.

## What the doors cannot hold

The doors check what the kit hands a program. They cannot stop:

- **A harness that ignores its environment.** One that finds its home by the
  user's number, or the Keychain by the login session, reaches your files
  whatever the kit sets. So does a launcher that writes beside its own program
  where that is inside its store. On a Mac, Muse runs inside a file sandbox. For
  the others, the checks after the run find such a change; they do not prevent
  it.
- **The app's own code.** The daemon a case starts runs ConsensFlow's code.
  Its windows are sealed, but programs it starts for itself are not.
- **Another process** that changes a path between the kit's check and its act.
- **A secret store no path names.** Windows' Credential Manager (where agy keeps
  its login on Windows), the Secret Service of a Linux desktop and a Mac's
  Keychain are reached through your login session, not through a folder or a
  variable. The kit's own code never names one except `door::keychain`, which
  reads the Keychain through `security`: its audit finds a call to the
  Credential Manager's or the Keychain's functions, the `keyring` crate,
  `cmdkey`, `vaultcmd` and `secret-tool`. What a harness does there itself, the
  kit cannot prevent.
- **A program reached through a shell.** A window or a start whose program is a
  shell (`sh -c`, `cmd /c`, `powershell -Command`) is judged by the program its
  command starts. A command the shell would read for itself (an operator, a
  substitution, a variable) cannot be judged, and is refused.
- **A test run by hand.** `cargo xtask live` clears `CF_LIVE_MADE_ROOTS` and
  `CF_LIVE_PROTECTED_FILE`, which tell a run what it made and what it protects.
  A `cargo test` started directly with them set is not protected the same way.
  Always start a live case through `cargo xtask live`.

The only full protection is another user, or a virtual machine. On your daily
account, run a case say-only first (`CF_LIVE_AUDIT=1`), read the audit file,
and only then run it for real.

## When a run leaves something

A failed or stopped run can leave folders behind. List them:

```sh
cargo xtask live sweep
cargo xtask live sweep --older-than 2
cargo xtask live sweep --in <folder> --home <folder>
```

`sweep` lists each folder that earlier runs left (rig folders, login copies
and harness homes in the temp folder, project folders in
`~/.consensflow-candidate/live`), with what it is, how old it is, and why you
might keep it. `--in` names another temp folder, `--home` another home, and
`--older-than` a number of days.

It removes nothing. A folder may belong to a run still going, or hold evidence
someone kept. Remove what you choose yourself. Login copies hold an access
token, so remove those you no longer need.

## Reporting a problem

If a live run changed something in your harness setup, failed a before/after
check, or left something `sweep` does not list, open an issue at
<https://github.com/ngvoicu/consensflow/issues>. Say:

- the command, and your platform (macOS or Windows) and harness versions;
- the line the kit printed (it names a path, never a login value);
- for a say-only run, the audit file.

Check the audit file and the output for anything private before you post them:
they hold paths and whole command lines of your machine.
