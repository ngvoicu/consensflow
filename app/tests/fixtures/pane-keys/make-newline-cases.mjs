#!/usr/bin/env node
/**
 * Regenerates `newline-cases.json`: what each harness's composer does with the bytes a pane might
 * write for a Shift+Enter (T-1330). No harness starts, no model runs and no provider is called.
 *
 * A case is `{ bytes, does }`: the bytes a pane writes to the harness, and what its input box
 * does with them: `"newline"` (a new line in the message), `"submit"` (the message is sent),
 * `"follow-up"` (Pi's Alt+Enter, which queues the message: a send as well) or `"nothing"`.
 *
 * - **Pi 1.5.0 is replayed** here: its own `matchesKey` of the installed `@earendil-works/pi-tui`
 *   against its default bindings (`TUI_KEYBINDINGS`: `tui.input.newLine`, `tui.input.submit`; and
 *   `app.message.followUp`, `alt+enter` off Windows, which `pi-coding-agent` keeps). Its editor
 *   tests the new-line bindings before the submit one (`components/editor.js`), as this does. Pi's
 *   keyboard protocol is off in a pane (xterm.js 6 answers no kitty query), so `setKittyProtocolActive(false)`.
 * - **The others are read from source, not run** (a harness would need a login to show its input
 *   box): the place of each rule is in `source`, and a rule that is a line of the harness's own
 *   code or documentation says which.
 *   - Claude Code 2.1.319: the key reader `Mr` of its binary's JavaScript (`\r` is `return`,
 *     `ESC CR` is `return` with `meta`, `\n` is `enter`, `CSI 13;2u` and `CSI 27;2;13~` are
 *     `return` with `shift`: `Ua` and `_c` are tried before any mode is asked), and its text
 *     input: `return` with `meta` or `shift` and `enter` insert a new line, `return` alone sends.
 *   - Codex 0.167.0: `codex-rs/tui/src/keymap.rs` `insert_newline` is Ctrl+J, Ctrl+M, Enter,
 *     Shift+Enter and Alt+Enter (the composer takes Enter first: `submit_keys`), and crossterm
 *     reads `\n` in raw mode as Ctrl+J, `ESC CR` as Alt+Enter and `CSI 13;2u` as Shift+Enter
 *     whatever keyboard flags were pushed (`event/sys/unix/parse.rs`); it reads no `CSI 27;2;13~`.
 *   - Devin 3000.12.28: its documentation (`docs.devin.ai/cli/reference/keyboard-shortcuts`):
 *     `editor.insert_newline` is `shift-enter`, `alt-enter`, `ctrl-j`; its binary is crossterm's.
 *   - Muse Code 1.4.14: the binary's composer keymap lists `newline`: `shift+enter`, `ctrl+j`,
 *     `ctrl+m`, `alt+enter`; its binary is crossterm's.
 *
 * **Windows** is a second table (`windows`) where it differs: the bytes are the same, and what changes
 * is what the console host makes of them before the harness reads them. Codex, Devin and Muse read
 * the console's key events (input mode 0x1f0, through crossterm), and are given a key; Claude Code and
 * Pi set virtual-terminal input (0x208) and are given the bytes again. `WINDOWS_READINGS` holds what the
 * shipped console host (ConPTY 1.25) made of each byte string, printed on zeewin2 on 2026-11-08 by
 * `cargo test -p cf-panes --lib shift_enter_candidates -- --ignored --nocapture` and held by the tests
 * of `crates/cf-panes/src/pty/keys.rs`. A key crossterm reads is `KeyCode::Enter` with the modifiers
 * the console says (`parse_key_event_record`): the line feed is Enter with Control, which none of the
 * three binds (their new-line lists are Shift+Enter, Alt+Enter, Ctrl+J, and Codex's Ctrl+M), and the
 * CSI u form is seven keys, `ESC [ 1 3 ; 2 u`, typed into the box as text (`"types"`).
 *
 * Run: `node app/tests/fixtures/pane-keys/make-newline-cases.mjs` (Pi installed).
 */
import { writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))

/** The bytes a pane might write for a Shift+Enter, and what each is as a key. */
const CANDIDATES = [
  { id: 'cr', key: 'Enter', bytes: [13] },
  { id: 'lf', key: 'Ctrl+J', bytes: [10] },
  { id: 'esc-cr', key: 'Alt+Enter', bytes: [27, 13] },
  { id: 'csi-u', key: 'Shift+Enter (CSI u)', bytes: [27, 91, 49, 51, 59, 50, 117] },
  {
    id: 'modify-other-keys',
    key: 'Shift+Enter (modifyOtherKeys)',
    bytes: [27, 91, 50, 55, 59, 50, 59, 49, 51, 126],
  },
]

