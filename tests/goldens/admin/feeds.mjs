/**
 * Node's own release feed, asked over a network the scenario scripts: the
 * request it makes (the address, the five seconds, no redirect), and the
 * release or the words of why not that each answer comes to, as a row says
 * them. Where Node's words are its runtime's (V8 reading JSON), the
 * scenario says what Rust says in their place.
 */
import { exe, onPath, said } from './kit.mjs'

/** Where a CLI whose feed is read as each format lives, and which harness it is. */
const SETUPS = {
  npm: { harness: 'codex', folder: '$ROOT/bin' },
  cask: { harness: 'codex', folder: '$ROOT/brew/Caskroom/codex/1/bin' },
  formula: { harness: 'codex', folder: '$ROOT/brew/Cellar/codex/1/bin' },
  text: { harness: 'claude', folder: '$ROOT/home/.local/share/claude/versions' },
  manifest: { harness: 'devin', folder: '$ROOT/bin' },
}

/** An answer of 200 whose body is `text`, in one chunk. */
const body = (text) => ({ status: 200, chunks: [text] })

/** An answer of 200 whose body is the JSON of `value`. */
const json = (value) => body(JSON.stringify(value))

/** A feed asked once for each of `answers`, the first asked as it is and each after it again. */
function feeds(name, setup, answers, extra = {}) {
  const { harness, folder } = SETUPS[setup]
  return {
    name,
    files: [exe(`${folder}/${harness}`)],
    env: { PATH: onPath(folder) },
    effects: {
      run: { [`${harness} --version`]: answers.map(() => said(`${harness} 5.0.0\n`)) },
      fetch: answers,
    },
    steps: answers.map((_, index) => ({ op: 'check', id: harness, refresh: index > 0 })),
    ...extra,
  }
}

/** What Rust says where a feed's answer was not JSON: its own words in place of V8's. */
const NOT_JSON = 'Release metadata is not valid JSON'

/** What Rust says where JSON held nothing to read a release from: the same as for none. */
const UNAVAILABLE = 'Release version is unavailable'

/** A step's row, with its update's reason as Rust says it. */
const rustReason = (reason) => (rows) => {
  rows[0].update.reason = reason
  return rows
}

/** A body of exactly `units` UTF-16 code units: JSON with a release, padded with `pad`. */
function padded(units, pad = 'x') {
  const head = '{"version":"1.2.3","pad":"'
  const tail = '"}'
  const width = pad.length
  const count = Math.floor((units - head.length - tail.length) / width)
  const filler = units - head.length - tail.length - count * width
  return {
    status: 200,
    chunks: [head, { text: pad, count }, 'x'.repeat(filler) + tail],
  }
}

