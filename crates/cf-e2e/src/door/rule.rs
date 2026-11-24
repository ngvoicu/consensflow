//! The rule that the live kit ends a process, closes a window, takes a file away and writes a file
//! only through its door ([`super::Door`], and the writers of [`crate::door`]), read from the
//! kit's Rust as syntax.
//!
//! On 2026-11-12 a live run of a kit branch ended 22 processes that were none of its own, and
//! its review (T-1557) found a rig's start taking other runs' folders and a renewal taking the
//! owner's records by the end of their names; the reviews after it found writes reaching other
//! runs' files as well (T-1594). Every ending and every removal of the live kit now asks the
//! door, which acts only on its grounds and, in a say-only run, says instead
//! ([`super::AUDIT`]); every write asks one of its writers, which allow a path only below a
//! folder this run made (by its real path, never through a link) or a file it made new
//! ([`super::writable`]). What a caller hands the kit (`--claude-config`, `--fixture`, a
//! folder it names for an export) is read, never written. This rule reads the live kit's code
//! (`src/live`, `src/rig`, `src/matrix`, `src/bin` (the stand-in programs), `src/process*`,
//! `src/daemon.rs` and the `tests/live_*` suites: all of `src` but what the door is made of, and
//! the folders of tests that are none of a live run's) and fails on any other way to do either:
//!
//! - a call of `remove_file`, `remove_dir`, `remove_dir_all`, the descent's `end_chosen` /
//!   `pause_chosen` (and their `_by`), `signal`, `stall`, `end_all`, libc's or nix's `kill` and
//!   `killpg`, the descent's `.hold(pid)` of a process to end, or Windows' `TerminateProcess` /
//!   `TerminateJobObject`;
//! - a string that runs a program that ends processes (`kill`, `pkill`, `killall`, `taskkill`,
//!   `tskill`, `Stop-Process`) or asks the pane host to end a pane (`pane.kill`);
//! - `.kill()` in a file that holds a child program itself (names a type `Child`), but on
//!   `self` (a type of the kit's own whose `kill` goes through the door);
//! - a path or an import of the `tempfile` crate: its folders and files are taken away when
//!   they are dropped, past the door and in a say-only run too (the door's own are
//!   [`super::made`] and [`super::made_in`]);
//! - a write that does not ask the door's writers: `std::fs::write`, `copy`, `rename`,
//!   `hard_link`, `create_dir[_all]`, `set_permissions`, a symlink, `File::create[_new]`, an
//!   `OpenOptions` opened for writing, `set_len`, `set_modified` or `set_times`, the plain writers
//!   of [`crate::files`], and a program named in a call that starts one to write or move
//!   (`cp`, `mv`, `tee`, `mkdir`, …), including through an import (`use std::fs::{write, …}`).
//!
//! A string compared (`==`, `!=`) is read, not run; documentation is not code.
//!
//! The door's own code (`src/door*`), `src/files.rs`, the descent's (which ends one process it has
//! read again, for the door) and the process primitives (`src/process/pid.rs`) are what the door is
//! made of, and are not read; nor is code only a unit test builds (`#[cfg(test)]`, and the module
//! files such an attribute gates), which runs on folders and programs it made, nor the builder of
//! the programs a suite starts (`src/cf.rs`: it runs `cargo build` in the checkout and keeps a lock
//! and a stamp in the folder the builds share, which no run of the live kit makes; `cargo xtask
//! live` builds first). What a closure given to the door does is the door's to allow. Macros are
//! read token by token: a call named above, followed by its arguments, is one.
//!
//! What it cannot see, and what the door's guards do not stand for either (T-1592): a program or a
//! file named in a string that is built (a shell line made from parts, `format!`ed into a
//! command), a call made through a function value, a trait object or another crate's wrapper
//! (a write another crate does for the kit), a raw system call, an ending or a removal that a
//! destructor does (a dropped terminal hangs its program up, a dropped `tempfile` folder is
//! taken, a dropped child of another crate may be killed), the folders the rule does not read
//! (`src/files.rs`, `src/cf.rs`, `tools/xtask`, the product's own crates, and tests that are no
//! `live_*`): those that start the kit's stand-ins run on folders they make through the door
//! as well, and the programs the run starts, harnesses and `git` among them, write as they
//! write: what they may reach is the isolation's (`live::isolation`, `live::login`) to hold, with
//! the check at the end that the owner's own files are as they were. The door's writers refuse a
//! path outside the run's allocations, so a mistake the syntax rule cannot see is an error at the
//! write, and the kit's tests say what a run did by running it (`tests/rig/layout.rs`); this
//! reads the code for the ways that are plain.

use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{
    Attribute, BinOp, Expr, ExprBinary, ExprCall, ExprMethodCall, ItemFn, ItemMod, ItemUse, LitStr,
    Macro, Meta, UseTree,
};

/// The calls that end, pause or signal a process, or take a file away.
const CALLS: &[&str] = &[
    "end_chosen",
    "end_chosen_by",
    "pause_chosen",
    "pause_chosen_by",
    "signal",
    "killpg",
    "stall",
    "end_all",
    "TerminateProcess",
    "TerminateJobObject",
];

/// The calls that take a file or a folder away: a removal is a write, and asks the write door
/// ([`crate::door::remove`], or the door's own removals).
const REMOVALS: &[&str] = &["remove_file", "remove_dir", "remove_dir_all"];

/// The names that start a program on a pseudo-terminal of the kit's own: the product's pane table
/// (`cf_panes`), `portable_pty`'s system, command builder and spawn. A live case opens a window
/// only on a rig's pane host (`Rig::open_pane`), through the seal; a program on a pty of its own
/// has no seal, no update switches and the test process's whole environment (T-2119 X1).
const PTY_STARTS: &[&str] = &[
    "PaneTable",
    "native_pty_system",
    "CommandBuilder",
    "openpty",
    "spawn_command",
];

/// The calls that start a program, by their names alone, outside `std::process`: the system's own.
const START_CALLS: &[&str] = &["fork", "posix_spawn", "posix_spawnp", "execv", "execve", "execvp"];

/// The functions of `std::fs` that read: what a held file ([`READS_HELD`]) does through the door's
/// readers ([`crate::door::reads`]) for a place of the run's, and through a
/// [`crate::door::source::Source`] for the owner's.
const FS_READS: &[&str] = &[
    "read",
    "read_to_string",
    "read_dir",
    "metadata",
    "symlink_metadata",
    "canonicalize",
    "read_link",
];

/// The methods of a path that read the file system, with no argument.
const PATH_READS: &[&str] = &[
    "exists",
    "try_exists",
    "read_dir",
    "read_link",
    "canonicalize",
    "symlink_metadata",
    "metadata",
];

/// The files that touch the owner's stores, and read nothing of the file system but through the
/// door's readers or a source: every other read of theirs is found. A path is a prefix of a file's,
/// from the crate's own folder. The files left out are those of the offline suites and the stand-in
/// programs, which read the folders they made; the unit tests of a held file are not read.
const READS_HELD: &[&str] = &[
    "src/live/isolation",
    "src/live/login",
    "src/live/launcher",
    "src/live/layout.rs",
    "src/live/guard.rs",
    "src/live/machine.rs",
    "src/live/renew.rs",
    "src/live/keychain.rs",
    "src/muse.rs",
    "src/rig/install.rs",
    "src/rig/homes.rs",
    "src/matrix/evidence.rs",
    "tests/live_harness_dialogs.rs",
];

/// The files that ask the account for a source of the owner's files ([`crate::door::source`]):
/// each reads for one of its three purposes (the seed of a run's home, a marketplace or package it
/// copies, the guard of the owner's setup), and the rig's placement of the person's project
/// folders. No other file names the account.
const OWNER_FILES: &[(&str, &str)] = &[
    (
        "src/live/machine.rs",
        "where the machine's harness is found, its login looked for and the home its setup is read from",
    ),
    (
        "src/live/isolation.rs",
        "the seeds of a harness's home and the owner's files it watches",
    ),
    (
        "src/live/launcher.rs",
        "the owner's launchers it hashes around a run",
    ),
    (
        "src/live/login.rs",
        "the caller's config folder a Claude login is copied from",
    ),
    (
        "src/matrix/evidence.rs",
        "the owner's launcher links, Muse and Claude files the matrix compares before and after a run",
    ),
    (
        "src/live/agents.rs",
        "Pi's login, read once in memory to name the providers a run's Pi model is asked of",
    ),
    (
        "src/muse.rs",
        "Muse's login and catalog copied into a live rig's home, and the launcher's version line",
    ),
    (
        "tests/live_harness_dialogs.rs",
        "the marketplace of the owner's Claude Code the dialog case copies, and the owner's `.claude.json` it compares around the run",
    ),
    (
        "tests/live_muse/tui.rs",
        "the owner's Muse login a direct window case copies into its own home",
    ),
    (
        "tests/live_muse_reads.rs",
        "the owner's Muse login and catalog the reads case copies into a scratch folder",
    ),
    (
        "tests/live_keychain.rs",
        "the guard of the owner's launchers the Keychain control runs under",
    ),
    (
        "tests/live_launcher.rs",
        "the owner's home a launcher case says its folders below",
    ),
    (
        "tests/live_finish/compacted.rs",
        "the decoy owners the compacted case's layouts are checked against",
    ),
    (
        "tests/rig/muse_window.rs",
        "the decoy owner a Muse window case's Devin isolation copies from",
    ),
    (
        "tests/rig/standin_homes.rs",
        "the decoy home a launcher watch must refuse",
    ),
    (
        "tests/rig/harness_homes.rs",
        "the decoy owners the rig's harness homes are copied from",
    ),
];