// --- Pi -----------------------------------------------------------------------------------------

const PI_VERSION = process.env.PI_VERSION ?? '1.5.0'
const PI_TUI = join(
  homedir(),
  '.pi',
  'agent',
  'install',
  'releases',
  PI_VERSION,
  'node_modules',
  '@earendil-works',
  'pi-tui',
  'dist',
)

async function piCases() {
  const { matchesKey, setKittyProtocolActive } = await import(join(PI_TUI, 'keys.js'))
  const { TUI_KEYBINDINGS } = await import(join(PI_TUI, 'keybindings.js'))
  setKittyProtocolActive(false)
  const keys = (name) => [TUI_KEYBINDINGS[name].defaultKeys].flat()
  const newLine = keys('tui.input.newLine')
  const submit = keys('tui.input.submit')
  // `app.message.followUp` is `alt+enter` off Windows and `ctrl+q` on it (pi-coding-agent's core/keybindings.js).
  const followUp = ['alt+enter']
  const reads = (text, bindings) => bindings.some((binding) => matchesKey(text, binding))
  return CANDIDATES.map(({ id, bytes }) => {
    const text = String.fromCharCode(...bytes)
    const does = reads(text, newLine)
      ? 'newline'
      : reads(text, submit)
        ? 'submit'
        : reads(text, followUp)
          ? 'follow-up'
          : 'nothing'
    return { id, bytes, does }
  })
}

// --- Read from source ---------------------------------------------------------------------------

/** `does` by candidate id, in CANDIDATES' order. */
const rule = (...does) => CANDIDATES.map(({ id, bytes }, at) => ({ id, bytes, does: does[at] }))

const HARNESSES = {
  'claude-code': {
    version: '2.1.319',
    evidence: 'read',
    source:
      "Mr (key reader) and the text input's Return handler in the binary's JavaScript; default keybinding ctrl+j: chat:newline",
    cases: rule('submit', 'newline', 'newline', 'newline', 'newline'),
  },
  codex: {
    version: '0.167.0',
    evidence: 'read',
    source:
      'codex-rs/tui/src/keymap.rs (editor insert_newline), textarea.rs c0_line_feed test; crossterm event/sys/unix/parse.rs',
    cases: rule('submit', 'newline', 'newline', 'newline', 'nothing'),
  },
  pi: {
    version: PI_VERSION,
    evidence: 'replayed',
    source: "@earendil-works/pi-tui keys.js matchesKey against TUI_KEYBINDINGS' defaults",
    cases: await piCases(),
  },
  devin: {
    version: '3000.12.28',
    evidence: 'read',
    source:
      'docs.devin.ai/cli/reference/keyboard-shortcuts (editor.insert_newline: shift-enter, alt-enter, ctrl-j); crossterm',
    cases: rule('submit', 'newline', 'newline', 'newline', 'nothing'),
  },
  muse: {
    version: '1.4.14',
    evidence: 'read',
    source: "the binary's composer keymap (newline: shift+enter ctrl+j ctrl+m alt+enter); crossterm",
    cases: rule('submit', 'newline', 'newline', 'newline', 'nothing'),
  },
}

/** What the shipped console host made of each byte string, on zeewin2, 2026-11-08 (`keys.rs`). */
const WINDOWS_READINGS = {
  keyEvents: {
    cr: 'Enter, char 13, no modifier',
    lf: 'Enter, char 10, Control',
    'esc-cr': 'Enter, char 13, Alt',
    'csi-u': 'seven keys: ESC [ 1 3 ; 2 u, as typed characters',
    'modify-other-keys': 'no key',
  },
  virtualTerminalInput: {
    cr: '\\r',
    lf: '\\n',
    'esc-cr': 'ESC \\r',
    'csi-u': 'ESC [ 1 3 ; 2 u',
    'modify-other-keys': 'ESC [ 2 7 ; 2 ; 1 3 ~',
  },
}

// Codex, Devin and Muse read key events on Windows: what each is given is the Enter key with the
// modifiers of WINDOWS_READINGS.keyEvents; a Control+Enter is in none of their new-line lists.
for (const name of ['codex', 'devin', 'muse']) {
  HARNESSES[name].windows = rule('submit', 'nothing', 'newline', 'types', 'nothing')
}

const out = { candidates: CANDIDATES, windowsReadings: WINDOWS_READINGS, harnesses: HARNESSES }
writeFileSync(join(HERE, 'newline-cases.json'), `${JSON.stringify(out, null, 2)}\n`)
process.stdout.write(
  `newline-cases.json: ${Object.keys(HARNESSES).length} harnesses, ${CANDIDATES.length} byte strings each\n`,
)
