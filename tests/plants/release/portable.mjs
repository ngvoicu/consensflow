/**
 * Plants in the portable app's collector (app/src-tauri/src/portable.rs): what it
 * removes of the runtimes an older start unpacked, and what it keeps because a
 * program of the runtime runs: the programs it looks at, and the open it makes of
 * each. The app's tests of it must catch each. A program that runs is stood in
 * for by a read-only file and, on macOS, by a file that may only be appended to,
 * which refuses a write open and grants an append open as Windows does a program
 * that runs: the last plant is caught here by that file alone, and on Windows by
 * the tests with real processes (which Windows runs, and this does not).
 *
 * And in the library that writes the portable exe and reads it back
 * (crates/cf-portable): its footer, the unpacking of its payload, and the packer.
 * Its tests (against an exe the JavaScript packer wrote, and the exes it makes
 * itself) must catch each.
 *
 * And in the two commands over it (tools/xtask, `cargo xtask portable pack` and
 * `portable inspect`): their defaults, the arguments they take and refuse, what
 * they say, and the status they end with. Their own tests, in the module and as
 * processes, must catch each.
 */
import { PORTABLE, PORTABLE_RS } from './kit.mjs'

const plant = (name, from, to, meant) => ({
  name: `portable: ${name}`,
  edits: [[PORTABLE_RS, from, to]],
  runs: [PORTABLE],
  meant,
})

/** The library's tests, which run on any system. Every test binary is run: cargo stops at the first that fails. */
const LIBRARY = ['cargo', 'test', '--offline', '-p', 'cf-portable', '--no-fail-fast']
const SRC = 'crates/cf-portable/src'

const library = (name, file, from, to, meant) => ({
  name: `portable ${name}`,
  edits: [[`${SRC}/${file}`, from, to]],
  runs: [LIBRARY],
  meant,
})

/** The commands' own tests, on a checkout in a temporary folder. */
const COMMANDS = [
  'cargo',
  'test',
  '--offline',
  '-p',
  'xtask',
  '--lib',
  'portable::',
  '--no-fail-fast',
]
/** The commands as processes, as `cargo xtask` starts them. */
const PROCESS = [
  'cargo',
  'test',
  '--offline',
  '-p',
  'xtask',
  '--test',
  'portable',
  '--no-fail-fast',
]
const COMMANDS_RS = 'tools/xtask/src/portable.rs'

const command = (name, from, to, meant, runs = [COMMANDS]) => ({
  name: `portable command: ${name}`,
  edits: [[COMMANDS_RS, from, to]],
  runs,
  meant,
})

/** The test of a runtime kept whole for a program that cannot be written, and the others removed. */
const HELD = 'a_runtime_with_a_program_that_cannot_be_written_stays_whole_and_the_others_go'
/** The test of the open the check makes, on macOS. */
const OPEN = 'a_program_that_refuses_a_write_open_and_grants_an_append_open_is_seen_to_run'
const PROGRAMS = 'const PROGRAMS: [&[&str]; 2] = [&["node.exe"], &["cli", "bin", "cf.exe"]];'
const PROBE = 'program.is_file() && OpenOptions::new().write(true).open(program).is_err()'

/** The tests of the footer: what ends an exe that carries a runtime, and what it names. */
const NO_TAG = 'a_file_that_does_not_end_with_the_tag_carries_nothing'
const IMPOSSIBLE = 'a_footer_that_cannot_be_right_is_an_error'
const TRAILER =
  'the_trailer_names_the_crc_then_the_length_and_the_folder_the_crc_in_eight_hex_digits'
