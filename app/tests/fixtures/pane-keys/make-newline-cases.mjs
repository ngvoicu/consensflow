#!/usr/bin/env node
/**
 * Regenerates `newline-cases.json`: what the page writes to a window for a Shift+Enter, and what
 * each harness's input box does with the bytes a pane might write for it (T-1330, T-1336). No
 * harness starts, no model runs and no provider is called.
 *
 * - `sends` is what the page writes, by harness and by platform (`mac`, `windows`): the page's
 *   `shiftEnterFor` (app/ui/term.js), which the page tests hold it to and which the real-pty test of
 *   `crates/cf-panes/src/pty/keys.rs` reads its bytes from. **A pane's key never starts with
 *   Escape** (T-1336): a program takes a lone Escape for "stop", and a read that ends in the middle of
 *   a two-byte write (a pty's input queue nearly full, the console host's 4,096-byte read) reads
 *   the Escape alone, then the rest. A single byte cannot be split.
 * - `rows` is, for each harness and platform, a case `{ id, bytes, does, whileBusy }` for each byte
 *   string a pane might write: what the input box does with it at rest and while a turn runs
 *   (`"newline"`, `"submit"`, `"follow-up"`, `"interrupt"`, `"types"` when its characters are typed
 *   into the message, `"nothing"`).
 * - `evidence` says, for each harness, which rows were **run** and which were **read from source or
 *   documentation**, and the same for what happens while a turn runs.
 *
 * What is run here:
 *
 * - **Pi 1.5.0**: its installed `CustomEditor` and `KeybindingsManager`, with callback stubs for
 *   submit, interrupt and follow-up, at rest and "busy" (Pi's follow-up action queues while a turn
 *   runs), with the Mac bindings (`alt+enter` follows up) and the Windows ones (`ctrl+q`), which are
 *   selected explicitly on the Mac running this. It does not run the interactive mode's own handlers
 *   (`interactive-mode.js:3720-3749`): that "busy" is what those callbacks are told, not a turn.
 *
 * What is only read (no harness's input box can start without a login, and the closed-source ones
 * are binaries):
 *
 * - **Claude Code 2.1.319**: its key reader `Mr` of the binary's JavaScript (`\r` is `return`, `ESC CR`
 *   is `return` with `meta`, `\n` is `enter`, `CSI 13;2u` and `CSI 27;2;13~` are `return` with
 *   `shift`; `Ua` and `_c` are tried before any mode is asked) and its text input (`return` with
 *   `meta` or `shift`, and `enter`, insert a new line; `return` alone sends): no branch of it asks
 *   whether a turn runs, so the rows hold at rest and while a turn runs. Windows: virtual-terminal
 *   input hands it the bytes as they were written.
 * - **Codex 0.167.0**: `codex-rs/tui/src/keymap.rs` (`insert_newline` is Ctrl+J, Ctrl+M, Enter, Shift+Enter
 *   and Alt+Enter; the composer takes Enter for `submit` first), `textarea.rs`, `bottom_pane/mod.rs`
 *   (while a turn runs and no popup is open a plain Escape interrupts it before the composer is
 *   asked) and crossterm's readers (`event/sys/unix/parse.rs`: `\n` in raw mode is Ctrl+J, `ESC CR` is
 *   Alt+Enter, `CSI 13;2u` is Shift+Enter whatever flags were pushed, no `CSI 27;2;13~`; a read that
 *   ends in an Escape is a lone Escape: T-1336 ran this with the pinned fork on the Mac;
 *   `event/sys/windows/parse.rs`: the Enter key with the modifiers the console says). On Windows
 *   Codex's `tui/windows_key_sequence.rs` rebuilds `ESC [ 1 3 ; 2 u` from seven console key events,
 *   within 50 ms, as Shift+Enter; it does not rescue `ESC CR`.
 * - **Devin 3000.12.28**: its documentation (`docs.devin.ai/cli/reference/keyboard-shortcuts`:
 *   `editor.insert_newline` is `shift-enter`, `alt-enter`, `ctrl-j`) and its binary (crossterm).
 *   How its input handler orders a busy turn's keys is not established.
 * - **Muse Code 1.4.14**: the binary's composer keymap (`newline`: `shift+enter`, `ctrl+j`, `ctrl+m`,
 *   `alt+enter`; `submit`, `queue`, `cancel` apart) (crossterm). How it orders a busy turn's keys
 *   is not established.
 *
 * **Windows** differs where the console host turns the bytes into key events first: Codex, Devin and
 * Muse read the console's key events (input mode 0x1f0, crossterm) and are given a key; Claude Code
 * and Pi set virtual-terminal input (0x208) and are given the bytes again. `WINDOWS_READINGS` is what
 * the shipped console host (ConPTY 1.25) made of each byte string on zeewin2 (`cargo test -p cf-panes
 * --lib pty::keys`, which holds them). A key crossterm reads is `KeyCode::Enter` with the console's
 * modifiers (`parse_key_event_record`): the line feed is Enter with Control, which none of the three
 * binds; `CSI 13;2u` is seven key events that Codex's own adapter rebuilds and Devin and Muse are
 * not known to.
 *
 * Run: `node app/tests/fixtures/pane-keys/make-newline-cases.mjs` (Pi installed).
 */
import { writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))

const ESC_CR = [27, 13]
const LF = [10]

/**
 * What the page writes for a Shift+Enter, by harness and platform: `shiftEnterFor` in app/ui/term.js.
 */
const SENDS = {
  'claude-code': { mac: LF, windows: LF },
  // Codex, Devin and Muse on Windows read the console's key events: no single byte is their new
  // line there (a line feed is Enter with Control, bound to nothing), so ESC CR is sent until the
  // human decides (T-1336).
  codex: { mac: LF, windows: ESC_CR },
  pi: { mac: LF, windows: LF },
  devin: { mac: LF, windows: ESC_CR },
  muse: { mac: LF, windows: ESC_CR },
}

/** The bytes a pane might write for a Shift+Enter, and what each is as a key. */
const CANDIDATES = [
  { id: 'cr', key: 'Enter', bytes: [13] },
  { id: 'lf', key: 'Ctrl+J', bytes: LF },
  { id: 'esc-cr', key: 'Alt+Enter', bytes: ESC_CR },
  { id: 'csi-u', key: 'Shift+Enter (CSI u)', bytes: [27, 91, 49, 51, 59, 50, 117] },
  {
    id: 'modify-other-keys',
    key: 'Shift+Enter (modifyOtherKeys)',
    bytes: [27, 91, 50, 55, 59, 50, 59, 49, 51, 126],
  },
  { id: 'esc', key: 'Escape (a read that ends in the Escape of an Escape-led key)', bytes: [27] },
]

// --- Pi: its editor, run -------------------------------------------------------------------------

const PI_VERSION = process.env.PI_VERSION ?? '1.5.0'
const PI_ROOT = join(
  homedir(),
  '.pi',
  'agent',
  'install',
  'releases',
  PI_VERSION,
  'node_modules',
  '@earendil-works',
)

async function piRows(windows) {
  const { CustomEditor } = await import(
    join(PI_ROOT, 'pi-coding-agent/dist/modes/interactive/components/custom-editor.js')
  )
  const { KeybindingsManager } = await import(
    join(PI_ROOT, 'pi-coding-agent/dist/core/keybindings.js')
  )
  const { setKittyProtocolActive } = await import(join(PI_ROOT, 'pi-tui/dist/keys.js'))
  // Pi's keyboard protocol is off in a pane: xterm.js 6 answers no kitty query.
  setKittyProtocolActive(false)
  const run = (bytes, busy) => {
    const text = String.fromCharCode(...bytes)
    const kb = new KeybindingsManager({
      'app.message.followUp': windows ? 'ctrl+q' : 'alt+enter',
    })
    const calls = []
    const editor = new CustomEditor({ requestRender() {} }, { borderColor: (s) => s }, kb)
    editor.onSubmit = () => calls.push('submit')
    editor.onEscape = () => calls.push('interrupt')
    editor.onAction('app.message.followUp', () => calls.push(busy ? 'follow-up' : 'submit'))
    editor.setText('draft')
    editor.handleInput(text)
    if (calls.length > 0) return calls[0]
    if (editor.getText() === 'draft\n') return 'newline'
    if (editor.getText() === 'draft') return 'nothing'
    return 'types'
  }
  return CANDIDATES.map(({ id, bytes }) => ({
    id,
    bytes,
    does: run(bytes, false),
    whileBusy: run(bytes, true),
  }))
}

// --- The others: read ----------------------------------------------------------------------------

/** `[does, whileBusy]` by candidate id, in CANDIDATES' order. */
const rows = (...each) =>
  CANDIDATES.map(({ id, bytes }, at) => ({ id, bytes, does: each[at][0], whileBusy: each[at][1] }))

const same = (does) => [does, does]

/** Claude Code: no branch of its text input asks whether a turn runs; a lone Escape stops it. */
const CLAUDE = rows(
  same('submit'),
  same('newline'),
  same('newline'),
  same('newline'),
  same('newline'),
  ['nothing', 'interrupt'],
)

/**
 * Codex, Devin and Muse on the Mac (crossterm's unix reader): a lone Escape at rest is nothing and
 * while a turn runs it interrupts.
 */
