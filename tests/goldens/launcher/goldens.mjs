/**
 * What Node's terminal command and its stale-hook report say, as Node says it:
 * what the Rust `cf-launcher` and `cf_harness::claude::stale_hooks` are held
 * to (`crates/cf-launcher/tests/goldens.rs`,
 * `crates/cf-harness/tests/host_payloads.rs`). The real `src/terminal.js` and
 * `src/host-payloads.js` play every case, over a throwaway home.
 *
 * The files are deterministic, so the unit suite holds the committed copies
 * equal to what this computes (tests/launcher-goldens.test.mjs), and `npm run
 * goldens:launcher` writes them again after a change to either module or to
 * Node. None is keyed by a platform: what a platform cannot make is left out
 * of what it records, and the Rust replay leaves the same out.
 *
 * - `crates/cf-launcher/tests/goldens/installs-cmd.json`: `installTerminalCommand`
 *   in the form of cmd.exe (`OS=Windows_NT`, which any system can make, and
 *   which Windows makes whatever its environment says), and `installs-sh.json`:
 *   in the form of `sh`, which Windows cannot, and with the cases whose words
 *   are a POSIX system's own. Each case is `{ name, pin, path, place, before,
 *   after, installed, error }`:
 *   - `pin`: whether the environment names a `CONSENSFLOW_HOME`;
 *   - `path`: where the folder is on `PATH`: `on`, `off`, `among` others, or
 *     `near`, with folders that begin or end like it and none that is it;
 *   - `place`: the one candidate: `bin` (a folder, there or not), `blocked`
 *     (under a file) or `file` (a file where the folder goes);
 *   - `before`: what the folder held, `{ name, text, mode }` or `{ name,
 *     directory }`, by name without `.cmd`;
 *   - `after`: what it holds, the same, with `executable` where there are
 *     modes, and the `text` as Node wrote it, with `<runtime>`, `<cli>` (the
 *     `cf.mjs`) and `<home>` for what is the machine's; none when there is
 *     no folder;
 *   - `installed`: what the call answered, `{ name, onPath }` or null;
 *   - `error`: what it threw, with the root of the home as `$ROOT` and every
 *     separator `/`, or null.
 * - `crates/cf-launcher/tests/goldens/readings.json`: `terminalRuntime` over
 *   texts a command may hold: what it read, `{ runtime, entry }`, or null; and
 *   `dirnames`, what `path.posix.dirname` says of each path, which is how a
 *   bundle is found from the entry.
 * - `crates/cf-harness/tests/goldens/claude/stale-hooks.json`:
 *   `staleClaudeHooks` over settings files, as `text`, as the `hex` of bytes
 *   that are no text, or neither for none: the `events` it named, or what it
 *   `throws`.
 *
 * Characters that are only seen by their code (a no-break space, a byte
 * order mark, a backslash) are made here from it, so no editor and no tool
 * between this file and Node can turn one into another.
 */
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, delimiter, dirname, join, posix } from 'node:path'
import { fileURLToPath } from 'node:url'
import { staleClaudeHooks } from '../../../src/host-payloads.js'
import { installTerminalCommand, terminalRuntime } from '../../../src/terminal.js'

const REPO = fileURLToPath(new URL('../../..', import.meta.url))
const CLI = join(REPO, 'bin', 'cf.mjs')
const WINDOWS = process.platform === 'win32'

const BACKSLASH = String.fromCharCode(0x5c)
const NO_BREAK_SPACE = String.fromCharCode(0xa0)
const BYTE_ORDER_MARK = String.fromCharCode(0xfeff)
const NEXT_LINE = String.fromCharCode(0x85)

/** A command of ours, an older build's: marked, and naming what is not there. */
const OURS =
  '#!/bin/sh\n# Installed by ConsensFlow. an older build\nexec "/old/node" "/old/cf.mjs" "$@"\n'
/** A command of someone else's. */
const THEIRS = "someone else's command\n"