const FOOTER = 'the_footer_is_the_length_as_eight_little_endian_bytes_then_the_tag'
/** The test of an exe the JavaScript packer wrote. */
const JAVASCRIPT = 'the_footer_of_the_exe_the_javascript_packer_wrote_is_found'
/** The tests of the payload unpacked: damaged, and holding what it should not. */
const CHECKSUM = 'a_payload_that_does_not_match_its_crc_or_its_length_is_refused_when_unpacked'
const NOT_PLAIN = 'an_entry_that_is_not_a_plain_file_or_folder_is_refused'
const OUTSIDE = 'an_entry_with_a_path_outside_the_folder_is_refused_and_nothing_is_unpacked_for_it'
const PAX = 'a_global_header_of_pax_and_names_that_begin_with_a_dot_are_unpacked'
const UNPACKED = 'the_runtime_unpacks_as_it_was_packed_and_the_builds_leftovers_are_not_in_it'
/** The tests of the packer. */
const LAYOUT =
  'a_packed_exe_is_the_app_then_its_runtime_as_a_gzip_compressed_tar_then_the_length_and_the_tag'
const REFUSED = 'a_release_folder_missing_a_piece_is_refused_naming_the_piece_and_the_build'

export const PLANTS = [
  plant(
    'a cf.exe that runs does not keep its runtime',
    PROGRAMS,
    'const PROGRAMS: [&[&str]; 1] = [&["node.exe"]];',
    HELD,
  ),
  plant(
    'a node.exe that runs does not keep its runtime',
    PROGRAMS,
    'const PROGRAMS: [&[&str]; 1] = [&["cli", "bin", "cf.exe"]];',
    HELD,
  ),
  plant('no program is taken to run', PROBE, `${PROBE} && false`, HELD),
  plant(
    'every program is taken to run',
    PROBE,
    'program.is_file() && (OpenOptions::new().write(true).open(program).is_err() || true)',
    HELD,
  ),
  plant(
    'a runtime whose program runs is removed all the same',
    'if path != keep && !runs(&path) {',
    'if path != keep {',
    HELD,
  ),
  plant(
    'a program that runs is looked for by an open for appending, which it grants',
    PROBE,
    'program.is_file() && OpenOptions::new().append(true).open(program).is_err()',
    OPEN,
  ),
  plant(
    'the runtime folder is not named by the crc of its payload',
    'root.join(payload.folder(version))',
    'root.join(version)',
    'the_first_start_unpacks_the_runtime_and_later_ones_reuse_it',
  ),

  // The library's footer.
  library(
    'footer: a file that does not end with the tag is taken for one that does',
    'format.rs',
    '        if tag != *TAG {',
    '        if false && tag != *TAG {',
    NO_TAG,
  ),
  library(
    'footer: the length is read big-endian',
    'format.rs',
    'let length = u64::from_le_bytes(length);',
    'let length = u64::from_be_bytes(length);',
    JAVASCRIPT,
  ),
  library(
    'footer: a payload smaller than any gzip stream is taken',
    'format.rs',
    'if length < SMALLEST_GZIP || length > size - footer_bytes {',
    'if length > size - footer_bytes {',
    IMPOSSIBLE,
  ),
  library(
    'footer: a payload longer than the file is taken',
    'format.rs',
    'if length < SMALLEST_GZIP || length > size - footer_bytes {',
    'if length < SMALLEST_GZIP {',
    IMPOSSIBLE,
  ),
  library(
    'footer: the crc is read from the wrong place in the trailer',
    'format.rs',
    'file.seek(SeekFrom::Start(offset + length - TRAILER_BYTES))?;',
    'file.seek(SeekFrom::Start(offset + length - 4))?;',
    TRAILER,
  ),
  library(
    'footer: the folder is named by the crc without its leading zeros',
    'format.rs',
    'format!("{:08x}", self.crc)',
    'format!("{:x}", self.crc)',
    TRAILER,
  ),
  library(
    'footer: the footer is written with another tag',
    'format.rs',
    'footer[8..].copy_from_slice(TAG);',
    'footer[8..].copy_from_slice(b"CFPAYLD2");',
    FOOTER,
  ),

  // The library's unpacking.
  library(
    'archive: the gzip stream is not read to its end, so its crc is not checked',
    'archive.rs',
    '        io::copy(&mut archive.into_inner(), &mut io::sink())?;',
    '        let _ = &archive;',
    CHECKSUM,
  ),
  library(
    'archive: a link is unpacked',
    'archive.rs',
    '            if !(kind.is_file() || kind.is_dir()) {',
    '            if false {',
    NOT_PLAIN,
  ),
  library(
    'archive: an absolute path is unpacked under the folder, as the tar crate does',
    'archive.rs',
    '            if !stays_inside(&entry.path()?) || !entry.unpack_in(into)? {',
    '            if !entry.unpack_in(into)? {',
    OUTSIDE,
  ),
  library(
    'archive: a global header of pax is refused',
    'archive.rs',
    '            if kind.is_pax_global_extensions() {\n                continue;\n            }\n',
    '',
    PAX,
  ),
  library(
    'archive: a name that begins with a dot is refused',
    'archive.rs',
    '        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))',
    '        .all(|part| matches!(part, Component::Normal(_)))',
    PAX,
  ),
  library(
    'archive: the folder to unpack into is not made',
    'archive.rs',
    '        fs::create_dir_all(into).map_err(files("make", into))?;\n',
    '',
    UNPACKED,
  ),

  // The library's packer.
  {
    name: 'portable pack: the license of the console host is not packed',
    edits: [
      [`${SRC}/pack.rs`, 'const RUNTIME: [&str; 4] = [', 'const RUNTIME: [&str; 3] = ['],
      [
        `${SRC}/pack.rs`,
        '    "OpenConsole.exe",\n    "OpenConsole-LICENSE.txt",\n];',
        '    "OpenConsole.exe",\n];',
      ],
    ],
    runs: [LIBRARY],
    meant: LAYOUT,
  },
  library(
    'pack: the pieces of the runtime are not looked for',
    'pack.rs',
    'match required().find(|piece| !release.join(piece).is_file()) {',
    'match required().find(|piece| !release.join(piece).is_file() && false) {',
    REFUSED,
  ),
  library(
    'pack: the app is not looked for',
    'pack.rs',
    '    if !exe.is_file() {',
    '    if false {',
    'the_first_piece_missing_is_the_one_named_and_the_app_comes_first',
  ),
  library(
    'pack: the footer names a byte too many',
    'pack.rs',
    'file.write_all(&footer(payload.len() as u64))',
    'file.write_all(&footer(payload.len() as u64 + 1))',
    LAYOUT,
  ),
  library(
    'pack: the folder the exe goes to is not made',
    'pack.rs',
    '        fs::create_dir_all(folder).map_err(files("make", folder))?;\n',
    '',
    'the_folder_the_exe_is_written_to_is_made_and_an_older_exe_is_replaced',
  ),
  library(
    'pack: a socket in the runtime is taken for a file',
    'pack.rs',
    '    } else if kind.is_file() {',
    '    } else if true {',
    'a_special_file_in_the_runtime_is_refused_and_nothing_is_written',
  ),
  library(
    'pack: the folders of the runtime are not in the tar',
    'pack.rs',
    '        tar.append_dir(name, path).map_err(files("pack", path))?;\n',
    '',
    'a_folder_of_the_runtime_is_packed_whole_and_by_name',
  ),
  library(
    'pack: the refusal does not name the build',
    'error.rs',
    'build first with npm --prefix app run build',
    'build it first',
    REFUSED,
  ),
  library(
    'inspect: a footer that cannot be right is told without the file',
    'format.rs',
    'Err(cause) => Err(files("read", path)(cause.into())),',
    'Err(cause) => Err(cause),',
    'inspect_names_a_file_whose_footer_cannot_be_right',
  ),

  // The commands over it.
  command(
    'pack looks in another folder for the build',
    'const RELEASE: &str = "app/src-tauri/target/release";',
    'const RELEASE: &str = "app/src-tauri/target/debug";',
    'pack_with_no_options_packs_the_builds_release_folder_into_its_bundle_named_by_the_sources',
  ),
  command(
    'pack puts the exe in the release folder, not in its bundle',
    '.unwrap_or_else(|| release.join("bundle").join("portable"));',
    '.unwrap_or_else(|| release.clone());',
    'pack_with_no_options_packs_the_builds_release_folder_into_its_bundle_named_by_the_sources',
  ),
  command(
    'pack puts the exe in the build’s bundle, whichever release folder it was told',
    '.unwrap_or_else(|| release.join("bundle").join("portable"));',
    '.unwrap_or_else(|| context.path(RELEASE).join("bundle").join("portable"));',
    'a_release_folder_alone_has_its_exe_in_its_own_bundle_folder',
  ),
  command(
    'pack names the exe without its architecture',
    'format!("ConsensFlow_{version}_x64-portable.exe")',
    'format!("ConsensFlow_{version}-portable.exe")',
    'pack_with_no_options_packs_the_builds_release_folder_into_its_bundle_named_by_the_sources',
  ),
  command(
    'pack names the exe by a version the sources do not say',
    'None => source_version(&context.root)?,',
    'None => String::from("0.0.0"),',
    'pack_with_no_options_packs_the_builds_release_folder_into_its_bundle_named_by_the_sources',
  ),
  command(
    'pack names the exe by the sources’ version, not the one it was given',
    'Some(version) => version,',
    'Some(_) => source_version(&context.root)?,',
    'a_version_given_names_the_file_and_the_sources_are_not_asked',
  ),
  command(
    'pack does not say where the exe is',
    'writeln!(console.out, "portable: {}", packed.path.display())?;',
    'let _ = &packed;',
    'pack_with_no_options_packs_the_builds_release_folder_into_its_bundle_named_by_the_sources',
  ),
  command(
    'an option joined to its value is not read',
    'Some((flag, value)) => (flag, Some(value)),',
    'Some((flag, _)) => (flag, None::<&str>),',
    'an_option_takes_its_value_after_it_or_joined_to_it_by_the_first_equals_sign',
  ),
  command(
    'a relative path is from where xtask is run, not from the checkout',
    '.map(|value| context.root.join(value))',
    '.map(PathBuf::from)',
    'a_relative_path_is_from_the_checkouts_root',
  ),
  command(
    'an option given twice is taken',
    'if options.insert(name, value).is_some() {',
    'if options.insert(name, value).is_some() && false {',
    'pack_refuses_what_it_does_not_take',
  ),
  command(
    'an option takes the next option for its value',
    ".filter(|word| !word.to_string_lossy().starts_with('-'))",
    '.filter(|_| true)',
    'pack_refuses_what_it_does_not_take',
  ),
  command(
    'pack takes a word that is no option’s',
    'if !given.words.is_empty() {',
    'if false {',
    'pack_refuses_what_it_does_not_take',
  ),
  command(
    'inspect takes more than one file',
    'let [exe] = given.words.as_slice() else {',
    'let [exe, ..] = given.words.as_slice() else {',
    'inspect_refuses_what_it_does_not_take',
  ),
  command(
    'inspect says the crc in decimal',
    'writeln!(console.out, "crc: {}", payload.crc_hex())?;',
    'writeln!(console.out, "crc: {}", payload.crc)?;',
    'inspect_says_the_payload_and_the_crc_and_with_the_version_the_runtime_folder',
  ),
  command(
    'inspect leaves the runtime folder out',
    'writeln!(console.out, "runtime: {}", payload.folder(&version))?;',
    'let _ = &version;',
    'inspect_says_the_payload_and_the_crc_and_with_the_version_the_runtime_folder',
  ),
  command(
    'inspect looks for a relative path where xtask is run',
    'inspect(&context.root.join(exe))?',
    'inspect(&PathBuf::from(exe))?',
    'a_relative_path_is_from_the_checkouts_root_whatever_folder_xtask_is_run_in',
    [PROCESS],
  ),
  command(
    'pack is no command of the table',
    'words: &["portable", "pack"],',
    'words: &["portable", "packed"],',
    'the_two_commands_run_in_rust_and_are_named_pack_and_inspect',
  ),
  {
    name: 'portable command: a refusal of the library does not say it is the portable exe’s',
    edits: [['tools/xtask/src/dispatch.rs', '#[error("portable: {0}")]', '#[error("{0}")]']],
    runs: [PROCESS],
    meant: 'a_file_without_the_footer_ends_with_status_1_naming_it',
  },
]
