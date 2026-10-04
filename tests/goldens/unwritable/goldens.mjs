/**
 * What the roster says when it cannot save, as Node says it: what the Rust
 * `cf-catalog` is held to (`crates/cf-catalog/tests/unwritable.rs`). Each
 * situation is played through the real `src/roster.js`: the folders, files
 * and modes it makes under a temporary root, one call (`setPreferences` or
 * `addAgent`) with `CONSENSFLOW_HOME` somewhere under that root, and the
 * message of the error it throws, which the API answers as `{error:
 * cause.message}` and the page shows.
 *
 * The roster reads the file before it writes it, and refuses one that cannot
 * be read. So on Unix a folder that is a file, a file in the way of the folder
 * and a directory at the file all stop at the read, and the calls of
 * `saveDocument` that would fail on them (`mkdir`, `rename`) are never made.
 * Windows is expected to name a path through a file `ENOENT` (libuv's
 * translation of `ERROR_PATH_NOT_FOUND` and `ERROR_DIRECTORY`), which the
 * roster reads as a missing file, so that there those situations reach the
 * write; the win32 golden records what happens. The situations that call
 * `saveDocument` make its own calls, as a copy of them here, and what they
 * say is what the roster would say if it got there. The copy is held to the
 * roster itself: each situation the roster stops at the write of, the copy
 * must say the same of.
 *
 * The file is deterministic, so the unit suite holds the committed copy equal
 * to what this computes (tests/fs-error-goldens.test.mjs), and `npm run
 * goldens:unwritable` writes it again after a change to `saveDocument` or to
 * Node. One file a platform, `process.platform` naming it. In a message the
 * root is `$ROOT` and the process id `$PID`; in a step, the path is relative
 * to the root, written with `/`, and `$PID` is the process id too.
 *
 * A situation is `{ name, make, home, call, stage, message }`:
 * - `make`: the steps that make it, in order: `{ folder }`, `{ file, text }`
 *   (with the folders above it), and `{ mode, bits }`, which sets the
 *   permissions of a path (octal, as `chmod` has them);
 * - `home`: the path `CONSENSFLOW_HOME` is set to;
 * - `call`: the name of the function and its first argument: `setPreferences`,
 *   `addAgent`, or `saveDocument` with the text it writes;
 * - `stage`: where the call stopped, `read` when it refused the file as
 *   unreadable before it wrote, `write` when it refused the write;
 * - `message`: what the error said.
 *
 * A situation that cannot be made on a platform is left out of that
 * platform's file: `unix` marks a `chmod` of a folder, a permission Windows
 * does not have; `windows` marks a name only Windows refuses. A `chmod` of a
 * file without its owner's write bit is Windows' read-only attribute there.
 */
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { addAgent, setPreferences } from '../../../src/roster.js'

/** The calls of `saveDocument` (src/roster.js) on the file at `path`, as it makes them. */
function saveDocument(text, env) {
  const path = join(env.CONSENSFLOW_HOME, 'agents.json')
  mkdirSync(dirname(path), { recursive: true })
  const temporary = `${path}.${process.pid}.tmp`
  try {
    writeFileSync(temporary, text)
    renameSync(temporary, path)
  } catch (error) {
    rmSync(temporary, { force: true })
    throw error
  }
}

const CALLS = { setPreferences, addAgent, saveDocument }
const PREFERENCE = ['setPreferences', { ownHarnessOnly: true }]
const AGENT = ['addAgent', { name: 'zed', harness: 'codex', model: 'gpt-6-astra' }]
const SAVE = ['saveDocument', '{}\n']
const EMPTY_ROSTER = '{"schemaVersion":1,"agents":[]}\n'
const TEMPORARY = 'agents.json.$PID.tmp'