/** Every case of `installTerminalCommand`; `own` ones say what only a POSIX system says. */
const INSTALLS = [
  { name: 'a first install, from a copy that names no home of its own' },
  { name: 'a first install, from a copy with a home of its own', pin: true },
  { name: 'the folder is on PATH', pin: true, path: 'on' },
  { name: 'the folder is not on PATH', path: 'off' },
  { name: 'the folder is on PATH among others', path: 'among' },
  { name: 'folders that only begin or end like it are not it', path: 'near' },
  {
    name: 'a command of ours is replaced, by either name',
    pin: true,
    before: [
      { name: 'consensflow', text: OURS },
      { name: 'cf', text: OURS },
    ],
  },
  {
    name: "someone else's cf is left alone and the other name is made",
    before: [{ name: 'cf', text: THEIRS }],
  },
  {
    name: "someone else's consensflow is left alone, and the command is not ours to report",
    before: [{ name: 'consensflow', text: THEIRS }],
  },
  {
    name: "both names someone else's: nothing is written",
    before: [
      { name: 'consensflow', text: THEIRS },
      { name: 'cf', text: THEIRS },
    ],
  },
  {
    name: 'a command that holds the mark in a line of its own is ours all the same',
    before: [{ name: 'cf', text: '# not a launcher\n# Installed by ConsensFlow\necho hi\n' }],
  },
  { name: 'a folder that cannot be made says what was tried', place: 'blocked' },
  {
    name: 'a file where the folder goes is where the write fails',
    place: 'file',
    own: true,
  },
  {
    name: 'a name that is a folder cannot be read',
    before: [{ name: 'cf', directory: true }],
    own: true,
  },
  {
    name: 'a command of ours that may not be run is made so',
    before: [{ name: 'cf', text: OURS, mode: 0o600 }],
    own: true,
  },
]

/** Texts a command may hold, and what `terminalRuntime` reads in them. */
const READINGS = [
  [
    'an old launcher for sh',
    '#!/bin/sh\n# Installed by ConsensFlow. x\nexec "/App/MacOS/node" "/App/Resources/cli/bin/cf.mjs" "$@"\n',
  ],
  [
    'one that pins a home',
    '#!/bin/sh\n# Installed by ConsensFlow. x\nexport CONSENSFLOW_HOME="/Users/me/.cf"\nexec "/n" "/x/bin/cf.mjs" "$@"\n',
  ],
  [
    'an old launcher for cmd',
    `@echo off\r\nREM Installed by ConsensFlow. x\r\n"C:${BACKSLASH}App${BACKSLASH}node.exe" "C:${BACKSLASH}App${BACKSLASH}cli${BACKSLASH}bin${BACKSLASH}cf.mjs" %*\r\n`,
  ],
  [
    'one for cmd that pins a home',
    `@echo off\r\nREM Installed by ConsensFlow. x\r\nsetlocal\r\nset "CONSENSFLOW_HOME=C:${BACKSLASH}h"\r\n"C:${BACKSLASH}n.exe" "C:${BACKSLASH}x${BACKSLASH}cf.mjs" %*\r\n`,
  ],
  [
    'a runtime with a space in its path',
    '# Installed by ConsensFlow\nexec "/a b/node" "/c d/cf.mjs" "$@"\n',
  ],
  [
    'white space of any kind between the two',
    '# Installed by ConsensFlow\n"/n"\n\t \r\n"/x/cf.mjs"',
  ],
  [
    'a no-break space between the two',
    `# Installed by ConsensFlow\n"/n"${NO_BREAK_SPACE}"/x/cf.mjs"`,
  ],
  [
    'a byte order mark between the two',
    `# Installed by ConsensFlow\n"/n"${BYTE_ORDER_MARK}"/x/cf.mjs"`,
  ],
  [
    'a next-line character between the two',
    `# Installed by ConsensFlow\n"/n"${NEXT_LINE}"/x/cf.mjs"`,
  ],
  ['nothing between the two', '# Installed by ConsensFlow\n"/n""/x/cf.mjs"'],
  ['a cf.mjs with nothing before it', '# Installed by ConsensFlow\n"/n" "cf.mjs"'],
  ['a cf.mjs with something before it', '# Installed by ConsensFlow\n"/n" "/cf.mjs"'],
  ['a path that ends in more than cf.mjs', '# Installed by ConsensFlow\n"/n" "/x/cf.mjs.bak"'],
  ['an empty runtime', '# Installed by ConsensFlow\n"" "/x/cf.mjs"'],
  [
    'the first pair that fits, from the left',
    '# Installed by ConsensFlow\n"a" "b" "/n" "/x/cf.mjs" "/m" "/y/cf.mjs"',
  ],
  [
    'a native cf, which is no shape of Node',
    '#!/bin/sh\n# Installed by ConsensFlow. x\nexec "/App/cli/bin/cf" "$@"\n',
  ],
  ['a command that is not ours', 'exec "/n" "/x/cf.mjs" "$@"\n'],
  ['a mark and nothing else', '# Installed by ConsensFlow\n'],
  ['an empty file', ''],
  ['quotes that never pair', '# Installed by ConsensFlow\n"/n "/x/cf.mjs'],
]

