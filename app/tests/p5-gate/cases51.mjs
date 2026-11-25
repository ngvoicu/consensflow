// The cases of T-2083 (xterm-probe.mjs) and T-2092 (follow-up.mjs), as ops: bytes and resizes in order, then a suffix.
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { tree } from './lib.mjs'

const enc = (x) => (typeof x === 'string' ? [...new TextEncoder().encode(x)] : [...x])
const esc = '\x1b',
  cs = `${esc}[`
const size = (cols, rows) => ({ resize: [cols, rows] })
export const cases = []
const add = (from, name, cols, rows, ops, suffix = '', afterResize) =>
  cases.push({
    from,
    name,
    cols,
    rows,
    ops: ops.map((op) => (op.resize ? op : { bytes: enc(op) })),
    suffix: enc(suffix),
    afterResize,
  })
// T-2083: prefix, an optional resize before the cut, the suffix, an optional resize after it.
const first = [
  { name: '7-bit CSI, DEL before final', prefix: `${cs}?20\x7f`, suffix: '04hA' },
  { name: 'UTF-8 C1 CSI', prefix: '\u009b?20', suffix: '04hA' },
  { name: 'UTF-8 C1 OSC', prefix: '\u009d0;', suffix: 'TITLE\x07Z' },
  { name: 'UTF-8 C1 ST closes OSC', prefix: `${esc}]0;t\u009cZ`, suffix: '' },
  {
    name: 'half a UTF-8 character within CSI',
    prefix: Uint8Array.from([27, 91, 0xc2]),
    suffix: Uint8Array.from([0x9b, 63, 50, 48, 48, 52, 104, 65]),
  },
  {
    name: 'soft reset after saved cursor',
    prefix: `abc${esc}7${cs}4;4H${cs}!p`,
    suffix: `${esc}8X`,
  },
  { name: '1048 cursor save', prefix: `abc${cs}?1048h${cs}4;4H`, suffix: `${cs}?1048lX` },
  {
    name: 'saved cursor scrolled up',
    prefix: `${cs}3;2H${esc}7${cs}8;1H\r\n\r\n`,
    suffix: `${esc}8X`,
  },
  {
    name: 'repeated alternate save used on alt',
    prefix: `M${cs}?1049hA${cs}4;4H${cs}?1049h${cs}6;6H`,
    suffix: `${esc}8X`,
  },
  { name: 'origin reset before first enable', prefix: `prompt${cs}4;4H${cs}?6l`, suffix: 'X' },
  { name: 'invalidated scrolling region', prefix: `prompt${cs}4;4H${cs}4;4r`, suffix: 'X' },
  {
    name: 'same-size page resize with margins',
    prefix: `HEADER${cs}2;6r${cs}6;1HLAST`,
    beforeResize: [30, 8],
    suffix: '\r\nNEXT',
  },
  { name: 'erase at pending wrap', cols: 5, prefix: `ABCDE${cs}K`, suffix: 'X' },
  {
    name: 'wrap flags followed by resize',
    prefix: '123456789012345678901234567890X',
    suffix: `${cs}5;1H`,
    afterResize: [40, 8],
  },
  {
    name: 'already resized normal buffer',
    cols: 5,
    prefix: `abcdefg${cs}5;1H`,
    beforeResize: [9, 8],
    suffix: '',
  },
  { name: 'REP after printable at cut', prefix: 'prompt', suffix: `${cs}3b` },
  { name: 'wide CJK overwritten at first cell', prefix: '界A\rx', suffix: '' },
  { name: 'ordinary emoji followed by text', prefix: '🙂!', suffix: 'X' },
  { name: 'custom tab stop', prefix: `${cs}3g${cs}1;4H${esc}H\r`, suffix: '\tX' },
  { name: 'sparse 401-row pane', rows: 401, prefix: `${cs}401;1HX`, suffix: '' },
  { name: 'baseline normal paste', prefix: `prompt${cs}?2004h`, suffix: 'X' },
]
for (const c of first)
  add(
    'T-2083',
    c.name,
    c.cols ?? 30,
    c.rows ?? 8,
    [c.prefix, ...(c.beforeResize ? [size(...c.beforeResize)] : [])],
    c.suffix,
    c.afterResize,
  )
