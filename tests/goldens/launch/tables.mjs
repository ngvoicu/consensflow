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
 * - a path as a file URL (`pathToFileURL`), on either platform's rules.
 *
 * A call that throws is written `{"throws": true}`; one that answers
 * undefined, `{"undefined": true}`.
 */
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
  }
}