/** Every situation; `unix` and `windows` mark the ones only that system can make. */
const SITUATIONS = [
  {
    name: 'the folder is read-only',
    unix: true,
    make: [{ folder: 'home' }, { mode: 'home', bits: '555' }],
    home: 'home',
    call: PREFERENCE,
  },
  {
    name: 'the folder is read-only and holds the agents file',
    unix: true,
    make: [
      { file: 'home/agents.json', text: EMPTY_ROSTER },
      { mode: 'home', bits: '555' },
    ],
    home: 'home',
    call: AGENT,
  },
  {
    name: 'a folder above the one to make is read-only',
    unix: true,
    make: [{ folder: 'ro' }, { mode: 'ro', bits: '555' }],
    home: 'ro/x/y',
    call: PREFERENCE,
  },
  {
    name: 'a stale temporary is in a read-only folder',
    unix: true,
    make: [
      { file: `home/${TEMPORARY}`, text: 'stale' },
      { mode: 'home', bits: '555' },
    ],
    home: 'home',
    call: PREFERENCE,
  },
  {
    // Windows will not remove a read-only file: there the removal fails too.
    name: 'a stale read-only temporary is where the temporary goes',
    make: [
      { file: `home/${TEMPORARY}`, text: 'stale' },
      { mode: `home/${TEMPORARY}`, bits: '444' },
    ],
    home: 'home',
    call: PREFERENCE,
  },
  {
    name: 'a folder name Windows refuses',
    windows: true,
    make: [{ folder: 'home' }],
    home: 'home/bad<name',
    call: PREFERENCE,
  },
  {
    name: 'a directory is at the temporary',
    make: [{ folder: `home/${TEMPORARY}` }],
    home: 'home',
    call: PREFERENCE,
  },
  {
    name: 'a directory with a file in it is at the temporary',
    make: [{ file: `home/${TEMPORARY}/inside`, text: 'kept' }],
    home: 'home',
    call: AGENT,
  },
  {
    name: 'a directory with a file in it is at the agents file',
    make: [{ file: 'home/agents.json/inside', text: 'kept' }],
    home: 'home',
    call: AGENT,
  },
  {
    name: 'a file is where the folder should be: the folder itself is a file',
    make: [{ file: 'home', text: 'a file' }],
    home: 'home',
    call: PREFERENCE,
  },
  {
    name: 'a file is where the folder above should be',
    make: [{ file: 'state', text: 'a file' }],
    home: 'state/home',
    call: AGENT,
  },
  {
    name: 'a file is where a folder far above should be',
    make: [{ file: 'state', text: 'a file' }],
    home: 'state/a/b/home',
    call: PREFERENCE,
  },
  {
    name: 'saving where the folder itself is a file',
    make: [{ file: 'home', text: 'a file' }],
    home: 'home',
    call: SAVE,
  },
  {
    name: 'saving where a file is in the way of the folder',
    make: [{ file: 'state', text: 'a file' }],
    home: 'state/a/b',
    call: SAVE,
  },
  {
    name: 'saving over a directory with a file in it',
    make: [{ file: 'home/agents.json/inside', text: 'kept' }],
    home: 'home',
    call: SAVE,
  },
  {
    name: 'saving in a folder that may not be entered',
    unix: true,
    make: [{ folder: 'home' }, { mode: 'home', bits: '000' }],
    home: 'home',
    call: SAVE,
  },
]

/** A path of a step under `root`, as the platform writes it, with the process id in it. */
const under = (root, relative) =>
  join(root, ...relative.replaceAll('$PID', String(process.pid)).split('/'))

function make(root, steps) {
  for (const step of steps) {
    if (step.folder !== undefined) {
      mkdirSync(under(root, step.folder), { recursive: true })
    } else if (step.file !== undefined) {
      const path = under(root, step.file)
      mkdirSync(dirname(path), { recursive: true })
      writeFileSync(path, step.text)
    } else {
      chmodSync(under(root, step.mode), Number.parseInt(step.bits, 8))
    }
  }
}

/** What `situation` throws, made under a root of its own, with the root and the process id left out. */
function said(situation) {
  const root = mkdtempSync(join(tmpdir(), 'cf-unwritable-'))
  try {
    make(root, situation.make)
    const [name, argument] = situation.call
    try {
      CALLS[name](argument, { CONSENSFLOW_HOME: under(root, situation.home) })
    } catch (error) {
      return error.message
        .replaceAll(`.${process.pid}.tmp`, () => '.$PID.tmp')
        .replaceAll(root, () => '$ROOT')
    }
    throw new Error(
      `"${situation.name}" did not fail on ${process.platform}: a permission binds every user but root, which is not to run this`,
    )
  } finally {
    // A folder with no permission cannot be removed with what is in it, nor
    // can a read-only file on Windows; a file the call removed is gone.
    for (const step of situation.make.filter((step) => step.mode !== undefined)) {
      if (existsSync(under(root, step.mode))) chmodSync(under(root, step.mode), 0o755)
    }
    rmSync(root, { recursive: true, force: true })
  }
}

/** `situation` answered with where it stopped and what it said. */
function play(situation) {
  const message = said(situation)
  const stage = message.startsWith('Your agents file ') ? 'read' : 'write'
  const [name] = situation.call
  if (stage === 'write' && name !== 'saveDocument') {
    const copy = said({ ...situation, call: SAVE })
    if (copy !== message) {
      throw new Error(
        `"${situation.name}": the roster says ${message}, the copy of saveDocument ${copy}`,
      )
    }
  }
  return {
    name: situation.name,
    make: situation.make,
    home: situation.home,
    call: situation.call,
    stage,
    message,
  }
}

/** Every golden, by its path under crates/cf-catalog. */
export function unwritableGoldens() {
  const windows = process.platform === 'win32'
  const here = SITUATIONS.filter((situation) => (windows ? !situation.unix : !situation.windows))
  // A situation a line: diffs read it by situation.
  const lines = here.map((situation) => `    ${JSON.stringify(play(situation))}`).join(',\n')
  return {
    files: {
      [`tests/goldens/unwritable/${process.platform}.json`]: `{\n  "situations": [\n${lines}\n  ]\n}\n`,
    },
  }
}

/** Writes every golden into `crate`. */
export function writeUnwritableGoldens(crate) {
  const { files } = unwritableGoldens()
  for (const [relative, text] of Object.entries(files)) {
    const path = join(crate, ...relative.split('/'))
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, text)
  }
  return { written: Object.keys(files).length }
}
