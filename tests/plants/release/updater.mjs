/**
 * Plants in what the updater smoke (`npm run smoke:updater`, `cargo xtask
 * smoke-updater`) reads as proof: the app's process, its daemon's process and
 * readiness, the ledger's one holder, the ledger's contents, the terminal
 * command, the bundles it inspects and makes, the key and the versions it builds
 * with, and how a program is started for it (`spawn`). Each takes one check out,
 * and a test that needs no app (the tests of tools/xtask/src/updater_smoke, and
 * of tools/xtask/src/process.rs) must fail.
 *
 * The ones in the product are in updater-product.mjs.
 */
// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
import { EVIDENCE, LAUNCHERS, SMOKE_DIR, SMOKE_KIT, XTASK_PROCESS } from './kit.mjs'
import { PLANTS as PRODUCT } from './updater-product.mjs'

const PROCESS_RS = 'tools/xtask/src/process.rs'
const EVIDENCE_RS = `${SMOKE_DIR}/evidence.rs`
const HOLDER_RS = `${SMOKE_DIR}/evidence/holder.rs`
const LEDGER_RS = `${SMOKE_DIR}/ledger.rs`
const LAUNCHERS_RS = `${SMOKE_DIR}/launchers.rs`
const BUNDLE_RS = `${SMOKE_DIR}/bundle.rs`
const BUILD_RS = `${SMOKE_DIR}/build.rs`
const SIGNING_RS = `${SMOKE_DIR}/signing.rs`
const VERSIONS_RS = `${SMOKE_DIR}/versions.rs`
const PROCESSES_RS = `${SMOKE_DIR}/processes.rs`

const kit = (name, file, from, to, run, meant) => ({
  name: `updater evidence: ${name}`,
  edits: [[file, from, to]],
  runs: [run],
  meant,
})

/** What the readers of the process table and the daemon's log must refuse. */
const DAEMON = [
  kit(
    'a daemon that is not the app’s child is taken for its daemon',
    EVIDENCE_RS,
    'let mine: Vec<_> = rows.iter().filter(|row| row.ppid == seen.app).collect();',
    'let mine: Vec<_> = rows.iter().collect();',
    EVIDENCE,
    'refuses_a_daemon_that_is_not_the_apps_child',
  ),
  kit(
    'a home with a second daemon running is taken for one with a holder',
    EVIDENCE_RS,
    '        rows.len() == 1,\n        "one daemon serves the home',
    '        rows.len() == rows.len(),\n        "one daemon serves the home',
    EVIDENCE,
    'refuses_a_home_with_a_second_daemon_running',
  ),
  kit(
    'a daemon that logged no start line is taken for a ready one',
    EVIDENCE_RS,
    'let Some(start) = start_line(log, Some(row.pid)) else {',
    'let Some(start) = start_line(log, Some(row.pid)).or_else(|| start_line(log, None)) else {',
    EVIDENCE,
    'refuses_a_daemon_that_never_logged_its_start',
  ),
  kit(
    'a log that says one daemon where the process is the other is taken',
    EVIDENCE_RS,
    '        kind_of_command(&row.command) == start.kind,\n',
    '        start.kind == start.kind,\n',
    EVIDENCE,
    'refuses_a_log_that_says_one_daemon_where_the_process_is_the_other',
  ),
  kit(
    'the errors a daemon logged are not looked at',
    EVIDENCE_RS,
    '            error_lines(&written).is_empty(),\n',
    '            true,\n',
    EVIDENCE,
    'refuses_a_daemon_that_logged_an_error',
  ),
  kit(
    'a daemon that failed to start (exit 1) is not looked for',
    EVIDENCE_RS,
    '!written.iter().any(|line| refused_the_ledger(line)),',
    '!written.iter().any(|line| line.ends_with(" info never")),',
    EVIDENCE,
    'refuses_an_earlier_daemon_of_the_app_that_was_refused_its_start',
  ),
  kit(
    'a daemon of the app that was refused the ledger is let through at the end',
    EVIDENCE_RS,
    '        refused == probes.len(),\n',
    '        refused == refused,\n',
    EVIDENCE,
    'say_so_when_a_daemon_the_app_started_was_refused_the_ledger',
  ),
  kit(
    'a probe’s refusal counts against the app’s daemons',
    EVIDENCE_RS,
    '        .into_iter()\n        .filter(|each| !seen.probes.contains(each))\n    {',
    '        .into_iter()\n    {',
    EVIDENCE,
    'refuses_an_earlier_daemon_of_the_app_that_was_refused_its_start',
  ),
  kit(
    'a process that is not the app’s executable is taken for the app',
    EVIDENCE_RS,
    '        row.command == binary,\n',
    '        binary == binary,\n',
    EVIDENCE,
    'takes_the_app_for_the_process_that_is_the_bundles_executable',
  ),
  kit(
    'a zombie is taken for the running app',
    EVIDENCE_RS,
    ".filter(|row| !row.state.starts_with('Z'));",
    '.filter(|_| true);',
    EVIDENCE,
    'takes_the_app_for_the_process_that_is_the_bundles_executable',
  ),
  kit(
    'an app log that does not name the native daemon is taken for one that does',
    EVIDENCE_RS,
    "app_log_text.split('\\n').any(|line| line.ends_with(&said)),",
    'app_log_text.split(\'\\n\').any(|line| line.contains("native")),',
    EVIDENCE,
    'does_not_take_the_flips_sentence_another_bundles_cf_or_silence',
  ),
]

