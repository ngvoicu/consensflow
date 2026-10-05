/**
 * The launch's pure functions, as tables of what Node answers:
 * - each harness's window (`hosts/lib/windows.js`): its command, arguments,
 *   environment and the keys it drops, started or resumed, over agents,
 *   sessions and first messages; and `childEnv`;
 * - the text a window is given (`windowText`) and the text Windows' console
 *   carries to Devin (`consoleText`), the latter over every code point it
 *   changes, under the Unicode version Node's ICU holds;
 * - how a send's answer reads as a delivery outcome (`admission`), and what
 *   a record says in the dispatcher's terms (`recordState`);
 * - a path as a file URL (`pathToFileURL`), on either platform's rules;
 * - the URLs OpenCode's channel asks of its server from the folder a window
 *   works in (`new URL`, `searchParams` and `encodeURIComponent`).
 *
 * A call that throws is written `{"throws": true}`; one that answers
 * undefined, `{"undefined": true}`.
 */
import { readFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'
import { childEnv, interactiveResume, interactiveStart } from '../../../hosts/lib/windows.js'
import { admission, recordState, windowText } from '../../../src/adapters/shared.js'
import { consoleText } from '../../../src/console-text.js'

const answer = (call) => {
  try {
    const value = call()
    return value === undefined ? { undefined: true } : value
  } catch {
    return { throws: true }
  }
}

/** Every kind a window may be asked of: the five, and two ConsensFlow opens none for. */
const KINDS = ['claude-code', 'codex', 'pi', 'devin', 'opencode', 'kimi', 'image']

/** The agents a window opens on: no model, a model, and with each level a harness reads. */
const AGENTS = [
  {},
  { model: 'm-1' },
  { model: 'm-1', effort: 'high' },
  { model: 'm-1', thinking: 'medium' },
  { model: 'm-1', effort: 'max', thinking: 'low' },
  { effort: 'high' },
  { thinking: 'high' },
  { model: '' },
  { model: 'm-1', effort: '' },
]

/** The sessions a window is asked to open on, and the first messages it may carry. */
const SESSIONS = [null, undefined, '', 'S-1']
const SEEDS = [null, undefined, '', 'Go on', ' -x "quoted" \u00e9']

function windowsTable() {
  const rows = []
  for (const kind of KINDS) {
    for (const fields of AGENTS) {
      const agent = { kind, ...fields }
      for (const sessionId of SESSIONS) {
        for (const seed of SEEDS) {
          for (const [call, open] of [
            ['start', interactiveStart],
            ['resume', interactiveResume],
          ]) {
            rows.push({
              call,
              agent,
              sessionId: sessionId === undefined ? { undefined: true } : sessionId,
              seed: seed === undefined ? { undefined: true } : seed,
              window: answer(() => open(agent, sessionId, seed)),
            })
          }
        }
      }
    }
  }
  return rows
}

/** `childEnv` over a base, overrides and the keys dropped, CMUX's own among them. */
function childEnvTable() {
  const base = {
    PATH: '/bin',
    HOME: '/h',
    ANTHROPIC_API_KEY: 'k',
    CMUX_SOCKET_PATH: '/s',
    CMUX_SOCKET: '/s2',
    CMUX_CLAUDE_HOOK_CMUX_BIN: '/c',
    CMUX_OTHER: 'kept',
  }
  const cases = [
    {},
    { env: { A: '1' } },
    { dropEnv: ['ANTHROPIC_API_KEY'] },
    { env: { ANTHROPIC_API_KEY: 'new', B: '2' }, dropEnv: ['ANTHROPIC_API_KEY', 'NOT_THERE'] },
    { env: { CMUX_SOCKET_X: 'x' } },
    { env: { PATH: '/usr/bin' }, dropEnv: [] },
  ]
  return [
    { base, declared: null, env: childEnv(base) },
    ...cases.map((declared) => ({ base, declared, env: childEnv(base, declared) })),
  ]
}

/** `windowText` over every Latin-1 character, line ends, and characters past it. */
function windowTextTable() {
  const texts = [
    ...Array.from({ length: 256 }, (_, code) => String.fromCharCode(code)),
    '',
    'plain',
    'a\r\nb',
    'a\rb',
    'a\r\r\nb',
    'a\n\rb',
    '\r\n\r\n',
    'tab\there',
    'esc\u001b[31mred',
    '\u2028\u2029',
    '\u{1F600} emoji',
    'e\u0301',
    '\ufeff\ufffd',
  ]
  return [
    { text: null, window: windowText(null) },
    ...texts.map((text) => ({ text, window: windowText(text) })),
  ]
}

/**
 * `consoleText`: every code point it changes, alone (a surrogate is none:
 * a text holds pairs), and texts that compose or decompose as a whole.
 */
function consoleTextTable() {
  const changed = []
  for (let code = 0; code <= 0x10ffff; code += 1) {
    if (code >= 0xd800 && code <= 0xdfff) continue
    const character = String.fromCodePoint(code)
    const text = consoleText(character)
    if (text !== character) changed.push([code, text])
  }
  const texts = [
    '',
    'e\u0301',
    'A\u030a',
    '\u1e9b\u0323',
    'n\u0303o',
    '\u2460\u00bd',
    '\ufb01ne',
    '[ConsensFlow m-3 \u00b7 T-1 \u00b7 result from @worker]\nCosts \u20ac100 \u2014 20\u00d7 faster \u2192 \u201cdone\u201d\u2026',
    'Culoarea: albastr\u0103; \u00eet\u0326i scriu, caf\u00e9, Stra\u00dfe, 5 \u00b5s, \u00d1and\u00fa',
    '\u2400\u241b\u2421',
    '\u{1F600}\u2705',
  ]
  return {
    unicode: process.versions.unicode,
    changed,
    texts: [
      { text: null, console: consoleText(null) },
      ...texts.map((text) => ({ text, console: consoleText(text) })),
    ],
  }
}

/** `admission` over the answers a send gets, the refusal named, and whether it queued. */
function admissionTable() {
  const answers = [
    undefined,
    null,
    {},
    { ok: true },
    { ok: true, extra: 1 },
    { ok: false },
    { ok: false, admitted: false },
    { ok: false, admitted: false, error: 'refused' },
    { ok: false, admitted: null, error: 'deadline' },
    { ok: false, admitted: true, error: 'odd' },
    { ok: false, error: 'too-large' },
    { ok: false, error: 'transport', cause: 'socket closed' },
    { ok: false, admitted: false, cause: 'stale', error: 'pane' },
    { ok: 'yes' },
    'ok',
  ]
  const rows = []
  for (const sent of answers) {
    for (const refusal of [undefined, 'the window refused the paste']) {
      for (const options of [undefined, {}, { queued: true }, { queued: false }]) {
        rows.push({
          sent: sent === undefined ? { undefined: true } : sent,
          refusal: refusal === undefined ? { undefined: true } : refusal,
          options: options === undefined ? { undefined: true } : options,
          outcome: answer(() => admission(sent, refusal, options)),
        })
      }
    }
  }
  return rows
}

/** `recordState` over what a record may say. */
function recordStateTable() {
  const item = { id: 'a', role: 'assistant', text: 'x', complete: true }
  const records = [
    undefined,
    null,
    {},
    { unknown: true, reason: 'unreadable: x' },
    { items: [] },
    { items: [], inFlight: true },
    { items: [item] },
    { items: [item], settlement: { state: 'settled' } },
    { items: [item], settlement: { state: 'in-flight' } },
    { items: [item], settlement: { state: 'unknown' } },
    { items: [], settlement: { state: 'in-flight' } },
    { items: [], settlement: { state: 'unknown' } },
    { items: 'not a list', settlement: { state: 'settled' } },
    { items: [item], failed: true, quota: { state: 'exhausted', at: null, resetsAt: null } },
    { items: [item], failed: 'yes' },
    { items: [item], quota: undefined },
  ]
  return records.map((record) => ({
    record: record === undefined ? { undefined: true } : record,
    state: answer(() => recordState(record)),
  }))
}

/**
 * A path as a file URL, by either platform's rules, over every ASCII
 * character a name may hold and paths made odd. A root alone (`/`, `C:\`)
 * is left out: Node then adds a slash by the separator of the platform it
 * runs on, not of the rules it was asked for.
 */
function fileUrlTable() {
  const ascii = Array.from({ length: 128 }, (_, code) => String.fromCharCode(code)).filter(
    (character) => character !== '/' && character !== '\\',
  )
  const posix = [
    ...ascii.map((character) => `/x${character}y`),
    '/a/b',
    '/a b/c',
    '/caf\u00e9/\u{1F600}',
    '/a\\b',
    '/a/../b/./c',
    '/a/b/',
    '/a//b',
    '/a/./b/../c/',
    '/a/%2e%2e/b',
    '/a/\u00e4\u{1F600}',
    '/a/{b}<c>`|^~',
  ]
  const windows = [
    ...ascii.map((character) => `C:\\x${character}y`),
    'C:\\a\\b',
    'c:\\a%b#c',
    'C:\\Users\\n\u00e9\\x y',
    '\\\\server\\share\\x y',
    'C:/forward/slashes',
    'C:\\a\\..\\b',
    'C:\\a\\',
    'C:\\a//b',
    '\\\\SERVER\\share\\a\\..\\b',
    '\\\\localhost\\share\\x',
    '\\\\LocalHost\\s\\x',
    '\\\\m\u00fcnich\\share\\x',
    '//server/share/x',
    '\\\\?\\C:\\x',
    '\\\\?\\UNC\\Server\\share\\x',
    '\\\\127.0.0.1\\s\\x',
    '\\\\[::1]\\s\\x',
    '\\\\server',
    '\\\\\\x',
    '\\\\server:80\\s',
    '\\\\a b\\s',
    '\\\\server\\share\\',
    'C:\\a\\..\\..\\..\\b',
    'C:\\a\\.\\b\\',
    'C:\\a\\\u00e4\u{1F600}\\%41#?',
    'c:/Mixed\\slashes/',
    'C:\\a{b}<c>`d',
    '\\\\ser\tver\\share\\x',
    '\\\\ser\nver\\s\\x',
    '\\\\ser\rver\\s\\x',
    '\\\\\tlocalhost\\s\\x',
    '\\\\SER\tVER.Example\\s\\x',
  ]
  return [
    ...posix.map((path) => ({
      path,
      windows: false,
      url: answer(() => pathToFileURL(path, { windows: false }).href),
    })),
    ...windows.map((path) => ({
      path,
      windows: true,
      url: answer(() => pathToFileURL(path, { windows: true }).href),
    })),
  ]
}

/**
 * What `seedSession` and `createSession` (`src/channels/opencode.js`) build
 * their URLs of, which the table below writes as they are there: it refuses
 * to be made once the source no longer holds them.
 */
const URL_EXPRESSIONS = [
  `\`/session/\${encodeURIComponent(sessionId)}\``,
  `\`/session/\${encodeURIComponent(sessionId)}/prompt_async\``,
  "nativeUrl.searchParams.set('directory', directory)",
  "url.searchParams.set('directory', directory)",
  `\`session?directory=\${encodeURIComponent(canonical)}\``,
  `endpoint.href.endsWith('/') ? endpoint.href : \`\${endpoint.href}/\``,
]

/** Folders a window may work in, made odd: what each builder escapes, and what it keeps. */
const DIRECTORIES = [
  '/work/app',
  '/work/my app',
  "/work/it's",
  "/work/'''",
  '/work/(a)',
  '/work/a!b~c*d',
  '/work/caf\u00e9',
  '/work/\u65e5\u672c',
  '/work/\u{1F600}',
  '/work/e\u0301',
  '/work/a&b=c#d',
  '/work/a%b',
  '/work/%20',
  '/work/%zz',
  '/work/a+b',
  '/work/a b+c',
  '/work/a?b',
  '/work/"q"',
  '/work/<x>',
  '/work/{y}[z]|^`',
  '/work/a\\b',
  '/work/a\nb',
  '/work/a\tb',
  '/work/\u0000',
  '/work/\u007f',
  '/work/\u00a0',
  '/work/\u2028',
  '/work/\ufeff',
  '/work/-_.',
  '/work/~',
  '/',
  '',
  `/${'x'.repeat(300)}`,
  'C:\\Users\\me\\proj',
  'C:\\',
  '\\\\server\\share\\dir',
]

/** The sessions a URL names: an id OpenCode mints, which is all that is asked. */
const OPENCODE_SESSIONS = ['ses_abc123', 'ses_A', 'ses_0', 'ses_ABCxyz789']

/**
 * The URLs OpenCode's channel asks of its server for a folder: the session's
 * own record and the first message, built with `searchParams`, and the new
 * session, built from `encodeURIComponent` and then parsed. On the origin
 * `launchConfiguration` gives a server (`http://127.0.0.1:<port>`).
 */
function openCodeUrlsTable() {
  const source = readFileSync(new URL('../../../src/channels/opencode.js', import.meta.url), 'utf8')
  for (const expression of URL_EXPRESSIONS) {
    if (!source.includes(expression)) {
      throw new Error(
        `src/channels/opencode.js no longer holds ${expression}: the table is made of it`,
      )
    }
  }
  const endpoint = new URL('http://127.0.0.1:41001')
  const base = endpoint.href.endsWith('/') ? endpoint.href : `${endpoint.href}/`
  const row = (sessionId, directory) => {
    const nativeUrl = new URL(`/session/${encodeURIComponent(sessionId)}`, endpoint)
    nativeUrl.searchParams.set('directory', directory)
    const url = new URL(`/session/${encodeURIComponent(sessionId)}/prompt_async`, endpoint)
    url.searchParams.set('directory', directory)
    const creation = new URL(`session?directory=${encodeURIComponent(directory)}`, base)
    return {
      session: sessionId,
      directory,
      settings: nativeUrl.href,
      prompt: url.href,
      creation: creation.href,
    }
  }
  return [
    ...DIRECTORIES.map((directory) => row('ses_abc123', directory)),
    ...OPENCODE_SESSIONS.map((sessionId) => row(sessionId, '/work/app')),
  ]
}

/** The tables, as one golden. */
export function tables() {
  return {
    windows: windowsTable(),
    childEnv: childEnvTable(),
    windowText: windowTextTable(),
    consoleText: consoleTextTable(),
    admission: admissionTable(),
    recordState: recordStateTable(),
    fileUrl: fileUrlTable(),
    openCodeUrls: openCodeUrlsTable(),
  }
}
