/**
 * Plants in the thin drivers of xtask (tools/xtask: `app test`, `app clippy`,
 * `clippy-windows` and the archiver it has cc-rs run, `departures`, `bench
 * records-memory`) and in `check`, the one gate that runs them: an argument
 * dropped, the app's resources left on, the archiver's re-entry broken, an
 * exit status lost. The tests of each module, and `tests/drivers.rs`,
 * `tests/bench.rs` and `tests/cli.rs`, which run them as processes against a
 * stand-in for cargo, must catch each.
 */

const cargo = (...args) => ['cargo', 'test', '--offline', '-p', 'xtask', ...args]

const APP = cargo('--lib', 'app::')
const DEPARTURES = cargo('--lib', 'departures::')
const WINDOWS = cargo('--lib', 'clippy_windows::')
const BENCH = cargo('--lib', 'bench::')
const CHECK = cargo('--lib', 'check::')
/** The commands as processes, against a cargo that says where it ran, with what, and ends as told. */
const DRIVERS = cargo('--test', 'drivers')
/** `bench records-memory` as a process, the same way. */
const BENCH_RUN = cargo('--test', 'bench')
const CLI = cargo('--test', 'cli')

const APP_RS = 'tools/xtask/src/app.rs'
const DEPARTURES_RS = 'tools/xtask/src/departures.rs'
const WINDOWS_RS = 'tools/xtask/src/clippy_windows.rs'
const BENCH_RS = 'tools/xtask/src/bench.rs'
const CHECK_RS = 'tools/xtask/src/check.rs'

const plant = (name, file, from, to, runs, meant) => ({
  name: `drivers: ${name}`,
  edits: [[file, from, to]],
  runs,
  meant,
})

/** What each of the four commands that run cargo and answer its status is meant to say of the status. */
const STATUS = 'the_exit_status_of_cargo_is_the_commands_own_and_it_says_nothing_of_its_own'

const TEST_ARGS = '        .args(["test", "--offline", "--"])\n        .args(args.iter().cloned())'
const CLIPPY_ARGS =
  '            "--all-targets",\n            "--",\n            "-D",\n            "warnings",\n        ])'
const ARCHIVE_OPEN = '            .create(true)\n            .append(true)'

