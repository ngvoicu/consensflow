/**
 * Plants in the packaged smoke and in the candidate builder, both in Rust: the
 * smoke's parts (crates/cf-e2e/tests/smoke/ and the paste reader it runs), the
 * command that runs it (`cargo xtask smoke`, tools/xtask/src/smoke.rs) and the
 * candidate (`cargo xtask candidate`, tools/xtask/src/candidate.rs). What a
 * plant is held to is the tests of each, which need no app and no install: the
 * smoke's against stand-in apps, the candidate's against a system that is a
 * script on a temporary tree, never the real /Applications or a real home.
 *
 * The plants of the last block plant what only a real app shows, the steps of
 * the smoke's own case and its stand-in harness: they run the packaged smoke
 * itself, on the app a build of the checkout leaves (`npm --prefix app run build
 * -- --bundles app`), and are refused where there is none.
 */

const cargo = (...args) => ['cargo', 'test', '--offline', '--no-fail-fast', ...args]

/** The tests of the smoke's parts: the bundle, the machine, the app, the extension. */
const PARTS = cargo('-p', 'cf-e2e', '--test', 'smoke')
/** The reader of a large paste that the stand-in harness runs. */
const PASTE = cargo('-p', 'cf-e2e', '--bin', 'smoke-paste-reader')
/** The command that runs the smoke: its own tests, the packaged smoke's and not the updater's. */
const COMMAND = cargo('-p', 'xtask', '--lib', '--', 'smoke::', '--skip', 'updater_smoke')
/** The same command started as a process, against a stand-in for cargo. */
const COMMAND_RUN = cargo('-p', 'xtask', '--test', 'drivers')
/** The candidate's steps, on a machine of the test's own. */
const CANDIDATE = cargo('-p', 'xtask', '--lib', '--', 'candidate::')
/** The packaged smoke, on the app a build leaves. */
const SMOKE = ['cargo', 'xtask', 'smoke']
const BUILT_APP = 'app/src-tauri/target/release/bundle/macos/ConsensFlow.app'

const SMOKE_DIR = 'crates/cf-e2e/tests/smoke'
const BUNDLE_RS = `${SMOKE_DIR}/bundle.rs`
const MACHINE_RS = `${SMOKE_DIR}/machine.rs`
const APP_RS = `${SMOKE_DIR}/app.rs`
const EXTENSION_RS = `${SMOKE_DIR}/extension.rs`
const TEST_RS = 'crates/cf-e2e/tests/smoke.rs'
const HARNESS_SH = 'crates/cf-e2e/fixtures/smoke-harness.sh'
const PASTE_RS = 'crates/cf-e2e/src/bin/smoke_paste_reader.rs'
const COMMAND_RS = 'tools/xtask/src/smoke.rs'
const SUITES_RS = 'tools/xtask/src/suites.rs'
const CANDIDATE_RS = 'tools/xtask/src/candidate.rs'
const CANARY_RS = 'tools/xtask/src/candidate/canary.rs'
const SYSTEM_RS = 'tools/xtask/src/candidate/system.rs'

const plant = (name, file, from, to, runs, meant, more = {}) => ({
  name,
  edits: [[file, from, to]],
  runs,
  meant,
  ...more,
})

/** A plant in what the smoke is made of. */
const part = (what, file, from, to, meant) =>
  plant(`packaged smoke: ${what}`, file, from, to, [PARTS], meant)

/** A plant in the command that runs the smoke. */
const command = (what, file, from, to, meant, runs = [COMMAND, COMMAND_RUN]) =>
  plant(`packaged smoke: ${what}`, file, from, to, runs, meant)

/** A plant in the candidate. */
const candidate = (what, file, from, to, meant) =>
  plant(`candidate: ${what}`, file, from, to, [CANDIDATE], meant)