/** What the reader of the ledger’s one holder must refuse. */
const HOLDER = [
  kit(
    'a second ConsensFlow that ran is taken for one that was refused',
    HOLDER_RS,
    '        attempt.code == Some(1),\n',
    '        attempt.code == attempt.code,\n',
    EVIDENCE,
    'is_no_proof_when_the_second_one_ran',
  ),
  kit(
    'the ledger’s words are not looked for in the refusal',
    HOLDER_RS,
    '            .contains(&format!("another ConsensFlow has {db} open")),',
    '            .contains(""),',
    EVIDENCE,
    'is_no_proof_when_the_second_one_ran',
  ),
  kit(
    'a probe that never ended is taken for a refused one',
    HOLDER_RS,
    '        attempt.signal.is_none(),\n',
    '        true,\n',
    EVIDENCE,
    'is_no_proof_when_the_second_one_ran',
  ),
  kit(
    'a handle line from the second ConsensFlow is let through',
    HOLDER_RS,
    '        attempt.out.is_empty(),\n',
    '        true,\n',
    EVIDENCE,
    'is_no_proof_when_the_second_one_ran',
  ),
]

/** What the reader of the ledger’s contents must refuse. */
const CONTENTS = [
  kit(
    'a column that changed is let through',
    LEDGER_RS,
    'if REWRITTEN.contains(&column.as_str()) {',
    'if true {',
    EVIDENCE,
    'is_no_longer_whole_when_a_row_is_gone',
  ),
  kit(
    'a row that is gone is let through',
    LEDGER_RS,
    '            let Some(kept) = kept else {\n                return Err(Error::new(format!(\n                    "{table} {} is gone: {}",\n                    row.id(),\n                    row.json()\n                )));\n            };',
    '            let Some(kept) = kept else {\n                continue;\n            };',
    EVIDENCE,
    'is_no_longer_whole_when_a_row_is_gone',
  ),
  kit(
    'a table that is gone is let through',
    LEDGER_RS,
    '        let Some(now) = after.tables.get(table) else {\n            return Err(Error::new(format!("the ledger lost its {table} table")));\n        };',
    '        let Some(now) = after.tables.get(table) else {\n            continue;\n        };',
    EVIDENCE,
    'is_no_longer_whole_when_a_row_is_gone',
  ),
  kit(
    'a schema that went back is let through',
    LEDGER_RS,
    '        ledger.version >= at_least,\n',
    '        true,\n',
    EVIDENCE,
    'is_not_sound_when_its_schema_is_below',
  ),
  kit(
    'SQLite’s own check of the file is not asked',
    LEDGER_RS,
    '        ledger.integrity == ["ok"],\n',
    '        true,\n',
    EVIDENCE,
    'is_not_sound_when_its_schema_is_below',
  ),
  kit(
    'a reference that does not hold is let through',
    LEDGER_RS,
    '        ledger.references.is_empty(),\n',
    '        true,\n',
    EVIDENCE,
    'is_not_sound_when_its_schema_is_below',
  ),
  kit(
    'a project that is not in the ledger is let through',
    LEDGER_RS,
    '            held.contains(&directory.as_ref()),\n',
    '            true,\n',
    EVIDENCE,
    'has_a_project_for_each_directory_asked_for',
  ),
  kit(
    'an event the daemon traced and the ledger lost is let through',
    LEDGER_RS,
    '            held.contains(&key),\n',
    '            true,\n',
    EVIDENCE,
    'reads_the_events_a_daemon_traced',
  ),
]