const CROSSTERM_MAC = (busyLf) =>
  rows(
    ['submit', 'submit'],
    ['newline', busyLf],
    ['newline', busyLf],
    ['newline', busyLf],
    ['nothing', 'nothing'],
    ['nothing', 'interrupt'],
  )

/** The three on Windows: the console's key events (WINDOWS_READINGS.keyEvents). */
const CROSSTERM_WINDOWS = (csiU) =>
  rows(
    ['submit', 'submit'],
    ['nothing', 'nothing'],
    ['newline', 'newline'],
    csiU,
    ['nothing', 'nothing'],
    ['nothing', 'interrupt'],
  )

/** What the shipped console host made of each byte string, on zeewin2, 2026-11-08 (`keys.rs`). */
const WINDOWS_READINGS = {
  keyEvents: {
    cr: 'Enter, char 13, no modifier',
    lf: 'Enter, char 10, Control',
    'esc-cr': 'Enter, char 13, Alt',
    'csi-u': 'seven keys: ESC [ 1 3 ; 2 u, as characters (Codex rebuilds them as Shift+Enter)',
    'modify-other-keys': 'no key',
    esc: 'the Escape key, when it ends a read of the console host (4,096 bytes)',
  },
  virtualTerminalInput: {
    cr: '\\r',
    lf: '\\n',
    'esc-cr': 'ESC \\r',
    'csi-u': 'ESC [ 1 3 ; 2 u',
    'modify-other-keys': 'ESC [ 2 7 ; 2 ; 1 3 ~',
  },
}

const HARNESSES = {
  'claude-code': {
    version: '2.1.319',
    source:
      "Mr (key reader) and the text input's Return handler in the binary's JavaScript; default keybinding ctrl+j: chat:newline",
    evidence: {
      ran: [],
      read: ['rows', 'whileBusy'],
      note: 'its text input has no busy branch; the binary was read, not run',
    },
    rows: { mac: CLAUDE, windows: CLAUDE },
  },
  codex: {
    version: '0.167.0',
    source:
      'codex-rs/tui/src/keymap.rs, textarea.rs, bottom_pane/mod.rs, tui/windows_key_sequence.rs; crossterm event/sys/unix/parse.rs and event/sys/windows/parse.rs',
    evidence: {
      ran: [],
      read: ['rows', 'whileBusy'],
      note: "T-1336's review ran the pinned crossterm fork's reader on the Mac (ESC CR written at the end of a full pty read is read as Escape, Enter; a line feed is Ctrl+J); that probe is not kept in the repository",
    },
    rows: {
      mac: CROSSTERM_MAC('newline'),
      windows: CROSSTERM_WINDOWS(['newline', 'newline']),
    },
  },
  pi: {
    version: PI_VERSION,
    source:
      "the installed @earendil-works/pi-coding-agent CustomEditor and KeybindingsManager (pi-tui's keys.js matcher), with callback stubs",
    evidence: {
      ran: ['rows', 'whileBusy'],
      read: [],
      note: "the editor and its matcher were run (Mac and Windows follow-up bindings, selected explicitly); the interactive mode's own submit and follow-up handlers were not",
    },
    rows: { mac: await piRows(false), windows: await piRows(true) },
  },
  devin: {
    version: '3000.12.28',
    source:
      'docs.devin.ai/cli/reference/keyboard-shortcuts (editor.insert_newline: shift-enter, alt-enter, ctrl-j); the binary is crossterm',
    evidence: {
      ran: [],
      read: ['rows'],
      note: "how its input handler orders a busy turn's keys is not established: whileBusy repeats the documented new-line action",
    },
    rows: {
      mac: CROSSTERM_MAC('newline'),
      windows: CROSSTERM_WINDOWS(['types', 'types']),
    },
  },
  muse: {
    version: '1.4.14',
    source:
      "the binary's composer keymap (newline: shift+enter ctrl+j ctrl+m alt+enter; submit, queue and cancel apart); crossterm",
    evidence: {
      ran: [],
      read: ['rows'],
      note: "how its input handler orders a busy turn's keys is not established: whileBusy repeats the keymap's new-line action",
    },
    rows: {
      mac: CROSSTERM_MAC('newline'),
      windows: CROSSTERM_WINDOWS(['types', 'types']),
    },
  },
}

for (const [name, harness] of Object.entries(HARNESSES)) harness.sends = SENDS[name]

const out = { candidates: CANDIDATES, windowsReadings: WINDOWS_READINGS, harnesses: HARNESSES }
writeFileSync(join(HERE, 'newline-cases.json'), `${JSON.stringify(out, null, 2)}\n`)
process.stdout.write(
  `newline-cases.json: ${Object.keys(HARNESSES).length} harnesses, ${CANDIDATES.length} byte strings each, two platforms\n`,
)