export const PLANTS = [
  // app test, app clippy
  plant(
    'app test drops the words it was given',
    APP_RS,
    TEST_ARGS,
    '        .args(["test", "--offline", "--"])\n        .args(None::<OsString>)',
    [APP, DRIVERS],
    'the_app_tests_are_cargo_test_offline_in_the_app_with_the_words_after_a_double_dash',
  ),
  plant(
    'app test gives its words to cargo, before the double dash, and not to the tests',
    APP_RS,
    TEST_ARGS,
    '        .args(["test", "--offline"])\n        .args(args.iter().cloned())\n        .arg("--")',
    [APP, DRIVERS],
    'the_app_tests_are_cargo_test_offline_in_the_app_with_the_words_after_a_double_dash',
  ),
  plant(
    'app test is not told --offline',
    APP_RS,
    '.args(["test", "--offline", "--"])',
    '.args(["test", "--"])',
    [APP, DRIVERS],
    'the_app_tests_are_cargo_test_offline_in_the_app_with_the_words_after_a_double_dash',
  ),
  plant(
    'app clippy does not deny warnings',
    APP_RS,
    CLIPPY_ARGS,
    '            "--all-targets",\n            "--",\n        ])',
    [APP, DRIVERS],
    'the_app_lint_is_cargo_clippy_offline_over_every_target_with_warnings_denied_and_the_words_last',
  ),
  plant(
    'app clippy leaves the tests unlinted',
    APP_RS,
    '            "--all-targets",\n',
    '',
    [APP, DRIVERS],
    'the_app_lint_is_cargo_clippy_offline_over_every_target_with_warnings_denied_and_the_words_last',
  ),
  plant(
    'the app is built with its resources: the configuration is not given',
    APP_RS,
    'invocation.var(NO_RESOURCES.0, NO_RESOURCES.1)',
    'invocation',
    [APP, DRIVERS],
    'both_leave_the_apps_resources_out_of_the_configuration_and_nothing_else_of_the_environment',
  ),
  plant(
    'the app is built in the checkout, not in its own folder',
    APP_RS,
    'Invocation::new("cargo", context.path(APP))',
    'Invocation::new("cargo", &context.root)',
    [APP, DRIVERS],
    'the_app_tests_are_cargo_test_offline_in_the_app_with_the_words_after_a_double_dash',
  ),
  plant(
    'app test answers 0 whatever cargo ended with',
    APP_RS,
    'Ok(process::run(&test(context, args), &context.env)?)',
    'process::run(&test(context, args), &context.env)?;\n    Ok(0)',
    [DRIVERS],
    STATUS,
  ),
  plant(
    'app clippy answers 0 whatever cargo ended with',
    APP_RS,
    'Ok(process::run(&clippy(context, args), &context.env)?)',
    'process::run(&clippy(context, args), &context.env)?;\n    Ok(0)',
    [DRIVERS],
    STATUS,
  ),

  // departures
  plant(
    'departures does not set the variable that records',
    DEPARTURES_RS,
    '\n        .var("CF_RERECORD_DEPARTED", "1")',
    '',
    [DEPARTURES, DRIVERS],
    'the_traces_are_recorded_by_the_engines_dispatcher_tests_run_from_the_root_with_the_variable_set',
  ),
  plant(
    'departures runs another test of the engine',
    DEPARTURES_RS,
    '"--test", "dispatcher"',
    '"--test", "traces"',
    [DEPARTURES, DRIVERS],
    'the_traces_are_recorded_by_the_engines_dispatcher_tests_run_from_the_root_with_the_variable_set',
  ),
  plant(
    'departures answers 0 whatever cargo ended with',
    DEPARTURES_RS,
    'Ok(process::run(&rerecord(context), &context.env)?)',
    'process::run(&rerecord(context), &context.env)?;\n    Ok(0)',
    [DRIVERS],
    STATUS,
  ),
  plant(
    'departures takes an argument and runs on',
    DEPARTURES_RS,
    'if !args.is_empty() {',
    'if false {',
    [DEPARTURES, DRIVERS],
    'departures_takes_no_arguments_and_starts_nothing_without_them',
  ),

  // clippy-windows and its archiver
  plant(
    'clippy-windows compiles the C',
    WINDOWS_RS,
    '.var(CC, "true")',
    '.var(CC, "cc")',
    [WINDOWS, DRIVERS],
    'the_c_is_not_compiled_and_the_archive_is_made_by_xtask_run_as_the_archiver',
  ),
  plant(
    'clippy-windows does not tell the archiver it is one: the re-entry word is another',
    WINDOWS_RS,
    'as_archiver.push(AS_ARCHIVER);',
    'as_archiver.push("--archiver");',
    [WINDOWS, DRIVERS],
    'the_c_is_not_compiled_and_the_archive_is_made_by_xtask_run_as_the_archiver',
  ),
  plant(
    'clippy-windows has cc-rs run another program as the archiver',
    WINDOWS_RS,
    '    std::env::current_exe().map_err(doing("find the xtask that cc-rs is to run as the archiver"))',
    '    Ok::<_, io::Error>(PathBuf::from("ar")).map_err(doing("find the xtask that cc-rs is to run as the archiver"))',
    [WINDOWS, DRIVERS],
    'xtasks_own_program_is_the_archiver_it_is_told_to_run_as',
  ),
  plant(
    'the archiver is not a command of xtask',
    WINDOWS_RS,
    'words: &[AS_ARCHIVER],',
    'words: &["archive"],',
    [WINDOWS, DRIVERS],
    'the_two_commands_run_in_rust_and_the_archiver_is_the_word_cc_rs_is_told',
  ),
  plant(
    'the archiver does not make the archive',
    WINDOWS_RS,
    ARCHIVE_OPEN,
    '            .create(false)\n            .append(true)',
    [WINDOWS, DRIVERS],
    'the_archive_is_made_empty_when_there_is_none_and_left_as_it_is_when_there_is',
  ),
  plant(
    'the archiver empties an archive that is there',
    WINDOWS_RS,
    ARCHIVE_OPEN,
    '            .create(true)\n            .write(true)\n            .truncate(true)',
    [WINDOWS, DRIVERS],
    'the_archive_is_made_empty_when_there_is_none_and_left_as_it_is_when_there_is',
  ),
  plant(
    'the archiver looks for -out: in the first word only',
    WINDOWS_RS,
    'let named = words.iter().find_map(|word| {',
    'let named = words.iter().take(1).find_map(|word| {',
    [WINDOWS, DRIVERS],
    'an_archiver_line_names_its_archive_as_ar_does_or_as_lib_does',
  ),
  plant(
    'the archiver takes -OUT: for no option',
    WINDOWS_RS,
    'bytes[1..4].eq_ignore_ascii_case(b"out")',
    'bytes[1..4] == *b"out"',
    [WINDOWS, DRIVERS],
    'an_archiver_line_names_its_archive_as_ar_does_or_as_lib_does',
  ),
  plant(
    'the archiver takes /out: for no option',
    WINDOWS_RS,
    "matches!(bytes[0], b'-' | b'/')",
    "bytes[0] == b'-'",
    [WINDOWS, DRIVERS],
    'an_archiver_line_names_its_archive_as_ar_does_or_as_lib_does',
  ),
  plant(
    // Run as a process only: the archive it would make, named `cq`, is made in the
    // folder the test runs it in, which is a temporary one, not in the checkout.
    'the archiver takes the first word for the archive where there is no -out:',
    WINDOWS_RS,
    'words.get(1).map(|word| &**word)',
    'words.first().map(|word| &**word)',
    [DRIVERS],
    'the_archiver_makes_the_empty_archive_it_is_asked_for_leaves_one_that_is_there_and_says_nothing',
  ),
  plant(
    'clippy-windows lints the app with the workspace by default',
    WINDOWS_RS,
    '["--workspace", "--exclude", "app"]',
    '["--workspace"]',
    [WINDOWS, DRIVERS],
    'the_workspace_but_the_app_is_linted_for_the_target_from_the_root_when_no_crate_is_named',
  ),
  plant(
    'clippy-windows lints the whole workspace though crates are named',
    WINDOWS_RS,
    '        crates\n            .iter()\n            .flat_map(|name| [OsString::from("-p"), name.clone()])\n            .collect()',
    '        ["--workspace"].map(OsString::from).to_vec()',
    [WINDOWS, DRIVERS],
    'each_crate_named_is_one_package_and_the_workspace_is_not_linted_whole',
  ),
  plant(
    'clippy-windows is not told --offline',
    WINDOWS_RS,
    '.args(["clippy", "--offline", "--target", TARGET, "--all-targets"])',
    '.args(["clippy", "--target", TARGET, "--all-targets"])',
    [WINDOWS, DRIVERS],
    'the_workspace_but_the_app_is_linted_for_the_target_from_the_root_when_no_crate_is_named',
  ),
  plant(
    'clippy-windows lints the app with its resources on',
    WINDOWS_RS,
    '    if names_the_app(crates) {\n        app::without_resources(invocation)',
    '    if false {\n        app::without_resources(invocation)',
    [WINDOWS, DRIVERS],
    'the_app_is_linted_with_its_resources_left_out_when_it_is_named_among_the_crates',
  ),
  plant(
    'clippy-windows does not put the resource compiler that does nothing on the PATH',
    WINDOWS_RS,
    '    if names_the_app(args) {\n        lint = lint.var("PATH", path_with_stand_in(context)?);\n    }',
    '    let _ = path_with_stand_in;',
    [DRIVERS],
    'clippy_windows_of_the_app_leaves_its_resources_out_and_finds_a_resource_compiler_that_does_nothing',
  ),
  plant(
    'clippy-windows puts the resource compiler that does nothing last on the PATH',
    WINDOWS_RS,
    'std::env::join_paths(iter::once(folder.clone()).chain(kept))',
    'std::env::join_paths(kept.chain(iter::once(folder.clone())))',
    [WINDOWS, DRIVERS],
    'the_stand_in_is_the_first_place_the_path_names_and_the_others_follow_in_order',
  ),
  plant(
    'the resource compiler that does nothing is no program',
    WINDOWS_RS,
    '        fs::set_permissions(file, fs::Permissions::from_mode(0o755))?;',
    '        let _ = file;',
    [WINDOWS, DRIVERS],
    'the_stand_in_is_a_program_that_compiles_nothing_and_ends_with_0',
  ),
  plant(
    'clippy-windows answers 0 whatever cargo ended with',
    WINDOWS_RS,
    'Ok(process::run(&lint, &context.env)?)',
    'process::run(&lint, &context.env)?;\n    Ok(0)',
    [DRIVERS],
    STATUS,
  ),

  // bench records-memory
  plant(
    'bench says the lines of the test harness',
    BENCH_RS,
    '!line.is_empty() && !line.starts_with("running") && !line.starts_with("test ")',
    '!line.is_empty() && !line.starts_with("test ")',
    [BENCH, BENCH_RUN],
    'only_a_line_that_starts_with_running_or_test_is_the_harnesss',
  ),
  plant(
    'bench says the empty lines',
    BENCH_RS,
    '!line.is_empty() && !line.starts_with("running")',
    '!line.starts_with("running")',
    [BENCH, BENCH_RUN],
    'each_measure_has_its_heading_and_its_own_lines_indented_and_nothing_of_cargos_or_the_harness',
  ),
  plant(
    'bench ends with 0 when the first look fails',
    BENCH_RS,
    '            if !look(name, &which, &mut capture, out, err)? {\n                return Ok(1);',
    '            if !look(name, &which, &mut capture, out, err)? {\n                return Ok(0);',
    [BENCH, BENCH_RUN],
    'whichever_of_the_four_measures_fails_the_status_is_1_and_the_ones_after_it_are_not_run',
  ),
  plant(
    'bench ends with 0 when one of the last two fails',
    BENCH_RS,
    '        if !look(name, "", &mut capture, out, err)? {\n            return Ok(1);',
    '        if !look(name, "", &mut capture, out, err)? {\n            return Ok(0);',
    [BENCH, BENCH_RUN],
    'whichever_of_the_four_measures_fails_the_status_is_1_and_the_ones_after_it_are_not_run',
  ),
  plant(
    'bench goes on after a measure that failed',
    BENCH_RS,
    '        write!(err, "{}{}", done.stdout, done.stderr)?;',
    '        write!(err, "{}{}", done.stdout, done.stderr)?;\n        return Ok(true);',
    [BENCH, BENCH_RUN],
    'whichever_of_the_four_measures_fails_the_status_is_1_and_the_ones_after_it_are_not_run',
  ),
  plant(
    'bench does not say what a failed measure wrote',
    BENCH_RS,
    '        write!(err, "{}{}", done.stdout, done.stderr)?;\n',
    '',
    [BENCH, BENCH_RUN],
    'a_measure_that_fails_says_all_it_wrote_and_which_it_was_and_the_status_is_1_whatever_it_ended_with',
  ),
  plant(
    'bench runs the first look once whatever --runs says',
    BENCH_RS,
    'for run in 1..=runs {',
    'for run in 1..=runs.min(1) {',
    [BENCH, BENCH_RUN],
    'more_runs_repeat_the_first_look_and_the_look_in_parts_each_in_turn_and_the_others_once',
  ),
  plant(
    'bench does not say which run it is',
    BENCH_RS,
    'format!(" (run {run})")',
    'String::new()',
    [BENCH, BENCH_RUN],
    'more_runs_repeat_the_first_look_and_the_look_in_parts_each_in_turn_and_the_others_once',
  ),
  plant(
    'bench does not give the lines to the measures',
    BENCH_RS,
    'invocation = invocation.var("CF_RECORDS_MEMORY_LINES", lines);',
    'let _ = lines;',
    [BENCH, BENCH_RUN],
    'the_lines_and_the_transcript_reach_the_measures_by_the_variables_they_read',
  ),
  plant(
    'bench does not give the transcript to the measures',
    BENCH_RS,
    'invocation = invocation.var("CF_RECORDS_MEMORY_TRANSCRIPT", transcript);',
    'let _ = transcript;',
    [BENCH, BENCH_RUN],
    'the_lines_and_the_transcript_reach_the_measures_by_the_variables_they_read',
  ),
  plant(
    'bench measures a repo from where xtask is run, not from the checkout',
    BENCH_RS,
    '|dir| context.root.join(dir))',
    '|dir| dir.clone())',
    [BENCH, BENCH_RUN],
    'a_repo_is_measured_from_where_it_is_named_from_the_roots_folder_when_relative',
  ),
  plant(
    'bench builds the measures for debugging, not release',
    BENCH_RS,
    '            "--release",\n',
    '',
    [BENCH, BENCH_RUN],
    'a_measure_is_one_release_built_ignored_test_of_the_library_run_alone',
  ),
  plant(
    'bench runs every measure that has a name that starts with the one it is',
    BENCH_RS,
    '.args(["--", "--ignored", "--nocapture", "--exact"])',
    '.args(["--", "--ignored", "--nocapture"])',
    [BENCH, BENCH_RUN],
    'a_measure_is_one_release_built_ignored_test_of_the_library_run_alone',
  ),
  plant(
    'bench takes an option for the value of another',
    BENCH_RS,
    "        Some(word) if !word.to_string_lossy().starts_with('-') => Ok(word.clone()),",
    '        Some(word) => Ok(word.clone()),',
    [BENCH, BENCH_RUN],
    'an_option_with_no_value_is_refused_and_so_is_one_followed_by_another_option',
  ),
  plant(
    'bench takes a number of runs that is no number for one',
    BENCH_RS,
    '.and_then(|text| text.parse().ok())',
    '.and_then(|text| text.parse().ok().or(Some(1)))',
    [BENCH, BENCH_RUN],
    'the_number_of_runs_is_a_whole_number_which_may_be_none',
  ),
  plant(
    'bench takes a word that is no option and ignores it',
    BENCH_RS,
    '                return Err(refused(arg));',
    '                continue;',
    [BENCH, BENCH_RUN],
    'a_word_that_is_no_option_it_has_is_refused_with_what_it_takes',
  ),

  // check, which runs them
  plant(
    'check does not lint the app',
    CHECK_RS,
    '        app::clippy(context, &[]),\n    ];',
    '    ];',
    [CHECK, CLI],
    'the_steps_are_the_gates_in_order_and_each_runs_where_and_as_its_own_command_does',
  ),
  plant(
    'check does not run the app tests',
    CHECK_RS,
    '        app::test(context, &[]),\n',
    '',
    [CHECK, CLI],
    'the_steps_are_the_gates_in_order_and_each_runs_where_and_as_its_own_command_does',
  ),
  plant(
    'check runs the Node tests on a cf it did not build again',
    CHECK_RS,
    '        at("cargo", &["xtask", "build-cf", "--offline"]),\n',
    '',
    [CHECK, CLI],
    'the_cf_the_node_tests_run_is_built_again_just_before_them',
  ),
  plant(
    'check builds the cf after the Node tests that run it',
    CHECK_RS,
    '        at("cargo", &["xtask", "build-cf", "--offline"]),\n        at("npm", &["test"]),',
    '        at("npm", &["test"]),\n        at("cargo", &["xtask", "build-cf", "--offline"]),',
    [CHECK, CLI],
    'the_cf_the_node_tests_run_is_built_again_just_before_them',
  ),
  plant(
    'check has no lint for Windows on any machine',
    CHECK_RS,
    '    if !windows {',
    '    if false {',
    [CHECK, CLI],
    'the_steps_are_the_gates_in_order_and_each_runs_where_and_as_its_own_command_does',
  ),
  plant(
    'check has a lint for Windows on Windows too',
    CHECK_RS,
    '    if !windows {',
    '    if true {',
    [CHECK, CLI],
    'on_windows_the_workspaces_clippy_is_the_one_for_windows_and_there_is_no_step_of_it',
  ),
  plant(
    'check lints for Windows with the app',
    CHECK_RS,
    'steps.push(clippy_windows::lint(context, &[], archiver));',
    'steps.push(clippy_windows::lint(context, &[OsString::from("app")], archiver));',
    [CHECK, CLI],
    'the_lint_for_windows_is_the_one_its_command_runs_with_xtask_as_its_archiver',
  ),
]