/// The calls of the account's secret stores that no path or variable names ([`crate::live::stores::SECRET_STORES`]):
/// Windows' Credential Manager, the Mac's Keychain by its API. The kit's own code names none: the
/// Keychain is read by `/usr/bin/security`, in the one door that starts it.
const SECRET_STORE_CALLS: &[&str] = &[
    "CredReadW",
    "CredReadA",
    "CredEnumerateW",
    "CredEnumerateA",
    "CredWriteW",
    "CredWriteA",
    "CredDeleteW",
    "CredDeleteA",
    "SecItemCopyMatching",
    "SecItemAdd",
    "SecItemUpdate",
    "SecItemDelete",
    "SecKeychainFindGenericPassword",
    "SecKeychainFindInternetPassword",
];

/// The crates that read a secret store of the account's.
const SECRET_STORE_CRATES: &[&str] = &["keyring", "security_framework", "secret_service", "libsecret"];

/// The programs that read or write one, named in a string (the `security` utility is the
/// Keychain's door's alone).
const SECRET_STORE_PROGRAMS: &[&str] = &["cmdkey", "vaultcmd", "secret-tool"];

/// Whether `text` names a program that reads or writes a secret store of the account's.
fn names_a_secret_store_program(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ';' | '&' | '|' | '(' | ')'))
        .filter(|word| !word.is_empty())
        .any(|word| {
            let name = word
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(word)
                .to_ascii_lowercase();
            let name = name.strip_suffix(".exe").unwrap_or(&name);
            SECRET_STORE_PROGRAMS.contains(&name)
        })
}

/// The ways to open a SQLite database, which can write: its journal, its WAL, its shared memory.
/// A read goes through `door::database::read`, a write through `door::database::write`.
const DATABASE_OPENS: &[&str] = &["open", "open_with_flags", "open_with_flags_and_vfs"];

/// xtask's generic runs, which name any program: its live run starts `cargo` only, by
/// `process::cargo` and `process::run_cargo`.
const XTASK_RUNS: &[&str] = &["run", "capture", "spawn"];

/// Where the rule does not look for a class of act, and why. A path is a prefix of a file's, from
/// the crate's own folder. What the door is made of is not held to the door, but for the one thing
/// that holds it to its own files: no operation is made but in the three that make them
/// ([`WAIVERS`]).
const LEAVES_OUT: &[(&str, &[Class], &str)] = &[
    (
        "src/door",
        &[Class::End, Class::Write, Class::Start, Class::Env, Class::Database, Class::Account],
        "the door is what the rule holds the rest to",
    ),
    (
        "tests/rig",
        &[Class::End],
        "the offline suites end only processes they started, by their own ids: the test audit's (P21t)",
    ),
    (
        "tests/regressions",
        &[Class::End],
        "the offline suites end only processes they started, by their own ids: the test audit's (P21t)",
    ),
    (
        "tests/smoke",
        &[Class::End],
        "the offline suites end only processes they started, by their own ids: the test audit's (P21t)",
    ),
    (
        "tests/cli",
        &[Class::End],
        "the offline suites end only processes they started, by their own ids: the test audit's (P21t)",
    ),
    (
        "tests/daemon",
        &[Class::End],
        "the offline suites end only processes they started, by their own ids: the test audit's (P21t)",
    ),
    (
        "src/bin",
        &[Class::Window],
        "the stand-in programs give their children the environment their window gave them",
    ),
    (
        "src/live/members/descent",
        &[Class::End],
        "the descent ends the one process it has read again, for the door",
    ),
    (
        "src/process/pid.rs",
        &[Class::End],
        "the process primitives the door's endings are made of",
    ),
];

/// A place the rule allows, for one operation in one function of one file. A waiver is not a
/// file's: a second act in the same function is found. Each is listed, with its reason, and a
/// waiver that matches nothing fails the test.
const WAIVERS: &[(&str, &str, Class, &str)] = &[
    (
        "src/process.rs",
        "command_on",
        Class::Start,
        "the spawn door's own start: every program of the kit is started here, on the environment `Run::environment` sealed",
    ),
    (
        "src/process.rs",
        "command_on",
        Class::Env,
        "the sealed environment is given whole to the builder, and nothing else is",
    ),
    (
        "src/process/spawned.rs",
        "sh",
        Class::Start,
        "a test's own shell, holding only its streams, to show what a `Spawned` handle does with the child it is given",
    ),
    (
        "src/process/tests.rs",
        "the_programs_the_settle_test_starts",
        Class::Start,
        "a test of the process primitives starts a program of its own to settle, with the std builder those primitives are made for",
    ),
    (
        "src/process/tests.rs",
        "the_programs_the_settle_test_starts",
        Class::Env,
        "the same program's one variable, set on the std builder it is started with",
    ),
    (
        "src/process/tests.rs",
        "the_group_of_a_process_is_the_one_it_was_started_in_and_one_that_is_gone_has_none",
        Class::Start,
        "a test of the process primitives reads the group of a program it starts itself",
    ),
    (
        "src/muse.rs",
        "the_script_of_the_mac_answers_as_the_program_of_windows_does",
        Class::Start,
        "a test runs the script it wrote with bash, feeding it requests on a pipe",
    ),
    (
        "src/bin/fake_agent/background.rs",
        "run_in_session",
        Class::Start,
        "the one place a program is run in place of this one (`exec`), which no `Run` does: the stand-in's shell of a detached command, as a harness starts one",
    ),
    (
        "src/bin/fake_codex/children.rs",
        "launch",
        Class::Start,
        "the launcher leaves one program running on its own with no stream of its own: a `Run` handle would end it, and a stream would hold its caller's pipes",
    ),
    (
        "src/standin_process.rs",
        "unowned_tempdir",
        Class::Write,
        "the decoy folder the door's refusals are shown on: made where the run owns nothing, by design, so that a write there is refused",
    ),
    (
        "src/standin_process.rs",
        "foreign_write",
        Class::Write,
        "a file of the decoy folder, made as the foreign party made it: the door would refuse it, which is what the tests then show",
    ),
    (
        "src/standin_process.rs",
        "foreign_dir",
        Class::Write,
        "a folder of the decoy folder, made as the foreign party made it",
    ),
    (
        "tools/xtask/src/live/tests.rs",
        "the_live_tests_there_are_the_files_of_the_tests_folder_that_begin_with_live",
        Class::Write,
        "xtask's own unit test makes a tests folder of its own: xtask is another crate, which the run's doors are not (cf-e2e's), and it writes below the folder it made",
    ),
    (
        "tools/xtask/src/live/tests.rs",
        "what_a_say_only_run_left_is_told_as_acts_without_its_first_lines_and_never_as_a_pass",
        Class::Write,
        "xtask's own unit test makes an audit file of its own, below a folder it made, in another crate than the doors'",
    ),
    (
        "src/door/cargo.rs",
        "cargo_in",
        Class::Operation,
        "cargo's operation is made where its words and its variables are fixed",
    ),
    (
        "src/door/keychain.rs",
        "security",
        Class::Operation,
        "the Keychain's operation is made where its program is fixed, and every caller is a named function of that file",
    ),
    (
        "src/door/renewal.rs",
        "start",
        Class::Operation,
        "the renewal's operation is made where it asks for the test machine's token, the harness, and a folder this run made",
    ),
    (
        "src/cf.rs",
        "made",
        Class::Write,
        "the stamp of a build, in the folder the builds share, below the checkout's build folder: no folder of a run's, so the door cannot make it",
    ),
    (
        "src/cf.rs",
        "taking_turns",
        Class::Write,
        "the lock the builds take turns on, in the same shared folder",
    ),
    (
        "src/cf.rs",
        "a_program_the_run_built_and_that_is_gone_is_built_again",
        Class::Write,
        "a unit test takes away a program it built in a folder of its own, in the crate's own builder",
    ),
    (
        "src/door/keychain.rs",
        "start_claude",
        Class::Operation,
        "the control's operation is made where it asks the account's home and the config folder this run made",
    ),
    (
        "src/door/keychain.rs",
        "the_control_starts_claude_on_the_accounts_home_and_a_folder_this_run_made_and_no_other",
        Class::Operation,
        "the control's own test starts it with a stand-in program",
    ),
    (
        "src/live/login.rs",
        "read",
        Class::Operation,
        "the Keychain's read of Claude Code's login item, for the seed of a run's login copy: the one caller of `login_item`",
    ),
    (
        "src/live/login.rs",
        "keychain_services",
        Class::Operation,
        "the Keychain's item names, for the guard that compares them around a run: the one caller of `item_names`",
    ),
    (
        "src/live/renew.rs",
        "one_shot_within",
        Class::Operation,
        "the renewal's one start, behind the test machine's token: the one caller of `renewal::start`",
    ),
    (
        "src/live/renew/tests.rs",
        "the_renewals_door_runs_two_harnesses_in_a_folder_the_run_made",
        Class::Operation,
        "the test of the renewal's door asks it for its two harnesses",
    ),
    (
        "src/process.rs",
        "terminal",
        Class::Operation,
        "`Run::terminal`: the one caller of the pty door, after the seal and the admission every start asks",
    ),
    (
        "tests/live_keychain.rs",
        "run",
        Class::Operation,
        "the Keychain control's start of Claude Code on the account's home, behind the control's token",
    ),
    (
        "src/bin/fake_agent/background.rs",
        "running",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/bin/fake_agent/background.rs",
        "in_a_shell_as",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/bin/fake_agent/reply.rs",
        "shell",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/bin/fake_devin/command.rs",
        "start",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/door/k3_tests.rs",
        "a_plain_start_through_a_shell_is_judged_by_the_program_the_shell_is_told_to_run",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/live/codex.rs",
        "server",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/live/codex.rs",
        "a_request_the_server_does_not_answer_is_a_timeout_that_names_it",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/live/members/command.rs",
        "a_process_outside_the_window_that_holds_the_mark_runs_on_and_the_windows_own_command_is_ended",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/live/members/command.rs",
        "what_a_cut_window_left_of_the_command_is_ended_by_the_processes_chosen_while_it_was_the_windows",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/live/members/left/tests.rs",
        "a_real_monitor_pipeline_left_by_a_killed_window_is_found_and_not_ended_and_what_runs_elsewhere_is_not_found",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/process/tests.rs",
        "a_program_a_case_starts_holds_nothing_of_the_rigs_but_its_streams",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/process/tests.rs",
        "sh",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/rig/stranger.rs",
        "a_look_at_how_a_stranger_ended_waits_for_no_pipe_a_process_it_left_holds",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/live_finish/leaving/tests.rs",
        "a_process_that_runs_is_there_and_one_that_never_was_is_not",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/live_finish/leaving/tests.rs",
        "a_job_a_shell_detached_and_left_is_found_by_the_runs_folder_and_ended_by_which_process_it_is",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/live_finish/leaving/tests.rs",
        "a_command_of_this_runs_word_that_detached_unseen_and_runs_in_another_folder_escaped_and_fails_the_verdict",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/live_finish/stand_in.rs",
        "the_close_of_a_case_finds_what_its_window_left_says_so_and_ends_nothing_and_leaves_another_runs_job",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/live_parity/codex_devin.rs",
        "muse_s_case_gives_the_human_s_settings_a_server_and_a_hook_that_says_its_word",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/live_stops/hook.rs",
        "a_hook_of_another_run_on_the_machine_is_not_the_hook_the_case_waits_for",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/live_stops/slow.rs",
        "a_process_of_another_run_that_holds_the_hooks_old_word_is_not_the_hook_the_case_looks_for",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/rig/homes.rs",
        "a_rig_that_starts_takes_no_home_and_the_sweep_lists_the_old_ones_and_says_which_a_process_works_in_or_names",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/rig/live_member.rs",
        "the_check_ends_the_command_its_window_runs_and_nothing_outside_the_window_that_says_the_same",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/smoke/app.rs",
        "app",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "tests/smoke/app.rs",
        "a_report_that_does_not_come_is_a_timeout_that_says_what_did_and_where_to_look",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/scratch_home.rs",
        "a_program_run_by_the_home_sees_its_variables_and_none_of_the_tests",
        Class::Operation,
        "an offline fixture's own script, started through `Run::fixture_script`",
    ),
    (
        "src/process/pid.rs",
        "when_started",
        Class::Operation,
        "a script of the kit's own or a fixture's, started through `Run::script_in`",
    ),
    (
        "src/live/members/descent.rs",
        "table_run",
        Class::Operation,
        "a script of the kit's own or a fixture's, started through `Run::script_in`",
    ),
    (
        "tests/live_terminal.rs",
        "console_hosts",
        Class::Operation,
        "a script of the kit's own or a fixture's, started through `Run::script_in`",
    ),
    (
        "tests/rig/cf_native.rs",
        "take_a_many_line_argument_whole_from_powershell",
        Class::Operation,
        "a script of the kit's own or a fixture's, started through `Run::script_in`",
    ),
];