const s = (...a) => add('T-2092', ...a)
s('split em dash', 30, 8, [[65, 0xe2, 0x80], [0x94]], 'B')
s('unsplit em dash control', 30, 8, ['A—'], 'B')
s(
  'saved cursor with scroll region starting on first row',
  30,
  8,
  [`${cs}4;2H${esc}7${cs}1;6r${cs}6;1H\r\n\r\n`],
  `${esc}8X`,
)
s(
  'restore outside origin margins before cut',
  30,
  8,
  [`${cs}8;2H${esc}7${cs}2;5r${cs}?6h${esc}8X`],
  '!',
)
s('line feed into previously wrapped row', 5, 8, ['abcdeF', `${cs}H\n`], '', [9, 8])
s('backspace at right edge', 5, 8, ['ABCDE\bX'], '!')
s('shrink while cursor is at pending wrap', 5, 8, ['ABCDE', size(4, 8)], 'X')
s('widen while cursor is at pending wrap, then print', 5, 8, ['ABCDE', size(9, 8), 'X'], '!')
s('wide glyph at the last column with wrap off', 5, 8, [`${cs}?7labcd界`], 'X')
s('erase trailing half of a wide glyph', 8, 5, [`界A${cs}1;2H${cs}X`], '')
s('wide glyph moved onto wrapped row, then widen', 5, 8, [`abcd界Z${cs}5;1H`, size(10, 8)], '')
s('repeat 65535 control', 30, 8, [`A${cs}65535b`], 'X')
s('repeat 65536', 30, 8, [`A${cs}65536b`], 'X')
s('clear all tabs then widen', 12, 4, [`${cs}3g`, size(22, 4)], '\tX')
s(
  'custom tab to the left of old last column, then widen',
  12,
  4,
  [`${cs}3g${cs}1;4H${esc}H\r`, size(22, 4)],
  '\t\tX',
)
s('OSC terminated', 30, 8, [`before${esc}]0;${'x'.repeat(9000)}GHOST\x07Z`])
s('OSC still open', 30, 8, [`before${esc}]0;${'x'.repeat(10000)}visible`])
s('malformed UTF-8', 30, 8, [[97, 255, 98, 195, 99, 128]])
s('DCS containing BEL', 30, 8, [`before${esc}Pqpayload\x07GHOST${esc}\\Z`])
s('wrap flag alone changes joined text', 5, 4, ['abcde     X', `${cs}2;1H${cs}2K`])
s('80-column pane after refused wide page resize', 80, 24, [size(1050, 24), `${cs}1;1001HX`], '!')
s('24-row pane after refused tall page resize', 80, 24, [size(80, 430), `${cs}401;1HX`], '!')
s('clear all tabs then change only the height', 30, 8, [`${cs}3g`, size(30, 9)], '\tX')
s('write over the unused cell before a wrapped wide character', 5, 8, ['abcd界', `${cs}1;5HX`], '')
s(
  'Claude Code input box after a repaint',
  40,
  8,
  [`${'─'.repeat(40)}❯ `, `${cs}H\r\n`, `${cs}3;1H${'─'.repeat(40)}${cs}2;3H`],
  '',
)
const blankStart = `Ready${cs}2;1H${' '.repeat(30)}${cs}3;1H ${cs}1;6H`
const blankWrap = `${cs}2;1H${' '.repeat(31)}${cs}1;6H`
const blankHard = `${cs}3;1H${cs}2K ${cs}1;6H`
s('unchanged blank rows: before repaint', 30, 8, [blankStart])
s('unchanged blank rows: wrap repaint', 30, 8, [blankStart, blankWrap])
s('unchanged blank rows: alternate repaint', 30, 8, [
  blankStart,
  blankWrap,
  blankHard,
  blankWrap,
  blankHard,
])
const corpus = readFileSync(
  resolve(tree, 'crates/cf-harness/src/testing/corpus/recorded.txt'),
  'utf8',
)
const recorded = corpus
  .split('=== claude 00-first-screen.txt dialog\n')[1]
  .split('\n=== ')[0]
  .split('\n')
const at = recorded.findIndex((row) => row.includes('❯ No, exit'))
if (at < 0 || !recorded[at + 1].includes('Yes, I accept')) throw Error('fixture changed')
const cup = (row, col) => `${cs + row};${col}H`
const allRows = recorded.map((row, i) => cup(i + 1, 1) + row).join('')
const conditionWrap = cup(at + 1, 1) + recorded[at].padEnd(160, ' ') + recorded[at + 1]
const erasePadding = `${cup(at + 1, [...recorded[at]].length + 1) + cs + (160 - [...recorded[at]].length)}X`
s('recorded Claude first-screen dialog after a synthetic repaint', 160, 18, [
  allRows,
  conditionWrap + erasePadding,
  `${cup(at + 1, 1)}\r\n`,
])
s('last cell of the largest accepted pane', 80, 24, [size(1000, 400), `${cs}400;1000HX`])
