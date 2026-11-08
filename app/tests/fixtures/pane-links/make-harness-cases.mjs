#!/usr/bin/env node
/**
 * Regenerates `harness-cases.json`: what Pi, Claude Code and Codex print for a link, replayed
 * offline from the harnesses' own formatting code (T-1314's review, T-1315). No harness starts, no
 * model runs and no provider is called:
 *
 * - Pi 1.5.0: `Markdown.render` of the installed `@earendil-works/pi-tui`, with an ANSI-only theme,
 *   `PI_HYPERLINKS` set to 0 (the plain `words (address)` fallback) or 1 (what ConsensFlow's launch
 *   sets: the words as an OSC 8 hyperlink, once a row), at 48, 80 and 120 columns. Pi pads each row
 *   with spaces to the width and writes the rows joined by CR/LF, so xterm marks none of them wrapped.
 * - Claude Code 2.1.319: its `Cb` link formatter and the `h3e`/`Tb` checks that decide whether it
 *   writes OSC 8, sliced out of the installed binary's JavaScript when this runs (nothing of it is
 *   kept here). Plain: `Cb(address, words, {supportsHyperlinks: false})`. With ConsensFlow's
 *   `FORCE_HYPERLINK=1` in the environment: `Cb(address, words)`, which asks `Tb()` itself.
 * - Codex: `codex-cases.json` (8 cases), recorded by the review of T-1291 from the isolated
 *   compilation of `osc8_hyperlink`, `web_destination` and `sanitized_destination` of
 *   openai/codex `rust-v0.167.0` (`codex-rs/tui/src/terminal_hyperlinks.rs`); Codex sends OSC 8 for
 *   an `http` or `https` address whatever the terminal, so it has nothing to switch.
 *
 * A case is `{id, harness, mode, width, printed, destination, expect}`: `printed` is what the
 * program wrote into the pane, `destination` the address the program meant, and `expect` what a
 * click on the address must do: `"exact"` opens `destination` and nothing else, `"none"` is no
 * link and opens nothing. Never a shortened address.
 *
 * Run: `node app/tests/fixtures/pane-links/make-harness-cases.mjs` (Pi and Claude Code installed).
 */
import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))

const WIKI = 'https://en.wikipedia.org/wiki/Function_(mathematics)'
const GITHUB = 'https://github.com/ngvoicu/consensflow/blob/main/app/src-tauri/src/link.rs#L10-L20'
const BAYES = "https://en.wikipedia.org/wiki/Bayes'_theorem"
const ENCODED = 'https://example.com/a%28b%29'
const COMMA = 'https://example.com/a,b^c'
const YAHOO = 'https://en.wikipedia.org/wiki/Yahoo!'

// --- Pi ---------------------------------------------------------------------------------------

const PI_RELEASES = join(homedir(), '.pi', 'agent', 'install', 'releases')
const PI_VERSION = process.env.PI_VERSION ?? '1.5.0'
const PI_DIST = join(
  PI_RELEASES,
  PI_VERSION,
  'node_modules',
  '@earendil-works',
  'pi-tui',
  'dist',
)
if (!existsSync(PI_DIST)) {
  throw new Error(`Pi ${PI_VERSION} is not installed at ${PI_RELEASES} (${readdirSync(PI_RELEASES)})`)
}

/** Pi's Markdown for `markdown`, rendered at `width` with `padding` columns each side, hyperlinks `on`/off. */
function piRows(markdown, width, padding, on) {
  const script = `
    process.env.PI_HYPERLINKS = ${JSON.stringify(on ? '1' : '0')}
    process.env.TERM_PROGRAM = 'ConsensFlow'
    const { Markdown } = await import(${JSON.stringify(`${PI_DIST}/components/markdown.js`)})
    const { getCapabilities } = await import(${JSON.stringify(`${PI_DIST}/terminal-image.js`)})
    if (getCapabilities().hyperlinks !== ${on}) throw new Error('Pi did not take PI_HYPERLINKS')
    const same = (text) => text
    const theme = Object.fromEntries(
      ['heading', 'link', 'linkUrl', 'code', 'codeBlock', 'codeBlockBorder', 'quote', 'quoteBorder',
       'hr', 'listBullet', 'bold', 'italic', 'strikethrough', 'underline'].map((key) => [key, same]))
    theme.link = (text) => '\\u001b[34m' + text + '\\u001b[39m'
    theme.underline = (text) => '\\u001b[4m' + text + '\\u001b[24m'
    theme.linkUrl = (text) => '\\u001b[2m' + text + '\\u001b[22m'
    process.stdout.write(JSON.stringify(new Markdown(${JSON.stringify(markdown)}, ${padding}, 0, theme).render(${width})))
  `
  // A child process per render: Pi reads PI_HYPERLINKS once, when its capabilities are first asked.
  return JSON.parse(execNode(script, { PI_HYPERLINKS: on ? '1' : '0' }))
}