/// The functions of `std::fs` (and `std::os::*::fs`) that write, copy, rename, link, make or set
/// something in the file system: what only the door's writers ([`crate::door`]) may do in the
/// kit's code, as they refuse a path that is no part of this run's allocations.
const FS_WRITES: &[&str] = &[
    "write",
    "copy",
    "rename",
    "hard_link",
    "soft_link",
    "create_dir",
    "create_dir_all",
    "set_permissions",
    "symlink",
    "symlink_file",
    "symlink_dir",
];

/// The constructors of `File` and `OpenOptions` that open for writing: an `OpenOptions` is read as
/// one, but for a chain that is made and opened at once and says nothing of writing
/// ([`OPENS_FOR_WRITING`]), which only reads.
const FILE_WRITES: &[&str] = &["create", "create_new", "options"];

/// What an `OpenOptions` chain says to open for writing: without one of these it opens to read.
const OPENS_FOR_WRITING: &[&str] = &["write", "append", "create", "create_new", "truncate"];

/// The methods that change a file or its folder's entry by a handle or a path.
const METHOD_WRITES: &[&str] = &["set_len", "set_modified", "set_times", "set_permissions"];

/// The programs that write, copy, move or make in the file system, named in a call that starts one.
const WRITING_PROGRAMS: &[&str] = &[
    "cp", "mv", "install", "rsync", "tee", "dd", "ln", "touch", "mkdir", "truncate", "ditto",
    "robocopy", "xcopy", "copy", "move", "mklink",
];

/// Whether the path `segments` is one that reads the file system other than through the door's readers.
fn reads_raw(segments: &[String]) -> bool {
    let last = segments.last().map(String::as_str).unwrap_or_default();
    let before = segments.iter().rev().nth(1).map(String::as_str);
    match before {
        Some("fs") => FS_READS.contains(&last),
        Some("File") => last == "open",
        _ => false,
    }
}

/// Whether the path `segments` is one that writes in the file system other than through the door.
fn writes_raw(segments: &[String]) -> bool {
    let last = segments.last().map(String::as_str).unwrap_or_default();
    let before = segments.iter().rev().nth(1).map(String::as_str);
    match before {
        Some("fs") => FS_WRITES.contains(&last),
        Some("File") => FILE_WRITES.contains(&last),
        Some("OpenOptions") => last == "new",
        _ => false,
    }
}

/// The `OpenOptions::new()` (or `File::options()`) a chain that ends in `.open(…)` begins with, and
/// the names it calls between.
fn open_chain(call: &ExprMethodCall) -> Option<(&ExprCall, Vec<String>)> {
    let mut names = Vec::new();
    let mut at: &Expr = &call.receiver;
    while let Expr::MethodCall(inner) = at {
        names.push(inner.method.to_string());
        at = &inner.receiver;
    }
    let Expr::Call(root) = at else { return None };
    let Expr::Path(path) = &*root.func else {
        return None;
    };
    let named: Vec<String> = path
        .path
        .segments
        .iter()
        .map(|part| part.ident.to_string())
        .collect();
    let tail: Vec<&str> = named.iter().rev().take(2).map(String::as_str).collect();
    matches!(
        tail.as_slice(),
        ["new", "OpenOptions"] | ["options", "File"]
    )
    .then_some((root, names))
}

/// The door's methods that are handed what to do, done only where the door allows it.
const DOOR_CLOSURES: &[&str] = &["close_pane", "close_own_terminal", "quit_own_terminal"];

/// Where a string runs a program that ends processes, or asks the pane host to end a pane.
fn ends_in_words(text: &str) -> bool {
    let program = text.rsplit(['/', '\\']).next().unwrap_or(text);
    let program = program.strip_suffix(".exe").unwrap_or(program);
    matches!(
        program,
        "kill" | "pkill" | "killall" | "taskkill" | "tskill"
    ) || [
        "taskkill",
        "pkill ",
        "killall ",
        "Stop-Process",
        "pane.kill",
    ]
    .iter()
    .any(|word| text.contains(word))
        || sends_a_signal(text)
}

/// Whether `text` holds `kill -<signal>` that sends one: `kill -0` only asks whether the process
/// is there, and ends none.
fn sends_a_signal(text: &str) -> bool {
    text.match_indices("kill -").any(|(at, word)| {
        let after = &text[at + word.len()..];
        let mut chars = after.chars();
        let asks_only =
            chars.next() == Some('0') && !chars.next().is_some_and(|c| c.is_ascii_digit());
        !asks_only
    })
}

/// The kinds of act the rule holds to a door.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// A process ended or paused, a window closed, a file taken away.
    End,
    /// A write in the file system.
    Write,
    /// A program started, on a pipe or on a pty.
    Start,
    /// A program's environment set by a way that is not the door's.
    Env,
    /// A database opened (it can write its sidecar files) or written to a file of its own.
    Database,
    /// One of the door's operations, made outside the three files that make them.
    Operation,
    /// A child started on the environment of a stand-in's window, outside the stand-in programs.
    Window,
    /// A secret store of the account's that no path names (Windows' Credential Manager, the
    /// Keychain by its API), named by the kit's code ([`SECRET_STORE_CALLS`]).
    Account,
    /// The account asked for a source of the owner's files, outside the files named for it
    /// ([`OWNER_FILES`]).
    Owner,
    /// A read of the file system in a file that touches the owner's stores ([`READS_HELD`]), made
    /// outside the door's readers and a source.
    Read,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Class::End => "end",
            Class::Write => "write",
            Class::Start => "start",
            Class::Env => "environment",
            Class::Database => "database",
            Class::Operation => "operation",
            Class::Window => "window",
            Class::Read => "read",
            Class::Owner => "owner",
            Class::Account => "account",
        }
    }
}

