/**
 * Plants in what builds and stages the app's `cf` and its console host
 * (tools/xtask, `cargo xtask build-cf`, `stage` and `conpty`): the pinned
 * package's checks, the replacing of the `cf` in bin/ that Windows will not
 * delete while it runs, and the files the bundle is staged with. The tests in
 * the sidecar module must catch each.
 */

const cargo = (...args) => ['cargo', 'test', '--offline', ...args]

/** The sidecar commands' own tests, on a stand-in for the system: programs, platform, clock, deleting. */
const SIDECAR = cargo('-p', 'xtask', '--lib', 'sidecar::')

const SIDECAR_RS = 'tools/xtask/src/sidecar.rs'
const BUILD_CF_RS = 'tools/xtask/src/sidecar/build_cf.rs'
const STAGE_RS = 'tools/xtask/src/sidecar/stage.rs'
const CONPTY_RS = 'tools/xtask/src/sidecar/conpty.rs'

const PIN = '02b07b349af66d801159bdf9e440d4a1ce78bb951f37fc8609731665afdae7ee'
const ASIDE = 'fs::rename(placed, &aside).map_err(files("set aside", placed))'
const STAGED_CF = 'fs::copy(&cf, &staged).map_err(files("copy the cf to", &staged))?;'
const OPEN_CONSOLE = `    (
        "build/native/runtimes/x64/OpenConsole.exe",
        "OpenConsole.exe",
    ),
`

const plant = (name, file, from, to, meant) => ({
  name: `sidecar: ${name}`,
  edits: [[file, from, to]],
  runs: [SIDECAR],
  meant,
})

export const PLANTS = [
  plant(
    'a fetched package is used without its hash being checked',
    CONPTY_RS,
    'if actual != package.sha256 {',
    'if false {',
    'refuses_a_fetched_package_that_is_not_the_pinned_one_and_deletes_it',
  ),
  plant(
    'a kept package is used without its hash being checked',
    CONPTY_RS,
    'if sha256(&bytes) == package.sha256 {',
    'if true {',
    'fetches_again_a_kept_package_that_is_not_the_pinned_one',
  ),
  plant(
    'a cf that cannot be deleted is not set aside',
    BUILD_CF_RS,
    ASIDE,
    'Ok(())',
    'is_set_aside_and_the_new_one_put_in_its_place',
  ),
  plant(
    'the cf is left out of the bundle',
    STAGE_RS,
    STAGED_CF,
    'let _ = &cf;',
    'stages_the_cf_as_the_bundles_cli_bin_cf_and_nothing_else_off_windows',
  ),
  {
    name: 'sidecar: OpenConsole.exe is left out of the bundle',
    edits: [
      [CONPTY_RS, 'const FILES: [(&str, &str); 2] = [', 'const FILES: [(&str, &str); 1] = ['],
      [CONPTY_RS, OPEN_CONSOLE, ''],
    ],
    runs: [SIDECAR],
    meant: 'stages_the_console_host_beside_it_on_windows_and_says_each_file',
  },
  plant(
    'the console host is not staged on Windows',
    STAGE_RS,
    'if system.platform() == Platform::Windows {',
    'if false {',
    'stages_the_console_host_beside_it_on_windows_and_says_each_file',
  ),
  plant(
    'the bundle keeps what an older staging left in cli/',
    STAGE_RS,
    '    clear(&cli)?;',
    '    let _ = &cli;',
    'replaces_what_was_staged_in_the_cli_folder_and_leaves_the_other_folders',
  ),
  plant(
    'the bundle takes the cf cargo built and not the signed copy in bin/',
    STAGE_RS,
    STAGED_CF,
    `let _ = &cf;
    let built = context.path("app/src-tauri/target/release").join(system.platform().cf());
    fs::copy(&built, &staged).map_err(files("copy the cf to", &staged))?;`,
    'stages_the_copy_in_bin_after_it_was_signed_and_not_the_one_cargo_built',
  ),
  plant(
    'the copies set aside earlier are never taken away',
    BUILD_CF_RS,
    'let _ = system.remove_file(&entry.path());',
    'let _ = &entry;',
    'replaces_the_cf_there_and_takes_away_the_copies_set_aside_earlier',
  ),
  plant(
    'a build that left no cf is found only after bin/ was cleared',
    BUILD_CF_RS,
    'if !built.is_file() {',
    'if false {',
    'a_build_that_left_no_cf_is_said_before_the_cf_in_bin_is_touched',
  ),
  plant(
    'cargo is not told --offline',
    BUILD_CF_RS,
    '.args(offline.then_some("--offline"))',
    '.args(None::<&str>)',
    'is_given_offline_in_the_place_cargo_takes_it_when_it_is_asked_for',
  ),
  plant(
    'cargo is not held to the lockfile',
    BUILD_CF_RS,
    '.args(["build", "--release", "--locked"])',
    '.args(["build", "--release"])',
    'is_cargos_release_build_of_the_cf_binary_on_the_lockfile_from_the_root',
  ),
  plant(
    'the cf is not signed on macOS',
    BUILD_CF_RS,
    'if platform == Platform::MacOs {',
    'if false {',
    'only_on_macos_is_the_copy_signed_ad_hoc_once_it_is_in_place',
  ),
  plant(
    'a program that ends with a status is taken for done',
    SIDECAR_RS,
    '        0 => Ok(()),',
    '        _ => Ok(()),',
    'a_build_that_fails_ends_the_command_with_its_status_and_leaves_bin_alone',
  ),
  plant(
    'a closed pipe is a failure of the command and not an end like the help’s',
    SIDECAR_RS,
    '        Err(Error::Said(cause)) => Err(Failure::Io(cause)),\n',
    '',
    'what_could_not_be_said_is_an_input_output_failure_so_a_closed_pipe_ends_quietly',
  ),
  plant(
    'the cf is named cf on Windows',
    SIDECAR_RS,
    'Self::Windows => "cf.exe",',
    'Self::Windows => "cf",',
    'the_cf_is_a_program_of_the_system_it_is_built_on',
  ),
  plant(
    'the console host is taken from the arm64 folder of the package',
    CONPTY_RS,
    '("runtimes/win-x64/native/conpty.dll", "conpty.dll"),',
    '("runtimes/win-arm64/native/conpty.dll", "conpty.dll"),',
    'fetches_a_package_that_is_not_kept_and_puts_its_two_files_in_the_folder',
  ),
  plant(
    'the pin is another package',
    CONPTY_RS,
    `sha256: "${PIN}",`,
    `sha256: "1${PIN.slice(1)}",`,
    'the_pin_is_the_package_microsoft_published',
  ),
  plant(
    'a relative --into is from where xtask was run',
    CONPTY_RS,
    '.map(|dir| context.root.join(dir))',
    '.map(PathBuf::from)',
    'into_is_the_one_folder_conpty_takes_and_a_relative_one_is_from_the_root',
  ),
]