/** A plant that only a real app shows: it runs the smoke on the built one. */
const real = (what, file, from, to) =>
  plant(`packaged smoke, on the app: ${what}`, file, from, to, [SMOKE], 'the_built_app', {
    requires: [BUILT_APP],
  })

export const PLANTS = [
  // What the smoke holds the bundle to
  part(
    'a bundle that holds cf.mjs is taken for one that holds no Node',
    BUNDLE_RS,
    'const NODE_FILES: [&str; 3] = ["node", "node.exe", "cf.mjs"];',
    'const NODE_FILES: [&str; 2] = ["node", "node.exe"];',
    'node_is_found_wherever_it_is_in_the_bundle_and_named_with_where',
  ),
  part(
    'a bundle with no cf for a window to run is taken for one that can be run',
    BUNDLE_RS,
    'for (what, path) in [("executable", &binary), ("cf for a window to run", &cf)] {',
    'for (what, path) in [("executable", &binary)] {',
    'a_bundle_that_is_not_there_or_cannot_be_run_says_what_is_missing_and_what_builds_it',
  ),
  part(
    'an app named by an empty variable is taken for the one that is named',
    BUNDLE_RS,
    'match named.filter(|name| !name.is_empty()) {',
    'match named {',
    'the_bundle_is_the_one_named_else_the_one_a_build_leaves_and_a_relative_name_is_from_the_root',
  ),
  // The machine the app lives in
  part(
    'the machine has a shell for the app to ask for its PATH',
    MACHINE_RS,
    '("HOME", own(&self.home)),',
    '("HOME", own(&self.home)),\n            ("SHELL", "/bin/zsh".into()),',
    'the_environment_is_these_variables_and_has_no_shell_to_ask_for_a_path',
  ),
  part(
    'the machine has no system commands on its PATH',
    MACHINE_RS,
    'format!("{}:{SYSTEM_PATH}", self.bin.display())',
    'self.bin.display().to_string()',
    'the_environment_is_these_variables_and_has_no_shell_to_ask_for_a_path',
  ),
  part(
    'a harness that wrote pid 0 is taken for one that wrote its pid',
    MACHINE_RS,
    '.filter(|pid| *pid > 0))',
    '.filter(|pid| *pid >= 0))',
    'the_pids_the_harness_wrote_are_one_to_a_line_and_nothing_else_is_taken_for_one',
  ),
  // What the app says, and what is made of it
  part(
    'an app that reports "failed" is waited on',
    APP_RS,
    'Some("failed" | "page-rejection") => true,',
    'Some("page-rejection") => true,',
    'a_page_error_that_is_not_a_resize_and_a_rejection_give_up_too_and_a_probe_does_not',
  ),
  part(
    'a page error that is not a resize is taken for one',
    APP_RS,
    '.contains("ResizeObserver loop"),',
    '.contains("ResizeObserver"),',
    'a_page_error_that_is_not_a_resize_and_a_rejection_give_up_too_and_a_probe_does_not',
  ),
  part(
    'an app whose output is over is waited on for the rest of the time',
    APP_RS,
    'if reports.ended {',
    'if reports.ended && false {',
    'an_app_whose_output_is_over_fails_the_wait_at_once_and_does_not_wait_out_the_patience',
  ),
  part(
    'the cf a daemon was started as is read with its arguments',
    APP_RS,
    'Some(PathBuf::from(said.strip_suffix(ARGS)?))',
    'Some(PathBuf::from(said))',
    'the_cf_the_app_started_as_its_daemon_is_the_one_its_log_names',
  ),
  part(
    'what a failure quotes is the start of the text',
    APP_RS,
    'let skip = text.chars().count().saturating_sub(count);',
    'let skip = 0;',
    'what_a_failure_quotes_is_the_end_of_the_text_whole_characters_only',
  ),
  // The Pi extension, read for what it imports
  part(
    'an extension that imports a file outside its folder is taken for one that loads from it',
    EXTENSION_RS,
    'if !target.starts_with(root) {',
    'if false {',
    'what_is_found_anywhere_but_the_folder_is_named',
  ),
  part(
    'an extension that imports a package by its name is taken for one that loads from its folder',
    EXTENSION_RS,
    'if specifier.starts_with("node:") {',
    'if !specifier.starts_with("./") && !specifier.starts_with("../") {',
    'what_is_found_anywhere_but_the_folder_is_named',
  ),
  part(
    'a method named import is taken for an import',
    EXTENSION_RS,
    String.raw`r#"(?:^|[^.\w$])(?:from\s*|import\s*\(?\s*)(?:'([^'\n]*)'|"([^"\n]*)")"#,`,
    String.raw`r#"(?:^|[^\w$])(?:from\s*|import\s*\(?\s*)(?:'([^'\n]*)'|"([^"\n]*)")"#,`,
    'what_a_module_imports_is_read_however_the_import_is_written',
  ),
  part(
    'a run that fails does not say where its machine is kept',
    MACHINE_RS,
    '        if kept {',
    '        if !kept {',
    'a_run_that_fails_says_where_its_machine_is_kept_and_one_that_passes_says_nothing',
  ),
  // The reader of a large paste
  plant(
    'packaged smoke: a paste is whatever the first piece is',
    PASTE_RS,
    'if paste.ends_with(PASTE_END) {',
    'if true {',
    [PASTE],
    'the_end_of_a_paste_is_found_across_the_pieces_it_was_cut_into',
  ),
  plant(
    'packaged smoke: the paste is said to be one byte more than it was',
    PASTE_RS,
    '        paste.len(),\n        Sha256::digest(paste)',
    '        paste.len() + 1,\n        Sha256::digest(paste)',
    [PASTE],
    'the_report_is_the_size_and_the_sha256_in_hex_with_the_line_end_of_a_raw_terminal',
  ),
  // The command that runs the smoke
  command(
    'the smoke is run with its case ignored',
    SUITES_RS,
    'pub(crate) const SMOKE: Suite = Suite {\n    tests: &["smoke"],\n    filter: None,\n    ignored: true,\n};',
    'pub(crate) const SMOKE: Suite = Suite {\n    tests: &["smoke"],\n    filter: None,\n    ignored: false,\n};',
    'the_smoke_is_the_smoke_test_of_cf_e2e_asked_for_its_ignored_case_and_its_own_output',
  ),
  command(
    'the smoke test says nothing of its own',
    COMMAND_RS,
    'cargo_test(context, SMOKE, &["--".into(), "--nocapture".into()])',
    'cargo_test(context, SMOKE, &[])',
    'the_smoke_is_the_smoke_test_of_cf_e2e_asked_for_its_ignored_case_and_its_own_output',
  ),
  command(
    'the app it is given is not told to the test',
    COMMAND_RS,
    'Some(app) => invocation.var(APP_VARIABLE, context.root.join(app)),',
    'Some(_) => invocation,',
    'the_app_it_is_given_is_the_one_variable_it_sets_and_a_relative_path_is_from_the_root',
  ),
  command(
    'a relative app is from wherever the command was run',
    COMMAND_RS,
    'Some(app) => invocation.var(APP_VARIABLE, context.root.join(app)),',
    'Some(app) => invocation.var(APP_VARIABLE, app),',
    'the_app_it_is_given_is_the_one_variable_it_sets_and_a_relative_path_is_from_the_root',
  ),
  command(
    'an app given as --app=path is not the app',
    COMMAND_RS,
    'Some((name, value)) if name.starts_with("--") => (name, Some(OsString::from(value))),',
    'Some((_, _)) => (text.as_ref(), None),',
    'the_app_is_named_by_app_with_a_value_or_joined_and_by_nothing_else',
  ),
  command(
    'a mac is refused the smoke',
    COMMAND_RS,
    'if !cfg!(target_os = "macos") {',
    'if cfg!(target_os = "macos") {',
    'smoke_runs_the_smoke_test_from_the_root_asking_for_its_ignored_case_and_the_app_it_is_given',
    [COMMAND_RUN],
  ),
  // The candidate: what it refuses
  candidate(
    'a bundle that kept the release identity is installed',
    CANDIDATE_RS,
    'if plist_value(&paths.built, "CFBundleIdentifier")? != IDENTIFIER {',
    'if plist_value(&paths.built, "CFBundleIdentifier")?.is_empty() {',
    'a_bundle_that_kept_the_release_identity_is_refused_before_it_is_smoked_or_installed',
  ),
  candidate(
    'a candidate that fails the smoke is installed',
    CANDIDATE_RS,
    'if system.run(&plan.smoke)? != 0 {',
    'if system.run(&plan.smoke)? == 12345 {',
    'a_candidate_that_fails_the_smoke_is_not_installed_and_the_one_there_stays',
  ),
  candidate(
    'a candidate that is running is built over',
    CANDIDATE_RS,
    'if !running_from(&table, &paths.target).is_empty() {',
    'if false && !running_from(&table, &paths.target).is_empty() {',
    'a_candidate_that_is_running_is_refused_before_anything_is_run_and_nothing_else_is_taken_for_it',
  ),
  candidate(
    'a candidate is installed in the system applications, beside the live app',
    CANDIDATE_RS,
    'target: home.join("Applications").join(NAME),',
    'target: applications.join(NAME),',
    'a_run_is_where_the_checkout_the_home_and_the_applications_put_it',
  ),
  // The candidate: how it installs
  candidate(
    'a copy cut short is built on',
    CANDIDATE_RS,
    '    remove_all(&next)?;\n    remove_all(&previous)?;\n',
    '    remove_all(&previous)?;\n',
    'an_install_replaces_what_is_there_and_clears_what_an_interrupted_one_left',
  ),
  candidate(
    'a swap cut short is built on',
    CANDIDATE_RS,
    '    remove_all(&next)?;\n    remove_all(&previous)?;\n',
    '    remove_all(&next)?;\n',
    'an_install_replaces_what_is_there_and_clears_what_an_interrupted_one_left',
  ),
  candidate(
    'the installed one is not put aside for the new one',
    CANDIDATE_RS,
    '    if target.exists() {\n        fs::rename(target, &previous).map_err(file("move aside", target))?;\n    }\n',
    '',
    'an_install_replaces_what_is_there_and_clears_what_an_interrupted_one_left',
  ),
  // The candidate: its roster and its record
  candidate(
    'the roster is copied from the live app every time',
    CANDIDATE_RS,
    'if !roster.exists() && paths.live_roster.exists() {',
    'if paths.live_roster.exists() {',
    'the_roster_is_the_live_apps_once_and_the_candidates_own_after',
  ),
  candidate(
    'the candidates home is open to everyone',
    CANDIDATE_RS,
    'folder.mode(0o700);',
    'folder.mode(0o755);',
    'the_candidates_home_and_its_roster_are_the_users_alone',
  ),
  candidate(
    'the candidates roster is open to everyone',
    CANDIDATE_RS,
    'fs::Permissions::from_mode(0o600)',
    'fs::Permissions::from_mode(0o644)',
    'the_candidates_home_and_its_roster_are_the_users_alone',
  ),
  candidate(
    'a blank line of git status is a file that is uncommitted',
    CANDIDATE_RS,
    '        .filter(|line| !line.is_empty())\n        .count();',
    '        .count();',
    'git_is_asked_in_the_checkout_and_what_it_counts_as_uncommitted_is_the_lines_it_says',
  ),
  candidate(
    'the build says it was made at the epoch',
    CANDIDATE_RS,
    '"builtAt": iso(clock.now_ms()),',
    '"builtAt": iso(0),',
    'a_good_run_builds_proves_the_bundle_installs_it_and_writes_down_what_it_was_in_that_order',
  ),
  candidate(
    'the live processes are said with no space after the comma',
    CANDIDATE_RS,
    'format!("; live PID {} running", pids.join(", "))',
    'format!("; live PID {} running", pids.join(","))',
    'the_live_processes_are_those_that_run_the_live_apps_program_whole_and_alone',
  ),
  // The candidate: the proof that the live app was not touched
  candidate(
    'a live app that was changed is not found',
    CANDIDATE_RS,
    'if after.app != before.app {',
    'if false && after.app != before.app {',
    'a_build_that_touched_the_live_app_is_found_whatever_it_touched',
  ),
  candidate(
    'a live roster that was changed is not found',
    CANDIDATE_RS,
    'if after.roster != before.roster {',
    'if false && after.roster != before.roster {',
    'a_build_that_touched_the_live_roster_is_found_and_said_with_what_may_explain_it',
  ),
  candidate(
    'a live app that stopped running is not found',
    CANDIDATE_RS,
    'if !system.alive(*pid) {',
    'if false && !system.alive(*pid) {',
    'a_live_app_that_stopped_running_during_the_build_is_found_by_the_process_that_is_gone',
  ),
  candidate(
    'a helper of the live app is taken for the live app',
    CANDIDATE_RS,
    '.filter(|row| row.command == live_program)',
    '.filter(|row| row.command.starts_with(&live_program))',
    'the_live_processes_are_those_that_run_the_live_apps_program_whole_and_alone',
  ),
  candidate(
    'a change to the mode of a file of the live app is not one',
    CANARY_RS,
    '            hash.update(mode_of(&kind).to_string().as_bytes());\n',
    '',
    'a_link_made_to_name_another_file_and_a_file_made_executable_are_changes_too',
  ),
  candidate(
    'a link of the live app made to name another file is not a change',
    CANARY_RS,
    '            hash.update(target.to_string_lossy().as_bytes());\n',
    '',
    'a_link_made_to_name_another_file_and_a_file_made_executable_are_changes_too',
  ),
  // The candidate: the system it is run on
  candidate(
    'the status a program ended with is lost',
    SYSTEM_RS,
    'Ok(process::run(invocation, self.env)?)',
    '{\n            process::run(invocation, self.env)?;\n            Ok(0)\n        }',
    'the_programs_it_runs_end_with_their_status_and_what_they_wrote_is_kept',
  ),
  candidate(
    'the table of processes says there are none',
    SYSTEM_RS,
    'Ok(processes::process_table()?)',
    'Ok(Vec::new())',
    'the_table_is_the_systems_own_and_this_process_is_in_it',
  ),
  candidate(
    'a process that is gone is taken for one that runs',
    SYSTEM_RS,
    '        processes::alive(pid)\n',
    '        let _ = pid;\n        true\n',
    'a_process_is_alive_while_it_runs_and_not_once_it_has_gone',
  ),
  // What only the app shows: the packaged smoke, run on the app a build leaves
  real(
    'the harness finds no system command on its PATH',
    HARNESS_SH,
    'if command -v uname >/dev/null 2>&1; then',
    'if command -v no-such-command-of-the-system >/dev/null 2>&1; then',
  ),
  real(
    'the daemon is held to the program of the app and not to its cf',
    TEST_RS,
    'let bundled = std::fs::canonicalize(&bundle.cf)?;',
    'let bundled = std::fs::canonicalize(&bundle.binary)?;',
  ),
  real(
    'the second cf ui is held to a refusal the ledger does not give',
    TEST_RS,
    'consensflow\\.db open$',
    'consensflow\\.sqlite open$',
  ),
  real(
    'the agents are proved on a home that is not the daemons',
    TEST_RS,
    'home: &machine.state,',
    'home: &machine.probe,',
  ),
]