export function feedScenarios() {
  const invalid = ['<html>\n<head>', '', '{"version":', "{'version':1}", '﻿{"version":"1.2.3"}']
  return [
    feeds('a registry that answers a release', 'npm', [
      json({ name: '@openai/codex', version: '1.2.3' }),
      { status: 200, chunks: ['{"name":"x","vers', 'ion":"1.2.3"}'] },
      json({ version: '1.2.3-beta.1' }),
      json({ version: '1.2.3', pad: 'é \u{1F600}' }),
      body('{"version":"1.2.3","version":"2.0.0"}'),
      body('  {"version":"1.2.3"}\n'),
      { status: 299, chunks: ['{"version":"1.2.3"}'] },
    ]),
    feeds('Homebrew says the release of a cask in its version', 'cask', [
      json({ token: 'codex', version: '4.5.6' }),
      json({ versions: { stable: '7.8.9' } }),
    ]),
    feeds('a formula says its stable release', 'formula', [
      json({ name: 'codex', versions: { stable: '7.8.9', head: 'HEAD' } }),
      json({ versions: { head: 'HEAD' } }),
      json({ versions: null }),
      json({ versions: { stable: 7 } }),
      json({ version: '1.2.3' }),
      json({}),
    ]),
    feeds("Devin's manifest says its release as an npm registry does", 'manifest', [
      json({ version: '3000.10.21', channel: 'current' }),
    ]),
    feeds("Claude's installer says its release as plain text", 'text', [
      body('2.1.280\n'),
      body('  2.1.280  '),
      body('﻿2.1.280'),
      body('stable 2.1.280 (build)\r\n'),
      body('2.1.280\r\n'),
      body(''),
      body('  \n'),
      body('latest'),
      body('<html>'),
      body('\u00852.1.280\u0085'),
      body('2.1.280 €'),
    ]),
    feeds('an answer with no release in it is a feed that does not say', 'npm', [
      json({ name: 'x' }),
      json({ version: 1 }),
      json({ version: null }),
      json({ version: 'latest' }),
      json({ version: '' }),
      json({ version: ['1.2.3'] }),
      body('[]'),
      body('"1.2.3"'),
      body('5'),
      body('true'),
    ]),
    feeds(
      'JSON that is none is said in Rust s own words where Node said V8s',
      'npm',
      invalid.map(body),
      {
        kept: invalid.map((_, step) => ({
          step,
          why: 'Node said what V8 said of the text; Rust says its own words',
          rust: rustReason(NOT_JSON),
        })),
      },
    ),
    feeds('null where an object was read has no version to give', 'npm', [body('null')], {
      kept: [
        {
          step: 0,
          why: "Node said V8's TypeError of reading null; Rust says no release was there",
          rust: rustReason(UNAVAILABLE),
        },
      ],
    }),
    feeds(
      'a status that is no success is said as such, whatever the body, and a redirect is refused, one with no body at all is read as empty',
      'npm',
      [
        { status: 404, chunks: ['{"version":"1.2.3"}'] },
        { status: 500, chunks: ['oops'] },
        { status: 503, chunks: [] },
        { status: 400, chunks: ['{"version":"1.2.3"}'] },
        { status: 300, chunks: ['{"version":"1.2.3"}'] },
        { status: 301, chunks: [] },
        { status: 302, chunks: ['{"version":"1.2.3"}'] },
        { status: 303, chunks: [] },
        { status: 304, chunks: [] },
        { status: 305, chunks: [] },
        { status: 307, chunks: [] },
        { status: 308, chunks: [] },
        { status: 204, chunks: [] },
      ],
      {
        kept: [
          {
            step: 12,
            why: "an answer of 204 has no body in fetch, and Node's iterating it threw V8's TypeError of reading null; Rust reads it as an empty body, which is no JSON",
            rust: rustReason(NOT_JSON),
          },
        ],
      },
    ),
    feeds(
      'a connection that fails, a redirect and a body cut off are said in fetch s words',
      'npm',
      [{ failure: 'refused' }, { status: 302, chunks: [] }, { failure: 'cut' }],
    ),
    {
      ...feeds(
        'a request that outlives five seconds says so, whether the head or the body is slow',
        'npm',
        [{ failure: 'stall' }, { failure: 'stall-body' }],
      ),
      steps: [
        { op: 'check', id: 'codex', name: 'head' },
        { op: 'advance', ms: 4999 },
        { op: 'advance', ms: 1 },
        { op: 'check', id: 'codex', refresh: true, name: 'body' },
        { op: 'advance', ms: 4999 },
        { op: 'advance', ms: 1 },
      ],
    },
    feeds('a body of exactly two million units is read, one unit more is too big', 'npm', [
      padded(2_000_000),
      padded(2_000_001),
      padded(1_999_999),
    ]),
    feeds('a body is measured in units of text, not bytes: accents and emoji', 'npm', [
      padded(2_000_000, 'é'),
      padded(2_000_001, 'é'),
      padded(2_000_000, '\u{1F600}'),
      padded(2_000_002, '\u{1F600}'),
    ]),
    feeds(
      'a body that crosses the limit over several chunks, and what comes after is not read',
      'npm',
      [
        {
          status: 200,
          chunks: [{ text: 'x', count: 1_000_000 }, { text: 'x', count: 1_000_001 }, 'never'],
        },
        { status: 200, chunks: [{ text: 'x', count: 1_999_999 }, 'xx'] },
        { status: 200, chunks: [{ text: 'x', count: 2_000_001 }] },
      ],
    ),
    feeds('a character cut between two chunks is read as two replacement characters', 'text', [
      { status: 200, chunks: ['1.2.3 ', { bytes: [0xe2, 0x82] }, { bytes: [0xac] }] },
      { status: 200, chunks: ['1.2.3 ', { bytes: [0xf0] }, { bytes: [0x9f, 0x98, 0x80] }] },
      { status: 200, chunks: ['1.2.3 ', { bytes: [0xf0, 0x9f] }, { bytes: [0x98, 0x80] }] },
      { status: 200, chunks: ['1.2.3 ', { bytes: [0xf0, 0x9f, 0x98] }, { bytes: [0x80] }] },
      { status: 200, chunks: ['1.2.3 ', { bytes: [0xe2, 0x82, 0xac] }] },
      { status: 200, chunks: ['1.2.3 ', { bytes: [0xff] }, 'x'] },
      { status: 200, chunks: [{ bytes: [0xe2] }, { bytes: [0x82] }, { bytes: [0xac] }, ' 1.2.3'] },
    ]),
  ]
}