/** What the reader of the terminal command must refuse. */
const COMMANDS = [
  kit(
    'no command is looked at: all are taken for repaired',
    LAUNCHERS_RS,
    '    for name in &planted.repaired {\n',
    '    for name in &Vec::<&str>::new() {\n',
    LAUNCHERS,
    'is_not_repaired_while_it_still_names_nodes',
  ),
  kit(
    'only the first name is looked at: the second is taken for repaired',
    LAUNCHERS_RS,
    '    for name in &planted.repaired {\n',
    '    for name in planted.repaired.iter().take(1) {\n',
    LAUNCHERS,
    'is_not_repaired_when_the_second_name_was_left_as_it_was',
  ),
  kit(
    'a command that does not run the update’s cf is taken for repaired',
    LAUNCHERS_RS,
    '            text.contains(&format!("exec \\"{}\\" \\"$@\\"", cf.display())),\n',
    '            true,\n',
    LAUNCHERS,
    'is_not_repaired_while_it_still_names_nodes',
  ),
  kit(
    'a repaired command that lost its pin is taken for repaired',
    LAUNCHERS_RS,
    '            text.contains(&format!(\n                "export CONSENSFLOW_HOME=\\"{}\\"",\n                box_state.display()\n            )),\n',
    '            true,\n',
    LAUNCHERS,
    'is_not_repaired_while_it_still_names_nodes',
  ),
  kit(
    'a repaired command that lost its mark is taken for repaired',
    LAUNCHERS_RS,
    '        ensure!(text.contains(MARKER), "{shown} lost its mark");',
    '        ensure!(true, "{shown} lost its mark");',
    LAUNCHERS,
    'is_not_repaired_while_it_still_names_nodes',
  ),
  kit(
    'a repaired command that still names Node’s is taken for repaired',
    LAUNCHERS_RS,
    '            !(text.contains("cf.mjs") || text.contains("MacOS/node")),\n',
    '            true,\n',
    LAUNCHERS,
    'is_not_repaired_while_it_still_names_nodes',
  ),
  kit(
    'a repaired command that cannot be run is taken for repaired',
    LAUNCHERS_RS,
    '        ensure!(is_executable(&file)?, "{shown} is not executable");',
    '        ensure!(true, "{shown} is not executable");',
    LAUNCHERS,
    'is_not_repaired_while_it_still_names_nodes',
  ),
  kit(
    'a command that runs another cf than the daemon is taken for repaired',
    LAUNCHERS_RS,
    '            version_of(sandbox, box_state, name)? == version,\n',
    '            version == version,\n',
    LAUNCHERS,
    'is_no_repair_when_it_runs_another_cf',
  ),
  kit(
    'an app log that does not say the repair is taken for one that does',
    LAUNCHERS_RS,
    "                app_log.split('\\n').any(|line| line.ends_with(&said)),\n",
    "                app_log.split('\\n').any(|_| true),\n",
    LAUNCHERS,
    'is_not_own_home_only',
  ),
  kit(
    'a command that serves another home, changed in this home’s bin, is let through',
    LAUNCHERS_RS,
    '            now[name] == planted.own[name],\n',
    '            now[name] == now[name],\n',
    LAUNCHERS,
    'is_not_own_home_only',
  ),
  kit(
    'an app log that speaks of the command pinned to another home is let through',
    LAUNCHERS_RS,
    '            !app_log.contains(&file.display().to_string()),\n',
    '            true,\n',
    LAUNCHERS,
    'is_not_own_home_only',
  ),
  kit(
    'the commands of another home, changed, are let through',
    LAUNCHERS_RS,
    '        commands_of(&sandbox.other)? == planted.other,\n',
    '        commands_of(&sandbox.other)? == commands_of(&sandbox.other)?,\n',
    LAUNCHERS,
    'is_not_own_home_only',
  ),
  kit(
    'an app log that speaks of another home’s command is let through',
    LAUNCHERS_RS,
    '        !app_log.contains(&bin_of(&sandbox.other).display().to_string()),\n',
    '        true,\n',
    LAUNCHERS,
    'is_not_own_home_only',
  ),
]

