/**
 * Plants in the portable app's collector (app/src-tauri/src/portable.rs): what it
 * removes of the runtimes an older start unpacked, and what it keeps because a
 * program of the runtime runs: the programs it looks at, and the open it makes of
 * each. The app's tests of it must catch each. A program that runs is stood in
 * for by a read-only file and, on macOS, by a file that may only be appended to,
 * which refuses a write open and grants an append open as Windows does a program
 * that runs: the last plant is caught here by that file alone, and on Windows by
 * the tests with real processes (which Windows runs, and this does not).
 */
import { PORTABLE, PORTABLE_RS } from './kit.mjs'

const plant = (name, from, to, meant) => ({
  name: `portable: ${name}`,
  edits: [[PORTABLE_RS, from, to]],
  runs: [PORTABLE],
  meant,
})

/** The test of a runtime kept whole for a program that cannot be written, and the others removed. */
const HELD = 'a_runtime_with_a_program_that_cannot_be_written_stays_whole_and_the_others_go'
/** The test of the open the check makes, on macOS. */
const OPEN = 'a_program_that_refuses_a_write_open_and_grants_an_append_open_is_seen_to_run'
const PROGRAMS = 'const PROGRAMS: [&[&str]; 2] = [&["node.exe"], &["cli", "bin", "cf.exe"]];'
const PROBE = 'program.is_file() && OpenOptions::new().write(true).open(program).is_err()'

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
]
