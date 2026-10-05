/**
 * Which harnesses are installed here and where: on PATH first, then in the
 * places each installs itself and the places any may land in, by the names
 * this system gives a program. Each scenario is one look at a home, a PATH and
 * an environment, answered as the page and the screens read it: every harness
 * known, the ones missing, the ones detected and where each is.
 */
import { exe, onPath, WINDOWS } from './kit.mjs'

const IDS = ['devin', 'claude', 'codex', 'opencode', 'pi']

/** A scenario that detects. */
const detects = (name, files, env = {}) => ({
  name,
  files,
  env,
  steps: [{ op: 'detect' }],
})

/** A file at exactly `path`, the system may run. */
const runnable = (path) => ({ path, executable: true })

/** A file a system with an executable bit may not run. */
const plain = (path) => ({ path, text: '', mode: 0o644 })

export function detections() {
  return [
    detects('nothing is installed: every harness is missing', []),
    detects(
      'all five are on PATH',
      IDS.map((id) => exe(`$ROOT/bin/${id}`)),
    ),
    detects('each is found where it installs itself, with PATH holding nothing', [
      exe('$ROOT/home/.local/bin/devin'),
      exe('$ROOT/home/.claude/local/claude'),
      exe('$ROOT/home/.codex/bin/codex'),
      exe('$ROOT/home/.opencode/bin/opencode'),
      exe('$ROOT/home/.pi/bin/pi'),
    ]),
    detects('the places any CLI may land in: bun, npm global and volta', [
      exe('$ROOT/home/.bun/bin/codex'),
      exe('$ROOT/home/.npm-global/bin/claude'),
      exe('$ROOT/home/.volta/bin/pi'),
      exe('$ROOT/home/.local/bin/opencode'),
      exe('$ROOT/home/.bun/bin/devin'),
    ]),
    detects('PATH comes first, then the harness s own places, then the common ones', [
      exe('$ROOT/bin/codex'),
      exe('$ROOT/home/.codex/bin/codex'),
      exe('$ROOT/home/.local/bin/claude'),
      exe('$ROOT/home/.volta/bin/claude'),
      exe('$ROOT/home/.volta/bin/pi'),
      exe('$ROOT/home/.npm-global/bin/pi'),
      exe('$ROOT/home/.bun/bin/opencode'),
      exe('$ROOT/home/.volta/bin/opencode'),
    ]),
    detects("another harness's place is not this one's, nor is another's name", [
      exe('$ROOT/home/.codex/bin/pi'),
      exe('$ROOT/home/.pi/bin/codex'),
      exe('$ROOT/home/.claude/local/devin'),
    ]),
    detects(
      'the first folder of PATH that has it is the one',
      [exe('$ROOT/a/codex'), exe('$ROOT/b/codex'), exe('$ROOT/b/pi')],
      { PATH: onPath('$ROOT/nowhere', '$ROOT/a', '$ROOT/b') },
    ),
    detects('an empty PATH finds only the places', [exe('$ROOT/home/.codex/bin/codex')], {
      PATH: '',
    }),
    detects('PATH entries that are empty are skipped', [exe('$ROOT/bin/codex')], {
      PATH: onPath('', '$ROOT/bin', ''),
    }),
    detects(
      'a folder named like the CLI is not a CLI',
      [{ dir: '$ROOT/a/codex' }, exe('$ROOT/b/codex'), { dir: '$ROOT/b/pi' }],
      { PATH: onPath('$ROOT/a', '$ROOT/b') },
    ),
    detects(
      'the home is the profile when there is no home, as on Windows',
      [exe('$ROOT/home/.codex/bin/codex')],
      { HOME: undefined, USERPROFILE: '$ROOT/home' },
    ),
    detects(
      "an environment that says it is Windows' is looked at for the names Windows gives a program",
      [
        runnable('$ROOT/bin/codex.exe'),
        runnable('$ROOT/bin/claude.cmd'),
        runnable('$ROOT/bin/pi.bat'),
        runnable('$ROOT/bin/devin.com'),
        runnable('$ROOT/bin/opencode.js'),
      ],
      { OS: 'Windows_NT' },
    ),
    detects('a bare name is no program where the system is Windows', [runnable('$ROOT/bin/pi')], {
      OS: 'Windows_NT',
    }),
    detects(
      'PATHEXT decides the names and their order, and what is not startable is no name',
      [
        runnable('$ROOT/a/codex.cmd'),
        runnable('$ROOT/a/codex.exe'),
        runnable('$ROOT/b/claude.js'),
        runnable('$ROOT/b/claude.ps1'),
        runnable('$ROOT/b/pi.exe'),
      ],
      { OS: 'Windows_NT', PATH: onPath('$ROOT/a', '$ROOT/b'), PATHEXT: '.PS1;.CMD;.js;.EXE' },
    ),
    detects('an empty PATHEXT names no program', [runnable('$ROOT/bin/codex.exe')], {
      OS: 'Windows_NT',
      PATHEXT: '',
    }),
    ...(WINDOWS
      ? []
      : [
          detects(
            'a file this user may not run is passed over for the next folder',
            [plain('$ROOT/a/codex'), exe('$ROOT/b/codex'), plain('$ROOT/a/pi')],
            { PATH: onPath('$ROOT/a', '$ROOT/b') },
          ),
          detects('a link to a file counts and a dangling link, or one to a folder, does not', [
            { path: '$ROOT/target/codex', executable: true },
            { path: '$ROOT/bin/codex', link: '$ROOT/target/codex' },
            { path: '$ROOT/bin/claude', link: '$ROOT/target/missing' },
            { path: '$ROOT/bin/pi', link: '$ROOT/target' },
          ]),
        ]),
  ]
}