/// One place the rule found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    file: String,
    item: String,
    class: Class,
    what: String,
}

impl std::fmt::Display for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}): {}", self.file, self.item, self.what)
    }
}

struct Reader {
    file: String,
    item: Vec<String>,
    /// Whether this file holds a child program itself (names a type `Child`).
    holds_children: bool,
    /// Whether this is a file of xtask, where its own runner names a program ([`XTASK_RUNS`]).
    xtask: bool,
    /// The classes this file is not held to (what the door is made of: [`LEAVES_OUT`]).
    skip: &'static [Class],
    /// Whether this is a file of [`READS_HELD`].
    holds_reads: bool,
    /// Whether this is a file of [`OWNER_FILES`], or of the door.
    asks_account: bool,
    /// How deep in code only a test builds this is (a file such a test module is, too): a process
    /// a test ends by its own id is the test audit's, not this rule's.
    in_test: usize,
    /// How deep in code a `cfg(test)` builds only: a `#[test]` function of a live suite is not
    /// that, and an ending written in one is found (T-2220 finding 4).
    in_cfg_test: usize,
    /// How deep in closures given to the door this is.
    in_door: usize,
    /// How deep in a comparison this is, where a string is read.
    reading: usize,
    /// The `OpenOptions::new()` calls that a chain opens to read, which write nothing.
    read_opens: std::collections::HashSet<usize>,
    found: Vec<Found>,
}

/// What a call of the path `segments` is, if it is an act the rule holds to a door.
fn path_call(segments: &[String], xtask: bool) -> Option<(Class, String)> {
    let last = segments.last().map(String::as_str).unwrap_or_default();
    let before = segments.iter().rev().nth(1).map(String::as_str);
    // An exit status's own signal is read, not sent.
    let read = last == "signal" && before == Some("ExitStatusExt");
    let libc_kill = last == "kill" && matches!(before, Some("libc" | "signal"));
    if REMOVALS.contains(&last) {
        return Some((
            Class::Write,
            format!("{}(…) (a removal that does not ask the write door)", segments.join("::")),
        ));
    }
    if (CALLS.contains(&last) && !read) || libc_kill {
        return Some((Class::End, format!("{}(…)", segments.join("::"))));
    }
    if last == "new" && before == Some("Command") {
        return Some((
            Class::Start,
            format!("{} (a program started outside the spawn door)", segments.join("::")),
        ));
    }
    if START_CALLS.contains(&last) || last.starts_with("CreateProcess") || last.starts_with("ShellExecute") {
        return Some((
            Class::Start,
            format!("{}(…) (a program started outside the spawn door)", segments.join("::")),
        ));
    }
    if before == Some("Connection") && DATABASE_OPENS.contains(&last) {
        return Some((
            Class::Database,
            format!("{}(…) (a database opened outside the database door)", segments.join("::")),
        ));
    }
    if matches!(
        (before, last),
        (Some("keychain"), "login_item" | "item_names")
            | (Some("renewal"), "start")
            | (Some("terminal"), "open")
    ) {
        return Some((
            Class::Operation,
            format!("{}(…) (a door's operation, made outside the files that make them)", segments.join("::")),
        ));
    }
    if before == Some("Run") && matches!(last, "fixture_script" | "script_in") {
        return Some((
            Class::Operation,
            "Run::fixture_script(…) (the one start through a shell whose command is not read, made outside the files that call it)".into(),
        ));
    }
    if before == Some("Run") && last == "operation" {
        return Some((
            Class::Operation,
            "Run::operation(…) (a door's operation, made outside the files that make them)".into(),
        ));
    }
    if xtask
        && before == Some("process")
        && XTASK_RUNS.contains(&last)
    {
        return Some((
            Class::Start,
            format!("process::{last}(…) (a generic start of a program in xtask's live run)"),
        ));
    }
    None
}

/// Whether `receiver` is a file's metadata or its type, which says what a file is without reading
/// the file system again: `meta.is_dir()`, `kind.is_symlink()`, `found.file_type().is_file()`.
fn asks_a_file_type(receiver: &Expr) -> bool {
    match receiver {
        Expr::MethodCall(call) => matches!(
            call.method.to_string().as_str(),
            "file_type" | "unwrap" | "expect" | "clone"
        ),
        Expr::Path(path) => path.path.get_ident().is_some_and(|name| {
            matches!(
                name.to_string().as_str(),
                "meta" | "kind" | "found" | "metadata" | "file_type" | "ty" | "kind_of"
            )
        }),
        Expr::Reference(inner) => asks_a_file_type(&inner.expr),
        Expr::Paren(inner) => asks_a_file_type(&inner.expr),
        _ => false,
    }
}

/// What a method called `name` with `args` arguments is, if it is an act the rule holds to a
/// door. A getter of the same name with no arguments (`.env()`) sets nothing.
fn method_call(name: &str, args: usize) -> Option<(Class, String)> {
    if METHOD_WRITES.contains(&name) {
        return Some((
            Class::Write,
            format!(".{name}(…) (a change made by a path or a handle)"),
        ));
    }
    if args == 0 && PATH_READS.contains(&name) {
        return Some((
            Class::Read,
            format!(".{name}() (a read of the file system outside the door's readers and a source)"),
        ));
    }
    if PTY_STARTS.contains(&name) || (name == "exec" && args == 0) {
        return Some((
            Class::Start,
            format!(".{name}(…) (a program started outside the spawn door)"),
        ));
    }
    if name == "start_claude" && args == 5 {
        return Some((
            Class::Operation,
            ".start_claude(…) (the Keychain control's start of Claude Code, made outside its case)".into(),
        ));
    }
    if name == "as_this_window" {
        return Some((
            Class::Window,
            ".as_this_window(…) (the window's own environment, given outside the stand-in programs)"
                .into(),
        ));
    }
    let sets = match name {
        "inheriting_env" | "inheriting_window_env" | "machine_env" | "env_clear" | "env_remove"
        | "envs" => true,
        "env" => args > 0,
        _ => false,
    };
    sets.then(|| {
        (
            Class::Env,
            format!(".{name}(…) (an environment given by a way that is not the door's)"),
        )
    })
}

/// What a string says, if it runs something the rule holds to a door.
fn literal_class(text: &str) -> Option<(Class, String)> {
    if text.to_ascii_lowercase().contains("vacuum into") {
        return Some((
            Class::Database,
            format!("{text:?} (a database written to a file outside the database door)"),
        ));
    }
    if names_a_secret_store_program(text) {
        return Some((
            Class::Account,
            format!("{text:?} (a program that reads a secret store of the account's)"),
        ));
    }
    ends_in_words(text).then(|| (Class::End, format!("{text:?}")))
}

impl Reader {
    /// Notes a place that does `what`, of `class`. Inside a closure handed to the door only the
    /// door's own act, an ending, is its to allow: a write or a start there is found.
    fn flag(&mut self, class: Class, what: String) {
        if (class == Class::End && (self.in_door > 0 || self.in_cfg_test > 0)) || self.skip.contains(&class) {
            return;
        }
        // The unit tests of a file read and name what they made.
        if class == Class::Owner && (self.asks_account || self.in_test > 0) {
            return;
        }
        // The unit tests of a held file read the folders they made.
        if class == Class::Read && (!self.holds_reads || self.in_test > 0) {
            return;
        }
        self.found.push(Found {
            file: self.file.clone(),
            item: self.item.last().cloned().unwrap_or_else(|| "file".into()),
            class,
            what,
        });
    }

    /// The names called and the strings in `tokens`, a macro's.
    fn tokens(&mut self, tokens: TokenStream) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        for (at, tree) in trees.iter().enumerate() {
            match tree {
                TokenTree::Group(group) => self.tokens(group.stream()),
                TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    let arguments = match trees.get(at + 1) {
                        Some(TokenTree::Group(next))
                            if next.delimiter() == proc_macro2::Delimiter::Parenthesis =>
                        {
                            Some(
                                usize::from(!next.stream().is_empty())
                                    + next
                                        .stream()
                                        .into_iter()
                                        .filter(|tree| matches!(tree, TokenTree::Punct(p) if p.as_char() == ','))
                                        .count(),
                            )
                        }
                        _ => None,
                    };
                    let Some(arguments) = arguments else { continue };
                    // `fs :: write (`: the two names before the call, with their `::`.
                    let before = at
                        .checked_sub(3)
                        .filter(|from| {
                            matches!(&trees[*from + 1], TokenTree::Punct(first) if first.as_char() == ':')
                                && matches!(&trees[*from + 2], TokenTree::Punct(second) if second.as_char() == ':')
                        })
                        .and_then(|from| match &trees[from] {
                            TokenTree::Ident(before) => Some(before.to_string()),
                            _ => None,
                        });
                    let after_dot = at
                        .checked_sub(1)
                        .is_some_and(|from| matches!(&trees[from], TokenTree::Punct(dot) if dot.as_char() == '.'));
                    let mut found = None;
                    if REMOVALS.contains(&name.as_str()) {
                        found = Some((Class::Write, format!("{name}(…) in a macro")));
                    } else if CALLS.contains(&name.as_str()) {
                        found = Some((Class::End, format!("{name}(…) in a macro")));
                    } else if after_dot {
                        found = method_call(&name, arguments)
                            .map(|(class, _)| (class, format!(".{name}(…) in a macro")));
                    }
                    if let (None, Some(before)) = (&found, before) {
                        let segments = [before.clone(), name.clone()];
                        if writes_raw(&segments) {
                            found = Some((Class::Write, format!("{before}::{name}(…) in a macro")));
                        } else if let Some((class, _)) = path_call(&segments, self.xtask) {
                            found = Some((class, format!("{before}::{name}(…) in a macro")));
                        }
                    }
                    if let Some((class, what)) = found {
                        self.flag(class, what);
                    }
                }
                TokenTree::Literal(literal) => {
                    if let Ok(text) = syn::parse_str::<LitStr>(&literal.to_string()) {
                        if let Some((class, _)) = literal_class(&text.value()) {
                            self.flag(class, format!("{:?} in a macro", text.value()));
                        }
                    }
                }
                TokenTree::Punct(_) => {}
            }
        }
    }
}