/** Paths for `path.posix.dirname`, which finds a bundle from the entry. */
const DIRNAMES = [
  '/a/b/c/d/cf.mjs',
  'cf.mjs',
  '/cf.mjs',
  '/',
  '',
  '/a/b/',
  '//a',
  'a/b',
  'a//b',
  'a/',
  '//',
  '///a',
  './a',
  'a',
  '/a',
  '/a/',
  'a/b/c//',
  '/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf',
]

const HOOK = { hooks: [{ type: 'command', command: 'node /x/consensflow/hook.mjs' }] }
const OTHER = { hooks: [{ type: 'command', command: 'echo hello' }] }
const json = (value) => JSON.stringify(value)
/** Bytes that are no text: a string is text, a number a byte. */
const bytes = (...parts) => ({
  hex: Buffer.concat(
    parts.map((part) => (typeof part === 'number' ? Buffer.from([part]) : Buffer.from(part))),
  ).toString('hex'),
})

/** Settings files, as text, as bytes, or `null` for none. */
const SETTINGS = [
  ['no file at all', null],
  ['an empty file', ''],
  ['an empty object', '{}'],
  [
    'the events still holding a hook of ours, and not the others',
    JSON.stringify({ model: 'opus', hooks: { SessionStart: [HOOK], Stop: [OTHER] } }, null, 2),
  ],
  ['no hooks', json({ model: 'opus' })],
  ['JSON that is not JSON', '{"hooks": {"A": [{"c": "consensflow"}],}}'],
  ['text after the JSON', `${json({ hooks: { A: [{ c: 'consensflow' }] } })} x`],
  [
    'a byte order mark before the JSON',
    `${BYTE_ORDER_MARK}${json({ hooks: { A: [{ c: 'consensflow' }] } })}`,
  ],
  ['an array', '[{"hooks": {"A": [{"c": "consensflow"}]}}]'],
  ['a number', '5'],
  ['a text', '"consensflow"'],
  ['a flag', 'true'],
  ['the JSON null, which Node reads as an object and throws on', 'null'],
  ['hooks that is null', '{"hooks": null}'],
  ['hooks that is a text', '{"hooks": "consensflow"}'],
  ['hooks that is a number', '{"hooks": 7}'],
  ['hooks that is a flag', '{"hooks": true}'],
  [
    'hooks that is a list: each item is an event named by its place',
    '{"hooks": [[{"c": "x"}], [{"c": "consensflow"}], [{"c": "consensflow"}]]}',
  ],
  ['hooks that is a list of what is no list', '{"hooks": [{"c": "consensflow"}, "consensflow"]}'],
  [
    'an event that is no list',
    json({ hooks: { A: { c: 'consensflow' }, B: 'consensflow', C: 3, D: null } }),
  ],
  ['an empty list', json({ hooks: { A: [] } })],
  ['the name only in a key', json({ hooks: { A: [{ consensflow: 1 }] } })],
  ['the name deep in a list', json({ hooks: { A: [[[['consensflow']]]] } })],
  ['the name as a list item that is a text', json({ hooks: { A: [null, 5, 'consensflow'] } })],
  [
    'the name in another case',
    json({ hooks: { A: [{ c: 'ConsensFlow' }, { c: 'CONSENSFLOW' }] } }),
  ],
  ['the name cut in two', json({ hooks: { A: [{ c: 'consens', d: 'flow' }] } })],
  ['the name as part of a longer word', json({ hooks: { A: [{ c: 'xconsensflowx' }] } })],
  ['the name spelled with an escape', `{"hooks": {"A": [{"c": "consens${BACKSLASH}u0066low"}]}}`],
  [
    'the events as JavaScript lists them: the keys that are indexes first, ascending',
    `{"hooks": {"b": [${json(HOOK)}], "10": [${json(HOOK)}], "a": [${json(HOOK)}], "2": [${json(HOOK)}], "-1": [${json(HOOK)}], "01": [${json(HOOK)}]}}`,
  ],
  [
    'a key that is written twice keeps its place and takes the last value',
    '{"hooks": {"A": [{"c": "x"}], "B": [{"c": "consensflow"}], "A": [{"c": "consensflow"}]}}',
  ],
  ['a key that is __proto__', '{"hooks": {"__proto__": [{"c": "consensflow"}]}}'],
  [
    'an event named outside ASCII',
    json({ hooks: { 'événement 日本 😀': [{ c: 'consensflow' }] } }),
  ],
  [
    'an event named with a quote and a backslash',
    json({ hooks: { [`a"b${BACKSLASH}c`]: [{ c: 'consensflow' }] } }),
  ],
  [
    'a number past 2^53 in the entry',
    '{"hooks": {"A": [{"n": 123456789012345678901234567890, "c": "consensflow"}]}}',
  ],
  ['white space all through', '\n\t{ "hooks" :\r\n { "A" : [ { "c" : "consensflow" } ] } }\n'],
  [
    'a byte that is no UTF-8 in an event name',
    bytes('{"hooks": {"A', 0xff, '": [{"c": "consensflow"}]}}'),
  ],
  [
    'a byte that is no UTF-8 in the name itself',
    bytes('{"hooks": {"A": [{"c": "consens', 0xff, 'flow"}]}}'),
  ],
  [
    'a byte that is no UTF-8 beside the name',
    bytes('{"hooks": {"A": [{"c": "', 0xc3, 'consensflow"}]}}'),
  ],
  [
    'a lone surrogate escape beside the name',
    `{"hooks": {"A": [{"c": "${BACKSLASH}ud800 consensflow"}]}}`,
  ],
]