/** What the inspection of a bundle, the refused bundles it makes, and what a build is given, must hold to. */
const BUNDLES = [
  kit(
    'a process that is under the app but not its child is not looked for',
    PROCESSES_RS,
    'if row.ppid == parent && !found.contains(&row.pid) {',
    'if row.ppid == root && !found.contains(&row.pid) {',
    SMOKE_KIT,
    'finds_everything_under_a_process',
  ),
  kit(
    'a tree is not ended, whatever the app made of it',
    PROCESSES_RS,
    '        if under.is_empty() && !alive(root) {\n            return;\n        }\n',
    '        return;\n',
    SMOKE_KIT,
    'ends_a_process_and_all_that_is_under_it',
  ),
  kit(
    'a bundle of another app is taken',
    BUNDLE_RS,
    '        identifier == IDENTITY,\n',
    '        IDENTITY == IDENTITY,\n',
    SMOKE_KIT,
    'refuses_another_app_two_versions_in_one_plist',
  ),
  kit(
    'a plist with two versions is taken',
    BUNDLE_RS,
    '        plist_value(app, "CFBundleVersion")? == version,\n',
    '        version == version,\n',
    SMOKE_KIT,
    'refuses_another_app_two_versions_in_one_plist',
  ),
  kit(
    'a bundle with no cf is taken',
    BUNDLE_RS,
    'for (what, path) in [("native executable", &binary), ("window\'s cf", &cf)] {',
    'for (what, path) in [("native executable", &binary)] {',
    SMOKE_KIT,
    'refuses_another_app_two_versions_in_one_plist',
  ),
  kit(
    'a bundle with half of Node’s files is taken',
    BUNDLE_RS,
    '        has.iter().all(|there| *there) || !has.iter().any(|there| *there),\n',
    '        true,\n',
    SMOKE_KIT,
    'refuses_another_app_two_versions_in_one_plist',
  ),
  kit(
    'a bundle signed again is not verified: any seal passes',
    BUNDLE_RS,
    'pub fn verify_seal(app: &Path, env: &Env) -> Result {\n',
    'pub fn verify_seal(app: &Path, env: &Env) -> Result {\n    if app.exists() {\n        let _ = env;\n        return Ok(());\n    }\n',
    SMOKE_KIT,
    'makes_the_refused_bundles_the_way_the_smoke_needs_them',
  ),
  kit(
    'the bundle with no cf is not signed again, so the seal refuses it before the rule does',
    BUNDLE_RS,
    '                run(\n                    "/usr/bin/codesign",\n                    &args!["--force", "--deep", "--sign", "-", app],\n                    env,\n                )\n                .map(drop)\n',
    '                let _ = env;\n                Ok(())\n',
    SMOKE_KIT,
    'makes_the_refused_bundles_the_way_the_smoke_needs_them',
  ),
  kit(
    'the tampered bundle is not tampered with',
    BUNDLE_RS,
    '                file.write_all(b"changed after signing")\n                    .map_err(files("write", &cf))\n',
    '                let _ = &mut file;\n                Ok(())\n',
    SMOKE_KIT,
    'makes_the_refused_bundles_the_way_the_smoke_needs_them',
  ),
  kit(
    'a replacement leaves the old files where they were',
    BUNDLE_RS,
    '    remove_all(to)?;\n    copy_over(from, to, env)\n',
    '    copy_over(from, to, env)\n',
    SMOKE_KIT,
    'copies_a_bundle_whole_replacing_what_was_there',
  ),
  kit(
    'a build that does not carry the run’s key is taken',
    BUILD_RS,
    '        holds(public_key),\n',
    '        true,\n',
    SMOKE_KIT,
    'a_build_is_held_to_its_key',
  ),
  kit(
    'a build that carries the product’s key is taken',
    BUILD_RS,
    '        !holds(product_key),\n',
    '        true,\n',
    SMOKE_KIT,
    'a_build_is_held_to_its_key',
  ),
  kit(
    'the update is not given its version',
    BUILD_RS,
    '        config.insert("version".into(), json!(version));\n',
    '        let _ = (config, version);\n',
    SMOKE_KIT,
    'a_build_is_given_the_override_of_the_runs_public_key',
  ),
  kit(
    'a build inherits the signing variables of the shell it runs in',
    SIGNING_RS,
    'const SIGNING_VARIABLES: [&str; 3] = ["TAURI_", "APPLE_", "CSC_"];',
    'const SIGNING_VARIABLES: [&str; 3] = ["NOTHING_", "NOTHING_", "NOTHING_"];',
    SMOKE_KIT,
    'a_build_inherits_no_updater_key_apple_certificate_or_identity',
  ),
  kit(
    'a build may use the network',
    SIGNING_RS,
    '("CARGO_NET_OFFLINE".into(), "true".into())',
    '("CARGO_NET_OFFLINE".into(), "false".into())',
    SMOKE_KIT,
    'a_build_inherits_no_updater_key_apple_certificate_or_identity',
  ),
  kit(
    'the update is given the checkout’s version, which is the installed app’s',
    VERSIONS_RS,
    '    if compare_versions(checkout, latest)? == Ordering::Greater {\n',
    '    if true {\n',
    SMOKE_KIT,
    'is_the_next_one_after_the_newest_installed_apps',
  ),
  kit(
    'the update is given a version newer than the first installed app’s alone',
    VERSIONS_RS,
    '    let latest = newest(installed)?;\n',
    '    let latest = installed[0];\n',
    SMOKE_KIT,
    'is_the_next_one_after_the_newest_installed_apps',
  ),
  kit(
    'the flip is the oldest release after the bridge',
    VERSIONS_RS,
    '        if compare_versions(version, latest_version)? == Ordering::Greater {\n',
    '        if compare_versions(version, latest_version)? == Ordering::Less {\n',
    SMOKE_KIT,
    'has_the_flip_release_the_newest_tag_after_the_bridges',
  ),
  kit(
    'the release under test is taken for the flip',
    BUILD_RS,
    '        .filter(|tag| !here.contains(tag))\n',
    '        .filter(|_| true)\n',
    SMOKE_KIT,
    'has_the_flip_among_the_tags_of_this_checkouts_history_but_for_the_one_under_test',
  ),
  kit(
    'the flip’s terminal command is repaired though it was current',
    LAUNCHERS_RS,
    '        if release.setup() == Kind::Native {\n            ensure!(\n                *text == planted.own[name],',
    '        if false {\n            ensure!(\n                *text == planted.own[name],',
    LAUNCHERS,
    'is_current_it_names_the_cf_of_the_bundle_which_the_updates_is_at',
  ),
  kit(
    'the app that started is not held to the bundle’s cf',
    EVIDENCE_RS,
    'line.ends_with(&said)',
    'line.contains("starting the daemon: ")',
    EVIDENCE,
    'does_not_take_the_flips_sentence_another_bundles_cf_or_silence',
  ),
]