/// Whether `attrs` hold a `#[cfg(…)]` only a test build has: `test`, or `all(test, …)`.
fn for_tests_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let Meta::List(list) = &attr.meta else {
            return false;
        };
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let said = list.tokens.to_string().replace(' ', "");
        said == "test"
            || (said.starts_with("all(") && said[4..].split([',', ')']).any(|part| part == "test"))
    })
}

/// Whether `attrs` hold `#[test]`.
fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("test"))
}

impl<'ast> Visit<'ast> for Reader {
    fn visit_attribute(&mut self, _attribute: &'ast Attribute) {}

    fn visit_expr_binary(&mut self, binary: &'ast ExprBinary) {
        if matches!(binary.op, BinOp::Eq(_) | BinOp::Ne(_)) {
            self.reading += 1;
            visit::visit_expr_binary(self, binary);
            self.reading -= 1;
        } else {
            visit::visit_expr_binary(self, binary);
        }
    }

    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        let test = for_tests_only(&item.attrs);
        self.in_test += usize::from(test);
        self.in_cfg_test += usize::from(test);
        visit::visit_item_mod(self, item);
        self.in_test -= usize::from(test);
        self.in_cfg_test -= usize::from(test);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let cfg = for_tests_only(&item.attrs);
        let test = cfg || is_test(&item.attrs);
        self.in_test += usize::from(test);
        self.in_cfg_test += usize::from(cfg);
        self.item.push(item.sig.ident.to_string());
        visit::visit_item_fn(self, item);
        self.item.pop();
        self.in_test -= usize::from(test);
        self.in_cfg_test -= usize::from(cfg);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let cfg = for_tests_only(&item.attrs);
        let test = cfg || is_test(&item.attrs);
        self.in_test += usize::from(test);
        self.in_cfg_test += usize::from(cfg);
        self.item.push(item.sig.ident.to_string());
        visit::visit_impl_item_fn(self, item);
        self.item.pop();
        self.in_test -= usize::from(test);
        self.in_cfg_test -= usize::from(cfg);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if self
            .read_opens
            .contains(&(std::ptr::from_ref(call) as usize))
        {
            for arg in &call.args {
                self.visit_expr(arg);
            }
            return;
        }
        if let Expr::Path(path) = &*call.func {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            let last = segments.last().map(String::as_str).unwrap_or_default();
            let before = segments.iter().rev().nth(1).map(String::as_str);
            let mut named = path_call(&segments, self.xtask);
            // A program that writes, started by name: `Run::new("cp")`, `Command::new("mv")`.
            if last == "new" && matches!(before, Some("Run" | "Command")) {
                if let Some(Expr::Lit(literal)) = call.args.first() {
                    if let syn::Lit::Str(text) = &literal.lit {
                        let value = text.value();
                        let program = value.rsplit(['/', '\\']).next().unwrap_or(&value);
                        let program = program.strip_suffix(".exe").unwrap_or(program);
                        if WRITING_PROGRAMS.contains(&program) && named.is_none() {
                            named = Some((
                                Class::Write,
                                format!("{value:?} started as a program that writes"),
                            ));
                        }
                    }
                }
            }
            if let Some((class, what)) = named {
                self.flag(class, what);
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        let on_self = matches!(&*call.receiver, Expr::Path(path) if path.path.is_ident("self"));
        if method == "kill" && call.args.is_empty() && self.holds_children && !on_self {
            self.flag(
                Class::End,
                ".kill() in a file that holds a child program".into(),
            );
        }
        if method == "open" {
            if let Some((root, names)) = open_chain(call) {
                if !names
                    .iter()
                    .any(|name| OPENS_FOR_WRITING.contains(&name.as_str()))
                {
                    self.read_opens.insert(std::ptr::from_ref(root) as usize);
                }
            }
        }
        if let Some((class, what)) = method_call(&method, call.args.len()) {
            self.flag(class, what);
        }
        if call.args.is_empty()
            && matches!(method.as_str(), "is_file" | "is_dir" | "is_symlink")
            && !asks_a_file_type(&call.receiver)
        {
            self.flag(
                Class::Read,
                format!(".{method}() (a read of the file system outside the door's readers and a source)"),
            );
        }
        // The descent's hold of a process, which it ends: the descent's own.
        if method == "hold" && call.args.len() == 1 {
            self.flag(Class::End, ".hold(…) of a process to end".into());
        }
        if DOOR_CLOSURES.contains(&method.as_str()) {
            self.visit_expr(&call.receiver);
            self.in_door += 1;
            for arg in &call.args {
                self.visit_expr(arg);
            }
            self.in_door -= 1;
            return;
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let named: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if let Some(name) = named.iter().find(|name| PTY_STARTS.contains(&name.as_str())) {
            self.flag(
                Class::Start,
                format!("{name} (a program started on a pty of its own)"),
            );
        }
        if self.xtask && named.iter().any(|name| name == "Invocation") {
            self.flag(
                Class::Start,
                "Invocation (a program xtask's live run names itself)".into(),
            );
        }
        if named.first().is_some_and(|first| first == "tempfile") {
            self.flag(
                Class::Write,
                format!("{} (taken away when dropped)", named.join("::")),
            );
        }
        if writes_raw(&named) {
            self.flag(
                Class::Write,
                format!(
                    "{} (a write that does not ask the door's writers)",
                    named.join("::")
                ),
            );
        }
        if named.len() >= 2
            && named[named.len() - 1] == "this"
            && named[named.len() - 2] == "Account"
        {
            self.flag(
                Class::Owner,
                "Account::this() (the owner's files asked for outside the files named for it)"
                    .into(),
            );
        }
        if named
            .iter()
            .any(|name| SECRET_STORE_CALLS.contains(&name.as_str()))
            || named
                .first()
                .is_some_and(|first| SECRET_STORE_CRATES.contains(&first.as_str()))
        {
            self.flag(
                Class::Account,
                format!(
                    "{} (a secret store of the account's, which no path names)",
                    named.join("::")
                ),
            );
        }
        if reads_raw(&named) {
            self.flag(
                Class::Read,
                format!(
                    "{} (a read of the file system outside the door's readers and a source)",
                    named.join("::")
                ),
            );
        }
        visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        if matches!(&item.tree, UseTree::Path(path) if path.ident == "tempfile") {
            self.flag(
                Class::Write,
                "use tempfile::… (taken away when dropped)".into(),
            );
        }
        for imported in imports(&item.tree, Vec::new()) {
            if let Some(name) = imported
                .iter()
                .find(|name| PTY_STARTS.contains(&name.as_str()))
            {
                self.flag(
                    Class::Start,
                    format!(
                        "use {} ({name}: a program started on a pty of its own)",
                        imported.join("::")
                    ),
                );
            }
            if self.xtask && imported.iter().any(|name| name == "Invocation") {
                self.flag(
                    Class::Start,
                    format!(
                        "use {} (a program xtask's live run names itself)",
                        imported.join("::")
                    ),
                );
            }
            if imported.len() >= 2
                && imported[imported.len() - 1] == "Account"
                && imported[imported.len() - 2] == "source"
            {
                self.flag(
                    Class::Owner,
                    format!(
                        "use {} (the owner's files asked for outside the files named for it)",
                        imported.join("::")
                    ),
                );
            }
            if imported
                .iter()
                .any(|name| SECRET_STORE_CALLS.contains(&name.as_str()))
                || imported
                    .first()
                    .is_some_and(|first| SECRET_STORE_CRATES.contains(&first.as_str()))
            {
                self.flag(
                    Class::Account,
                    format!(
                        "use {} (a secret store of the account's, which no path names)",
                        imported.join("::")
                    ),
                );
            }
            if reads_raw(&imported) {
                self.flag(
                    Class::Read,
                    format!(
                        "use {} (a read of the file system outside the door's readers and a source)",
                        imported.join("::")
                    ),
                );
            }
            if writes_raw(&imported)
                || (imported.last().is_some_and(|last| last == "*")
                    && imported
                        .iter()
                        .rev()
                        .nth(1)
                        .is_some_and(|before| before == "fs"))
            {
                self.flag(
                    Class::Write,
                    format!(
                        "use {} (a write that does not ask the door's writers)",
                        imported.join("::")
                    ),
                );
            }
        }
    }

    fn visit_lit_str(&mut self, literal: &'ast LitStr) {
        if self.reading == 0 {
            if let Some((class, what)) = literal_class(&literal.value()) {
                self.flag(class, what);
            }
        }
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        self.tokens(mac.tokens.clone());
    }
}

/// Every path a `use` brings in, as its names (`use std::fs::{write, File}` is `std::fs::write`
/// and `std::fs::File`; a glob ends in `*`).
fn imports(tree: &UseTree, mut above: Vec<String>) -> Vec<Vec<String>> {
    match tree {
        UseTree::Path(path) => {
            above.push(path.ident.to_string());
            imports(&path.tree, above)
        }
        UseTree::Name(name) => {
            above.push(name.ident.to_string());
            vec![above]
        }
        UseTree::Rename(rename) => {
            above.push(rename.ident.to_string());
            vec![above]
        }
        UseTree::Glob(_) => {
            above.push("*".to_owned());
            vec![above]
        }
        UseTree::Group(group) => group
            .items
            .iter()
            .flat_map(|item| imports(item, above.clone()))
            .collect(),
    }
}

/// Whether `text` names a type `Child` (std's, or a terminal's child), as a word.
fn holds_children(text: &str) -> bool {
    text.match_indices("Child").any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + "Child".len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

/// The file a `#[path = "…"]` attribute among `attrs` names.
fn path_attribute(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("path") {
            return None;
        }
        match &attr.meta {
            Meta::NameValue(value) => match &value.value {
                Expr::Lit(literal) => match &literal.lit {
                    syn::Lit::Str(text) => Some(text.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    })
}

/// The module files that `file`'s `#[cfg(test)] mod name;` items gate: `name.rs` and the
/// folder `name`, beside `file` where it is a `mod.rs` or `lib.rs`, and below its own folder
/// otherwise.
fn gated_by(file: &Path, text: &str) -> Vec<PathBuf> {
    let Ok(parsed) = syn::parse_file(text) else {
        return Vec::new();
    };
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let folder = file.parent().unwrap_or(Path::new(""));
    let base = if matches!(stem, "mod" | "lib" | "main") {
        folder.to_path_buf()
    } else {
        folder.join(stem)
    };
    parsed
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) if module.content.is_none() && for_tests_only(&module.attrs) => {
                Some((module.ident.to_string(), path_attribute(&module.attrs)))
            }
            _ => None,
        })
        .flat_map(|(name, path)| match path {
            // `#[path = "x.rs"]`: beside the file that declares it.
            Some(path) => vec![folder.join(path)],
            None => vec![base.join(format!("{name}.rs")), base.join(name)],
        })
        .collect()
}

/// What the rule finds in `text`, the file `name`, held to `skip` and, for a file of xtask's,
/// to its own runner.
fn read_as(
    name: &str,
    text: &str,
    xtask: bool,
    skip: &'static [Class],
    test_file: bool,
) -> Vec<Found> {
    let file = syn::parse_file(text).unwrap_or_else(|error| panic!("{name}: {error}"));
    let mut reader = Reader {
        file: name.to_owned(),
        item: Vec::new(),
        holds_children: holds_children(text),
        xtask,
        skip,
        read_opens: std::collections::HashSet::new(),
        holds_reads: READS_HELD.iter().any(|held| name.starts_with(held)),
        asks_account: name.starts_with("src/door")
            || OWNER_FILES.iter().any(|(file, _)| name == *file),
        in_test: usize::from(test_file),
        in_cfg_test: usize::from(test_file),
        in_door: 0,
        reading: 0,
        found: Vec::new(),
    };
    reader.visit_file(&file);
    reader.found
}

/// What the rule finds in `text`, the file `name`, as a file the kit's own is held to.
fn read(name: &str, text: &str) -> Vec<Found> {
    read_as(name, text, false, &[], false)
}

/// The Rust files at `path`: the file, or every one in the folder and the folders in it.
fn rust_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return if path.extension().is_some_and(|kind| kind == "rs") {
            vec![path.to_owned()]
        } else {
            Vec::new()
        };
    }
    let mut found: Vec<PathBuf> = fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .flat_map(|entry| rust_files(&entry.path()))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// The classes `name` (from the crate's folder) is not held to ([`LEAVES_OUT`]).
fn left_out(name: &str) -> &'static [Class] {
    LEAVES_OUT
        .iter()
        .find(|(prefix, _, _)| name.starts_with(prefix))
        .map_or(&[], |(_, classes, _)| classes)
}

/// Every file the rule reads, named from the crate's folder (xtask's live run from the workspace's
/// `tools`, as `tools/xtask/src/live…`): all of cf-e2e's `src` and `tests`, code only a test
/// builds included, and `cargo xtask live` with its folder.
fn all_files() -> Vec<(String, PathBuf, bool)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<(String, PathBuf, bool)> = ["src", "tests"]
        .iter()
        .flat_map(|folder| rust_files(&root.join(folder)))
        .map(|file| {
            let name = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (name, file, false)
        })
        .collect();
    let xtask = root.join("../../tools/xtask/src");
    for file in rust_files(&xtask.join("live.rs"))
        .into_iter()
        .chain(rust_files(&xtask.join("live")))
    {
        let name = format!(
            "tools/xtask/src/{}",
            file.strip_prefix(&xtask)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );
        files.push((name, file, true));
    }
    files
}

