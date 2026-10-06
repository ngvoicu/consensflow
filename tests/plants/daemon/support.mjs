/**
 * What the three players of Node's recordings share (`crates/cf-daemon/tests/support`):
 * a bug in it is one the players must catch, each that uses what it breaks, so each
 * bug is one plant for each of them.
 *
 * What no plant can show is what no recorded trace can see: the `«now»` a world's
 * file is written with (the recorder masks the stamps of a roster, so nothing
 * compares them: `players.mjs` holds the mask itself), the queues and the events an operation
 * held while a stand-in made a call of its own (the recorder refuses an operation
 * that reads the clock itself and through a stand-in too, and none logs an event
 * before one), and what only Windows reads (the shims of the programs, `PATHEXT`).
 */
import { lines } from './kit.mjs'

const SUPPORT = 'crates/cf-daemon/tests/support'
const player = (name) => ['-p', 'cf-daemon', '--test', name]

/** The test of each player that holds what a bug breaks, by player. */
const API = 'every_trace_of_the_api_is_answered_as_node_answered_it'
const CF = 'every_run_of_cf_against_the_api_prints_what_it_printed_against_node'
const PAGE = 'every_page_trace_is_answered_as_node_answered'
const SCREENS = 'corners_screens_001'
const SCREENS_TRACES = 'every_trace_of_the_screens_there_is_has_a_test_and_no_test_is_for_none'

/** Each bug: where, what it is, the text it replaces, and who must notice, with what. */
const BUGS = [
  {
    name: 'the dump lists the schema backwards',
    file: 'compare.rs',
    edits: [['AND sql IS NOT NULL ORDER BY name")', 'AND sql IS NOT NULL ORDER BY name DESC")']],
    players: { api: API, page: PAGE, screens: SCREENS },
  },
  {
    name: 'the dump lists the rows backwards',
    file: 'compare.rs',
    edits: [['FROM \\"{name}\\" ORDER BY rowid"))', 'FROM \\"{name}\\" ORDER BY rowid DESC"))']],
    players: { api: API, page: PAGE, screens: SCREENS },
  },
  {
    name: 'the path after «root» is lost',
    file: 'world.rs',
    edits: [
      [
        '.fold(self.path().to_path_buf(), |path, part| path.join(part));',
        '.fold(self.path().to_path_buf(), |path, _part| path);',
      ],
    ],
    players: { page: PAGE, screens: SCREENS },
  },
  {
    name: 'a program on the path is not executable',
    file: 'world.rs',
    edits: [['fs::Permissions::from_mode(0o755)', 'fs::Permissions::from_mode(0o644)']],
    players: { page: PAGE, screens: SCREENS },
  },
  {
    name: 'a task is assigned the other way round',
    file: 'ledger.rs',
    edits: [
      [
        'encode(ledger.assign_task(id(), integer(arg(args, 1)), integer(arg(args, 2))))',
        'encode(ledger.assign_task(id(), integer(arg(args, 2)), integer(arg(args, 1))))',
      ],
    ],
    players: { api: CF, page: PAGE },
  },
  {
    name: 'a session name is drawn in capitals',
    file: 'ledger.rs',
    edits: [
      [
        '.map(|name| name.as_str().expect("a name").to_owned()),',
        '.map(|name| name.as_str().expect("a name").to_uppercase()),',
      ],
    ],
    players: { api: CF, page: PAGE },
  },
  {
    name: 'the bearer is sent in a header nobody reads',
    file: 'front.rs',
    edits: [
      [
        'head.push_str(&format!("Authorization: {authorization}\\r\\n"));',
        'head.push_str(&format!("X-Authorization: {authorization}\\r\\n"));',
      ],
    ],
    players: { api: API, screens: SCREENS },
  },
  {
    name: 'the wake-ups are counted one too many',
    file: 'daemon.rs',
    edits: [['total - self.counted.replace(total)', 'total - self.counted.replace(total) + 1']],
    players: { api: API, page: PAGE, screens: SCREENS },
  },
  {
    name: 'the last trace of a suite is not found',
    file: 'trace.rs',
    edits: [
      [
        lines('    found.sort();', '    found'),
        lines('    found.sort();', '    found.pop();', '    found'),
      ],
    ],
    players: { api: API, page: PAGE, screens: SCREENS_TRACES },
  },
]

export const PLANTS = BUGS.flatMap(({ name, file, edits, players }) =>
  Object.entries(players).map(([who, meant]) => ({
    name: `support: ${name} (${who})`,
    edits: edits.map(([from, to]) => [`${SUPPORT}/${file}`, from, to]),
    runs: [player(who)],
    meant,
  })),
)