function execNode(script, env) {
  return execFileSync(process.execPath, ['--input-type=module', '-e', script], {
    encoding: 'utf8',
    env: { PATH: process.env.PATH, HOME: process.env.HOME, ...env },
  })
}

/**
 * What a click on the address must do. An OSC 8 hyperlink carries its destination whole, row by
 * row: `"exact"`. A plain address is exact where one row holds all of it, and where the program
 * broke it over rows with CR/LF (which xterm does not mark wrapped) nothing says the next row
 * continues it, so it is `"none"`: no link is better than a wrong one.
 */
function expected(mode, rows, destination) {
  if (mode === 'osc8') return 'exact'
  const plain = rows.map((row) => row.replace(/\u001b\[[0-9;]*m/g, ''))
  return plain.some((row) => row.includes(destination)) ? 'exact' : 'none'
}

const cases = []

// 12 of the review's 25 cases, as it ran them: Pi with hyperlinks off, four addresses, three widths.
const piPlain = [
  ['pi-48-0', 48, WIKI],
  ['pi-48-1', 48, GITHUB],
  ['pi-48-2', 48, ENCODED],
  ['pi-48-3', 48, COMMA],
  ['pi-80-4', 80, WIKI],
  ['pi-80-5', 80, GITHUB],
  ['pi-80-6', 80, ENCODED],
  ['pi-80-7', 80, COMMA],
  ['pi-120-8', 120, WIKI],
  ['pi-120-9', 120, GITHUB],
  ['pi-120-10', 120, ENCODED],
  ['pi-120-11', 120, COMMA],
]
for (const [id, width, destination] of piPlain) {
  const rows = piRows(`[Source](${destination})`, width, 0, false)
  cases.push(piCase(id, 'plain', width, rows, destination))
}
// The apostrophe address (C), which the review found cut at `Bayes`, on Pi's plain fallback too.
for (const width of [48, 80, 120]) {
  const rows = piRows(`[Source](${BAYES})`, width, 0, false)
  cases.push(piCase(`pi-${width}-bayes`, 'plain', width, rows, BAYES))
}
// The same with the margin Pi gives an assistant's text (`outputPad`, one column a side).
for (const [name, destination] of [
  ['wiki', WIKI],
  ['github', GITHUB],
  ['bayes', BAYES],
]) {
  for (const width of [48, 80]) {
    const rows = piRows(`[Source](${destination})`, width, 1, false)
    cases.push(piCase(`pi-${width}-${name}-padded`, 'plain', width, rows, destination))
  }
}
// With ConsensFlow's launch: PI_HYPERLINKS=1. The words are the hyperlink, the address behind them.
for (const width of [48, 80, 120]) {
  for (const [name, destination] of [
    ['wiki', WIKI],
    ['github', GITHUB],
    ['bayes', BAYES],
    ['encoded', ENCODED],
    ['comma', COMMA],
  ]) {
    const rows = piRows(`[Source](${destination})`, width, 0, true)
    cases.push(piCase(`pi-${width}-${name}-osc8`, 'osc8', width, rows, destination))
  }
}
// Pi's own wrapping of an address printed bare in text: with hyperlinks on, every row it lands on
// is a hyperlink of the whole address; off, it is the plain pieces the review found cut.
for (const [name, destination, width] of [
  ['wiki', WIKI, 40],
  ['github', GITHUB, 48],
  ['bayes', BAYES, 40],
]) {
  for (const on of [true, false]) {
    const rows = piRows(`See ${destination} now`, width, 0, on)
    cases.push(piCase(`pi-bare-${name}-${on ? 'osc8' : 'plain'}`, on ? 'osc8' : 'plain', width, rows, destination))
  }
}

function piCase(id, mode, width, rows, destination) {
  return {
    id,
    harness: 'pi',
    mode,
    width,
    printed: `${rows.join('\r\n')}\r\n`,
    destination,
    expect: expected(mode, rows, destination),
    source: `pi-tui ${PI_VERSION} Markdown.render, ANSI-only theme, PI_HYPERLINKS=${mode === 'osc8' ? 1 : 0}`,
  }
}

// --- Claude Code ------------------------------------------------------------------------------

const CLAUDE_BIN = process.env.CLAUDE_BIN ?? join(homedir(), '.local', 'share', 'claude', 'versions', '2.1.319')
const claudeBinary = readFileSync(CLAUDE_BIN)

/** The bytes of the binary from `from` up to and including `to`. */
function slice(from, to) {
  const start = claudeBinary.indexOf(from)
  const end = claudeBinary.indexOf(to, start)
  if (start < 0 || end < 0) throw new Error(`Claude Code's ${from} is not where 2.1.319 had it`)
  return claudeBinary.subarray(start, end).toString('latin1')
}

const link = slice('var BJn="', 'export{BJn,jJn,Cb,mzo}')
const checks = slice('var L=["ghostty"', 'export{h3e,Tb}')
// The pieces the two modules import, stood in: no colour, no settings file, no terminal.
const claude = new Function(
  'lf',
  'rs',
  'be',
  'Zn',
  'kCe',
  'A',
  `${link};${checks};return {Cb, Tb}`,
)(
  () => undefined,
  (value) => ['0', 'false', 'no', 'off'].includes(String(value).toLowerCase()),
  new Proxy({}, { get: () => (text) => `\u001b[94m${text}\u001b[39m` }),
  (text) => text,
  () => false,
  { default: { supportsHyperlink: () => false } },
)

const claudeAddresses = [
  ['wiki', WIKI],
  ['github', GITHUB],
  ['bayes', BAYES],
  ['encoded', ENCODED],
  ['yahoo', YAHOO],
]
for (const [name, destination] of claudeAddresses) {
  cases.push({
    id: `claude-${name}-plain`,
    harness: 'claude-code',
    mode: 'plain',
    width: 160,
    printed: `${claude.Cb(destination, 'Source', { supportsHyperlinks: false })}\r\n`,
    destination,
    expect: 'exact',
    source: 'Claude Code 2.1.319 Cb, hyperlinks off',
  })
}
process.env.TERM_PROGRAM = 'ConsensFlow'
delete process.env.WT_SESSION
for (const [name, destination] of claudeAddresses) {
  // Without the switch, ConsensFlow's pane is a terminal Claude Code does not know: plain text.
  delete process.env.FORCE_HYPERLINK
  const unforced = claude.Cb(destination, 'Source')
  if (unforced.includes('\u001b]8;;')) throw new Error('Claude Code wrote OSC 8 with no switch')
  process.env.FORCE_HYPERLINK = '1'
  cases.push({
    id: `claude-${name}-osc8`,
    harness: 'claude-code',
    mode: 'osc8',
    width: 160,
    printed: `${claude.Cb(destination, 'Source')}\r\n`,
    destination,
    expect: 'exact',
    source: 'Claude Code 2.1.319 Cb under FORCE_HYPERLINK=1 and TERM_PROGRAM=ConsensFlow',
  })
  // A bare address (no words of its own): the address is both the words and the hyperlink.
  cases.push({
    id: `claude-${name}-bare-osc8`,
    harness: 'claude-code',
    mode: 'osc8',
    width: 160,
    printed: `${claude.Cb(destination)}\r\n`,
    destination,
    expect: 'exact',
    source: 'Claude Code 2.1.319 Cb(address) under FORCE_HYPERLINK=1',
  })
}

// --- Codex ------------------------------------------------------------------------------------

for (const codex of JSON.parse(readFileSync(join(HERE, 'codex-cases.json'), 'utf8'))) {
  cases.push(codex)
}

writeFileSync(join(HERE, 'harness-cases.json'), `${JSON.stringify(cases, null, 1)}\n`)
process.stdout.write(`${cases.length} cases written\n`)