/// What the rule finds in the kit, before the waivers.
fn found_in_the_kit() -> Vec<Found> {
    let files = all_files();
    // The module files a `#[cfg(test)] mod name;` gates are code only a test builds.
    let gated: Vec<PathBuf> = files
        .iter()
        .flat_map(|(_, file, _)| gated_by(file, &fs::read_to_string(file).unwrap_or_default()))
        .collect();
    files
        .iter()
        .flat_map(|(name, file, xtask)| {
            read_as(
                name,
                &fs::read_to_string(file).unwrap(),
                *xtask,
                left_out(name),
                gated.iter().any(|gate| file.starts_with(gate)),
            )
        })
        .collect()
}

/// Whether `table` waives `found`: its file, its function and its class, all three.
fn waived_in(table: &[(&str, &str, Class, &str)], found: &Found) -> bool {
    table.iter().any(|(file, item, class, _)| {
        *file == found.file && *item == found.item && *class == found.class
    })
}

fn waived(found: &Found) -> bool {
    waived_in(WAIVERS, found)
}

#[test]
fn the_live_kit_starts_ends_removes_and_writes_only_through_its_doors() {
    let named: Vec<String> = all_files().into_iter().map(|(name, _, _)| name).collect();
    for suite in [
        "tests/live_finish/leaving.rs",
        "tests/live_stops/stuck.rs",
        "tests/live_restart_cut.rs",
        "tests/live_parity/claude.rs",
        "tests/rig/layout.rs",
        "tests/regressions.rs",
        "tests/smoke.rs",
        "tests/cli.rs",
        "tests/daemon.rs",
        "src/rig/sweep.rs",
        "src/live/renew.rs",
        "src/live/login.rs",
        "src/live/isolation/tests.rs",
        "src/process/spawned.rs",
        "src/daemon.rs",
        "src/matrix/run.rs",
        "src/matrix/evidence.rs",
        "src/matrix/scenario/window_life.rs",
        "src/bin/fake_agent.rs",
        "src/bin/fake_agent/tool.rs",
        "src/door/cargo.rs",
        "tests/live_muse/tui.rs",
        "tools/xtask/src/live.rs",
        "tools/xtask/src/live/tests.rs",
    ] {
        assert!(named.iter().any(|name| name == suite), "{suite} is read");
    }
    let found: Vec<String> = found_in_the_kit()
        .into_iter()
        .filter(|found| !waived(found))
        .map(|found| format!("[{}] {found}", found.class.name()))
        .collect();
    assert!(
        found.is_empty(),
        "the live kit starts, ends, removes or writes other than through its doors ({}): {found:#?}",
        found.len()
    );
}

#[test]
fn a_waiver_waives_one_operation_in_one_function_of_one_file_and_no_other_act() {
    let table = [("a.rs", "f", Class::Start, "why")];
    let found = |file: &str, item: &str, class| Found {
        file: file.into(),
        item: item.into(),
        class,
        what: String::new(),
    };
    assert!(waived_in(&table, &found("a.rs", "f", Class::Start)));
    // Another operation of the same function, another function, another file: found.
    assert!(!waived_in(&table, &found("a.rs", "f", Class::Write)));
    assert!(!waived_in(&table, &found("a.rs", "g", Class::Start)));
    assert!(!waived_in(&table, &found("b.rs", "f", Class::Start)));
}

#[test]
fn each_waiver_is_one_operation_in_one_function_of_one_file_and_is_used() {
    let found = found_in_the_kit();
    for (file, item, class, why) in WAIVERS {
        assert!(!why.is_empty(), "{file} ({item}): a waiver says why");
        assert!(
            found
                .iter()
                .any(|found| found.file == *file && found.item == *item && found.class == *class),
            "{file} ({item}): the {} waived there is no longer found: the waiver goes",
            class.name()
        );
    }
    let mut seen = std::collections::HashSet::new();
    for (file, item, class, _) in WAIVERS {
        assert!(
            seen.insert((file, item, class.name())),
            "{file} ({item}) is waived twice for the {}",
            class.name()
        );
    }
}