/** One line to a case: a file a diff can be read in. */
function lines(cases) {
  return cases.map((each) => `    ${JSON.stringify(each)}`).join(',\n')
}

/** Text with what is the machine's taken out: the `cf.mjs`, the runtime and the home. */
function mask(text, home) {
  const masked = text.replaceAll(CLI, '<cli>').replaceAll(process.execPath, '<runtime>')
  return home ? masked.replaceAll(home, '<home>') : masked
}

/** A message with the root of the home as `$ROOT`, and every separator `/`. */
const said = (message, root) => message.replaceAll(root, '$ROOT').replaceAll('\\', '/')

/** The environment of a case: the homes, and `PATH` where the case says. */
function environment({ root, bin, flavour, pin, path }) {
  const env = { HOME: join(root, 'home'), PATH: join(root, 'elsewhere') }
  if (pin) env.CONSENSFLOW_HOME = join(root, 'home', '.consensflow')
  if (flavour === 'cmd') env.OS = 'Windows_NT'
  if (path === 'on') env.PATH = bin
  if (path === 'off') env.PATH = '/nowhere'
  if (path === 'among') env.PATH = ['/a', bin, '/b'].join(delimiter)
  if (path === 'near') env.PATH = [`${bin}-near`, bin.slice(0, -1), `${bin}/`].join(delimiter)
  return env
}

/** The folder a case installs in, made as its `place` says. */
function placed(root, place) {
  if (place === 'blocked') {
    writeFileSync(join(root, 'blocked'), '')
    return join(root, 'blocked', 'bin')
  }
  if (place === 'file') {
    writeFileSync(join(root, 'bin'), 'not a folder')
    return join(root, 'bin')
  }
  return join(root, 'bin')
}

/** What the folder holds, by name without `.cmd`; none when there is no folder. */
function held(bin, flavour, home) {
  let names
  try {
    names = readdirSync(bin).sort()
  } catch {
    return null
  }
  return names.map((name) => {
    const path = join(bin, name)
    const base = flavour === 'cmd' ? name.replace(/\.cmd$/, '') : name
    if (statSync(path).isDirectory()) return { name: base, directory: true }
    const found = { name: base, text: mask(readFileSync(path, 'utf8'), home) }
    if (flavour === 'sh') found.executable = (statSync(path).mode & 0o111) !== 0
    return found
  })
}