/** What a program the smoke starts is given: its folder, environment and input, its pipes, and its group. */
const SPAWNED = [
  kit(
    'a program asked for a process group of its own is left in the smoke’s',
    PROCESS_RS,
    'if own_group {',
    'if false {',
    XTASK_PROCESS,
    'leads_a_process_group_of_its_own',
  ),
  kit(
    'a program not asked for a process group of its own is given one',
    PROCESS_RS,
    'if own_group {',
    'if true {',
    XTASK_PROCESS,
    'leads_a_process_group_of_its_own',
  ),
  kit(
    'a program is not given the input it is told',
    PROCESS_RS,
    '.stdin(stdin)',
    '.stdin(Stdio::null())',
    XTASK_PROCESS,
    'has_its_folder_its_environment_and_its_input',
  ),
  kit(
    'a program’s output is not given in a pipe',
    PROCESS_RS,
    '.stdout(Stdio::piped())',
    '.stdout(Stdio::null())',
    XTASK_PROCESS,
    'has_its_folder_its_environment_and_its_input',
  ),
  kit(
    'a program’s error output is not given in a pipe',
    PROCESS_RS,
    '.stderr(Stdio::piped());',
    '.stderr(Stdio::null());',
    XTASK_PROCESS,
    'has_its_folder_its_environment_and_its_input',
  ),
  kit(
    'a program is not run in the folder it is told',
    PROCESS_RS,
    '        .current_dir(&invocation.cwd)\n',
    '',
    XTASK_PROCESS,
    'has_its_folder_its_environment_and_its_input',
  ),
  kit(
    'a variable a program is told to go without is left in its environment',
    PROCESS_RS,
    'None => command.env_remove(name),',
    'None => &mut command,',
    XTASK_PROCESS,
    'has_its_folder_its_environment_and_its_input',
  ),
  kit(
    'a program that is not there is not said to be missing',
    PROCESS_RS,
    'command.spawn().map_err(|cause| failure(invocation, cause))',
    'command.spawn().map_err(|cause| Failure::Refused { program: String::new(), cause })',
    XTASK_PROCESS,
    'that_is_not_there_or_has_no_folder',
  ),
]

export const PLANTS = [
  ...DAEMON,
  ...HOLDER,
  ...CONTENTS,
  ...COMMANDS,
  ...BUNDLES,
  ...SPAWNED,
  ...PRODUCT,
]