#[test]
fn the_rule_finds_each_way_to_end_or_remove_and_not_what_only_reads_or_asks_the_door() {
    let found = |text: &str| read("x.rs", text);
    // What it finds.
    for text in [
        "fn f() { let _ = std::fs::remove_dir_all(&p); }",
        "fn f() { fs::remove_file(p).ok(); }",
        "fn f() { assert!(std::fs::remove_dir(&p).is_ok()); }",
        "fn f() { let _ = signal(pid, Signal::Kill); }",
        "fn f() { crate::process::stall(pid); }",
        "fn f() { end_chosen(&chosen); }",
        "fn f() { unsafe { libc::kill(pid, 9) }; }",
        "fn f() { let _ = nix::sys::signal::kill(pid, Signal::SIGKILL); }",
        "fn f() { let _ = killpg(Pid::from_raw(leader), Signal::SIGKILL); }",
        "fn f() { let _ = nix::sys::signal::killpg(group, Signal::SIGTERM); }",
        "fn f() { let held = ThisMachine.hold(pid)?; }",
        r#"fn f() { Run::new("taskkill").args(["/F"]); }"#,
        r#"fn f() { Run::new("/bin/kill").arg(pid); }"#,
        r#"fn f() { rig.host("pane.kill", pane); }"#,
        r#"fn f() { let s = format!("pkill -f {mark}"); }"#,
        r#"fn f() { let s = "kill -9 $pid"; }"#,
        r#"fn f() { let s = "kill -KILL 4711"; }"#,
        "use std::process::Child;\nfn f(child: &mut Child) { let _ = child.kill(); }",
        "struct S { child: Box<dyn portable_pty::Child> }\nimpl S { fn f(&mut self) { let _ = self.child.kill(); } }",
        r#"fn f() { assert!(std::fs::remove_file(p).is_ok() == true); }"#,
        "fn f() { let folder = tempfile::tempdir().unwrap(); }",
        "struct S { folder: tempfile::TempDir }",
        "use tempfile::TempDir;",
        // Writes that do not ask the door's writers.
        "fn f() { std::fs::write(p, b\"x\").unwrap(); }",
        "fn f() { fs::copy(a, b).unwrap(); }",
        "fn f() { let _ = fs::rename(a, b); }",
        "fn f() { fs::create_dir_all(p).unwrap(); }",
        "fn f() { std::fs::create_dir(p).unwrap(); }",
        "fn f() { let file = File::create(p).unwrap(); }",
        "fn f() { let file = std::fs::File::create_new(p).unwrap(); }",
        "fn f() { let file = OpenOptions::new().append(true).open(p); }",
        "fn f() { let file = OpenOptions::new().read(true).write(true).open(p); }",
        "fn f() { let mut open = OpenOptions::new(); open.read(true); }",
        "fn f() { let file = std::fs::OpenOptions::new().write(true).open(p); }",
        "fn f() { let file = File::options().write(true).open(p); }",
        "fn f() { file.set_len(0).unwrap(); }",
        "fn f() { file.set_permissions(mode).unwrap(); }",
        "fn f() { std::os::unix::fs::symlink(a, b).unwrap(); }",
        "fn f() { let write = fs::write; }",
        "use std::fs::{write, File};",
        "use std::fs::*;",
        "fn f() { let _ = Run::new(\"cp\").args([a, b]); }",
        "fn f() { let _ = std::process::Command::new(\"/bin/mv\"); }",
        "fn f() { assert!(fs::write(p, \"x\").is_ok()); }",
        // A program on a pty of the kit's own.
        "use cf_panes::pty::PaneTable;",
        "fn f() { let table = PaneTable::new(); }",
        "fn f() { let command = CommandBuilder::new(program); }",
        "fn f() { let child = pair.slave.spawn_command(command); }",
        "use portable_pty::native_pty_system;",
        "fn f() { paths.iter().for_each(|p| { let _ = fs::write(p, \"\"); }); }",
        // Code only a test builds is held to the doors too.
        "#[cfg(test)]\nmod tests { fn f() { std::fs::remove_dir_all(&p).unwrap(); } }",
        "#[cfg(all(test, unix))]\nmod tests { fn f() { std::fs::remove_file(&p).unwrap(); } }",
        "#[cfg(test)]\nmod tests { fn f() { let folder = tempfile::tempdir().unwrap(); } }",
        "#[cfg(test)]\nmod tests { fn f() { fs::write(p, \"x\").unwrap(); } }",
        "#[cfg(test)]\nmod tests { fn f() { let table = PaneTable::new(); } }",
        "#[test]\nfn a_case() { std::fs::create_dir_all(&folder).unwrap(); }",
        // A program started outside the spawn door, whatever its environment is made of.
        r#"fn f() { let _ = Command::new("git").arg("status"); }"#,
        r#"fn f() { let _ = std::process::Command::new(program); }"#,
        r#"fn f() { let _ = tokio::process::Command::new("sh"); }"#,
        "fn f() { let _ = command.exec(); }",
        "fn f() { let pid = unsafe { libc::fork() }; }",
        "fn f() { unsafe { posix_spawn(&mut pid, p, a, b, c, d) }; }",
        "fn f() { unsafe { CreateProcessW(a, b, c, d, e, f, g, h, i, j) }; }",
        // Its environment, set in a way that is not the door's: at the call it is written.
        r#"fn f() { let _ = Run::new("git").inheriting_env().run(); }"#,
        r#"fn f() { let _ = Run::new("git").inheriting_window_env().run(); }"#,
        r#"fn f() { let _ = Run::new("git").machine_env().run(); }"#,
        r#"fn f() { command.env("HOME", home); }"#,
        "fn f() { command.envs(vars); }",
        "fn f() { command.env_clear(); }",
        r#"fn f() { command.env_remove("PATH"); }"#,
        // A database opened where it can write, and one written to a file of its own.
        "fn f() { let db = rusqlite::Connection::open(p); }",
        "fn f() { let db = Connection::open_with_flags(p, flags); }",
        r#"fn f() { db.execute("VACUUM INTO ?1", [to]); }"#,
        r#"fn f() { db.execute_batch("vacuum into '/x/y.db'"); }"#,
        // A stand-in's window given to a child outside the stand-in programs.
        "fn f() { let _ = Run::new(\"x\").as_this_window(Window::this()); }",
        // An operation of the door's, made outside the files that make them.
        r#"fn f() { let _ = Run::operation("cargo", program); }"#,
        // Inside a closure handed to the door, only the door's own act is its to allow.
        r#"fn f() { door.close_own_terminal("t", || { fs::write(p, "x").unwrap(); }); }"#,
        r#"fn f() { door.quit_own_terminal("t", || { let _ = Command::new("x"); }); }"#,
        r#"fn f() { door.close_pane(&pane, |op, body| { let _ = Connection::open(body); }); }"#,
    ] {
        assert_eq!(found(text).len(), 1, "{text}");
    }
    // What it does not.
    for text in [
        "fn f() { let _ = Door::of_run().remove_made(&p); }",
        "use std::process::Child;\nfn f(child: &mut Child) { door.quit_own_terminal(\"t\", || { let _ = child.kill(); }); }",
        "fn f() { window.kill(); }",
        "fn f() { let code = ExitStatusExt::signal(&status); }",
        r#"fn f() { Run::new("ps").args(["-p", &pid]); }"#,
        r#"fn f() { Run::new("killer-app"); }"#,
        "use std::process::Child;\nfn f(child: &mut Child) { door.close_own_terminal(\"t\", || { let _ = child.kill(); }); }",
        r#"fn f() { door.close_pane(&pane, |op, body| rig.host(op, body)); }"#,
        r#"fn f() { if frame["op"] == "pane.kill" { kills.push(1); } }"#,
        "/// Every `pane.kill` the daemon asked.\nfn f() {}",
        "use std::process::ChildStdout;\nfn f() { window.kill(); }",
        "use std::process::Child;\nimpl S { fn drop(&mut self) { self.kill(); } }",
        "// std::fs::remove_dir_all(&p)\nfn f() {}",
        "fn f() { let folder = crate::door::made(\"x-\").unwrap(); }",
        r#"fn f() { let s = "kill -0 \"$PPID\" 2>/dev/null || exit 0"; }"#,
        // What reads, and what asks the door's writers.
        "fn f() { let _ = crate::door::write(p, \"x\"); }",
        "fn f() { let _ = std::fs::read_to_string(p); let _ = File::open(p); let _ = fs::metadata(p); }",
        "fn f() { let _ = fs::read_dir(p); let _ = fs::canonicalize(p); }",
        "use std::fs::{self, File, OpenOptions};",
        "fn f() { let _ = Run::new(\"ls\"); let _ = Copier::new(\"copier\"); }",
        "fn f() { let file = OpenOptions::new().read(true).custom_flags(1).open(p); }",
        "fn f() { let file = File::options().read(true).open(p); }",
        "fn f() { writer.write(b\"x\"); }",
        "fn f() { let _ = files::read(p); let _ = files::read_string(p); }",
        // The plain helpers of `files` are the door's writers, and name no way of their own.
        "fn f() { files::write(p, \"x\").unwrap(); crate::files::copy(a, b).unwrap(); }",
        "use portable_pty::PtySize;\nfn f() { let size = PtySize { rows: 40, cols: 140, pixel_width: 0, pixel_height: 0 }; }",
        "// fs::write(p, \"x\")\nfn f() {}",
        // What a getter of the same name does not set, and what only names a program.
        "fn f() { let env = muse.env(); let again = stand_in.env(); }",
        r#"fn f() { let _ = Run::new("ls").var("A", "b").run(); }"#,
        "fn f(command: &mut Command) { command.arg(\"x\"); }",
        "use std::process::Command;",
        "fn f() { let _ = Connection::open_in_memory(); }",
        "fn f() { let _ = crate::door::database::read(p); }",
        r#"fn f() { let _ = std::env::var("PATH"); let s = "vacuum the room"; }"#,
        "fn f() { let _ = Run::harness(\"claude\", program).homes(&homes).run(); }",
    ] {
        assert_eq!(found(text), Vec::new(), "{text}");
    }
}