/** One case of `installTerminalCommand`, played in the form of `flavour`. */
function install(each, flavour) {
  const root = mkdtempSync(join(tmpdir(), 'cf-launcher-golden-'))
  try {
    const bin = placed(root, each.place)
    const env = environment({ root, bin, flavour, pin: each.pin, path: each.path })
    if (each.before) mkdirSync(bin)
    for (const file of each.before ?? []) {
      const path = join(bin, flavour === 'cmd' ? `${file.name}.cmd` : file.name)
      if (file.directory) mkdirSync(path)
      else writeFileSync(path, file.text)
      if (file.mode !== undefined) chmodSync(path, file.mode)
    }
    let installed = null
    let error = null
    try {
      const status = installTerminalCommand(env, { candidates: [bin] })
      if (status.installed) {
        installed = { name: basename(status.path).replace(/\.cmd$/, ''), onPath: status.onPath }
      }
    } catch (cause) {
      error = said(cause.message, root)
    }
    return {
      name: each.name,
      pin: each.pin === true,
      path: each.path ?? null,
      place: each.place ?? 'bin',
      before: each.before ?? [],
      after: held(bin, flavour, env.CONSENSFLOW_HOME),
      installed,
      error,
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}

/** What `terminalRuntime` reads of `text`, left in a command. */
function reading([name, text]) {
  const root = mkdtempSync(join(tmpdir(), 'cf-launcher-golden-'))
  try {
    const bin = join(root, 'bin')
    mkdirSync(bin)
    writeFileSync(join(bin, WINDOWS ? 'consensflow.cmd' : 'consensflow'), text)
    const env = { HOME: root, ...(WINDOWS ? { OS: 'Windows_NT' } : {}) }
    const read = terminalRuntime(env, { candidates: [bin] })
    return { name, text, read: read === null ? null : { runtime: read.runtime, entry: read.entry } }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}

/** What `staleClaudeHooks` says of the settings `content` stands for. */
function stale([name, content]) {
  const root = mkdtempSync(join(tmpdir(), 'cf-launcher-golden-'))
  try {
    const dir = join(root, 'claude')
    mkdirSync(dir)
    const file = { text: null, hex: null }
    if (content !== null && typeof content === 'object') {
      writeFileSync(join(dir, 'settings.json'), Buffer.from(content.hex, 'hex'))
      file.hex = content.hex
    } else if (content !== null) {
      writeFileSync(join(dir, 'settings.json'), content)
      file.text = content
    }
    try {
      const { events } = staleClaudeHooks({ CLAUDE_CONFIG_DIR: dir })
      return { name, ...file, events, throws: null }
    } catch (cause) {
      return { name, ...file, events: null, throws: cause.message }
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}

/** The text of a golden file of `cases`, under its `key`. */
const golden = (key, cases) => `{\n  "${key}": [\n${lines(cases)}\n  ]\n}\n`

/** Every golden, by its path in the repository, as this system makes them. */
export function launcherGoldens() {
  const files = {
    'crates/cf-launcher/tests/goldens/installs-cmd.json': golden(
      'installs',
      INSTALLS.filter((each) => !each.own).map((each) => install(each, 'cmd')),
    ),
    'crates/cf-launcher/tests/goldens/readings.json': `{\n  "readings": [\n${lines(READINGS.map(reading))}\n  ],\n  "dirnames": [\n${lines(DIRNAMES.map((path) => ({ path, dirname: posix.dirname(path) })))}\n  ]\n}\n`,
    'crates/cf-harness/tests/goldens/claude/stale-hooks.json': golden(
      'settings',
      SETTINGS.map(stale),
    ),
  }
  if (!WINDOWS) {
    files['crates/cf-launcher/tests/goldens/installs-sh.json'] = golden(
      'installs',
      INSTALLS.map((each) => install(each, 'sh')),
    )
  }
  return { files }
}

/** Writes every golden into the repository at `repo`. */
export function writeLauncherGoldens(repo) {
  const { files } = launcherGoldens()
  for (const [relative, text] of Object.entries(files)) {
    const path = join(repo, ...relative.split('/'))
    mkdirSync(dirname(path), { recursive: true })
    writeFileSync(path, text)
  }
  return { written: Object.keys(files).length }
}