#[test]
fn xtasks_live_run_names_no_program_of_its_own_and_the_door_holds_what_it_is_made_of() {
    // xtask's live files: `Invocation` and the generic runs are found; cargo's entry is not.
    let xtask = |text: &str| read_as("tools/xtask/src/live.rs", text, true, &[], false);
    for text in [
        r#"fn f() { let i = Invocation::new("git", root); }"#,
        "use crate::process::{self, Invocation};",
        "fn f() { let _ = process::run(&i, &env); }",
        "fn f() { let _ = process::capture(&i, &env); }",
        "fn f() { let _ = process::spawn(&i, &env, s, false); }",
        "fn f(i: Invocation) {}",
    ] {
        assert_eq!(xtask(text).len(), 1, "{text}");
    }
    for text in [
        "fn f() { let _ = process::cargo(root).arg(\"test\"); }",
        "fn f() { let _ = process::run_cargo(&cargo, &env); }",
        "fn f(cargo: Cargo) {}",
    ] {
        assert_eq!(xtask(text), Vec::new(), "{text}");
    }
    // The same words in cf-e2e's own files are no xtask's runs.
    assert_eq!(read("x.rs", "fn f() { let _ = process::run(&i, &env); }"), Vec::new());
    // A class a file is left out of is not found there; the others are.
    let door = |text: &str| read_as("src/door/x.rs", text, false, left_out("src/door/x.rs"), false);
    assert_eq!(door("fn f() { Command::new(\"x\"); fs::write(p, \"\"); }"), Vec::new());
    assert_eq!(
        door(r#"fn f() { Run::operation("x", p); }"#).len(),
        1,
        "the door's own files are read for the operations they make: each is a waiver"
    );
    assert!(left_out("src/live/members/descent/tests.rs").contains(&Class::End));
    assert!(!left_out("src/live/members/descent/tests.rs").contains(&Class::Write));
    // The offline suites are held for every class but the ending of processes they started.
    let bin = |text: &str| read_as("src/bin/fake_agent.rs", text, false, left_out("src/bin/fake_agent.rs"), false);
    assert_eq!(bin("fn f() { let _ = Run::new(\"x\").as_this_window(Window::this()); }"), Vec::new());
    assert_eq!(left_out("tests/rig/layout.rs"), [Class::End]);
    assert!(left_out("tests/live_stops/stuck.rs").is_empty());
}


#[test]
fn the_rule_finds_a_read_outside_the_readers_and_an_account_asked_for_outside_its_files() {
    let held = |text: &str| read_as("src/live/isolation.rs", text, false, &[], false);
    let elsewhere = |text: &str| read_as("src/live/members.rs", text, false, &[], false);
    for text in [
        "fn f() { let _ = std::fs::read(p); }",
        "fn f() { let _ = fs::read_to_string(p); }",
        "fn f() { for e in fs::read_dir(p).unwrap() {} }",
        "fn f() { let _ = fs::metadata(p); }",
        "fn f() { let _ = fs::symlink_metadata(p); }",
        "fn f() { let _ = fs::canonicalize(p); }",
        "fn f() { let _ = fs::read_link(p); }",
        "fn f() { let _ = File::open(p); }",
        "fn f() { let _ = std::fs::File::open(p); }",
        "use std::fs::{read, File};",
        "fn f() { if p.exists() {} }",
        "fn f() { if p.is_file() {} }",
        "fn f() { if p.is_dir() {} }",
        "fn f() { let _ = p.read_dir(); }",
        "fn f() { let _ = p.canonicalize(); }",
        "fn f() { let _ = p.read_link(); }",
        "fn f() { let _ = p.symlink_metadata(); }",
        "fn f() { let _ = p.metadata(); }",
        "fn f() { if p.is_symlink() {} }",
    ] {
        let found = held(text);
        assert!(
            found.iter().any(|found| found.class == Class::Read),
            "{text} is not found as a read: {found:?}"
        );
        assert!(
            !elsewhere(text).iter().any(|found| found.class == Class::Read),
            "{text} is found in a file that is no held file's"
        );
    }
    // What it does not find: the door's readers and a source, a test of a held file, a path that
    // is built or compared, and a method of the same name with arguments.
    for text in [
        "fn f() { let _ = crate::door::reads::read(p); }",
        "fn f() { let _ = door::reads::is_dir(p); }",
        "fn f() { let _ = source.read(); }",
        "fn f() { let _ = source.present(); }",
        "fn f() { let _ = p.join(\"x\").display(); }",
        "#[cfg(test)]\nmod tests { fn f() { let _ = std::fs::read(p); } }",
        "fn f() { let _ = map.metadata(\"key\"); }",
        "fn f() { if meta.is_dir() && kind.is_file() && m.file_type().is_symlink() {} }",
        "fn f() { let _ = source.meta(); let _ = source.lstat(); let _ = source.real(); }",
    ] {
        let found: Vec<_> = held(text)
            .into_iter()
            .filter(|found| found.class == Class::Read)
            .collect();
        assert!(found.is_empty(), "{text} is found: {found:?}");
    }
    // The account is asked for only in the files named for it.
    for (name, asks) in [
        ("src/live/machine.rs", true),
        ("src/live/isolation.rs", true),
        ("src/door/source.rs", true),
        ("src/live/members.rs", false),
        ("tests/live_matrix.rs", false),
    ] {
        let found = read_as(
            name,
            "fn f() { let _ = Account::this().home(Purpose::Guard); }",
            false,
            &[],
            false,
        );
        assert_eq!(
            found.iter().any(|found| found.class == Class::Owner),
            !asks,
            "{name}: {found:?}"
        );
        let imported = read_as(
            name,
            "use crate::door::source::Account;",
            false,
            &[],
            false,
        );
        assert_eq!(
            imported.iter().any(|found| found.class == Class::Owner),
            !asks,
            "{name}: {imported:?}"
        );
    }
    // A case's own `Account` (the members' tally of a case) is no account of the owner's.
    let tally = read_as(
        "src/live/members.rs",
        "use crate::live::members::Account;\nfn f() { let _ = Account::new(); }",
        false,
        &[],
        false,
    );
    assert!(tally.iter().all(|found| found.class != Class::Owner), "{tally:?}");
}

#[test]
fn the_rule_finds_a_secret_store_no_path_names_and_each_listed_store_has_a_way_it_finds() {
    let found = |text: &str| read("x.rs", text);
    // What stores::SECRET_STORES lists, and a way each is reached that the rule finds.
    let ways: &[(&str, &[&str])] = &[
        (
            "Keychain",
            &[
                "fn f() { unsafe { SecItemCopyMatching(q, r) }; }",
                "use security_framework::passwords::get_generic_password;",
            ],
        ),
        (
            "Windows' Credential Manager",
            &[
                "fn f() { unsafe { CredReadW(t, 1, 0, &mut c) }; }",
                "fn f() { unsafe { windows_sys::Win32::Security::Credentials::CredEnumerateW(a, 0, &mut n, &mut c) }; }",
                r#"fn f() { let _ = Run::new("cmdkey").arg("/list"); }"#,
                r#"fn f() { let _ = Run::new("C:\\Windows\\System32\\vaultcmd.exe"); }"#,
                r#"fn f() { let _ = "powershell -Command cmdkey /list"; }"#,
            ],
        ),
        (
            "the Secret Service",
            &[
                "fn f() { let e = keyring::Entry::new(a, b); }",
                r#"fn f() { let _ = Run::new("secret-tool").arg("lookup"); }"#,
            ],
        ),
    ];
    for (name, whole) in crate::live::stores::SECRET_STORES
        .iter()
        .map(|(_, name, _)| (*name, ()))
        .map(|(name, ())| (name, ways.iter().find(|(held, _)| *held == name)))
    {
        let Some((_, texts)) = whole else {
            panic!("{name} is a secret store of the table, and no way to it is shown found")
        };
        for text in *texts {
            let flagged = found(text);
            assert!(
                flagged.iter().any(|found| found.class == Class::Account),
                "{name}: {text} is not found: {flagged:?}"
            );
        }
    }
    // What it does not find: the Keychain's own door is a file of the door, a string that only
    // says where a login is, and a name that has the program's letters in it.
    for text in [
        r#"fn f() { let _ = "the login is in Windows' Credential Manager"; }"#,
        r#"fn f() { let _ = Run::new("mycmdkeys"); }"#,
        "fn f() { let key = keyring_size; }",
    ] {
        assert!(
            found(text).iter().all(|found| found.class != Class::Account),
            "{text} is found"
        );
    }
    let door = read_as("src/door/keychain.rs", "fn f() { SecItemAdd(a, b); }", false, left_out("src/door/keychain.rs"), false);
    assert!(door.iter().all(|found| found.class != Class::Account), "{door:?}");
}
