// The gate's streams: random, seeded (mulberry32), built from the tokens the reviews' cases are
// made of (T-2096's 51, T-2121's): text, wide characters, em dashes, margins, modes, tabs,
// saved cursors, strings, questions, resizes at random offsets, reload points.
//
// A case is { name, seed, cols, rows, ops }, ops being:
//   { emit: number[] }              bytes the program prints, one message's worth
//   { repeat: number[], count }     the same bytes many times (past the cap)
//   { resize: [cols, rows] }        the page's xterm takes a size and the pane is resized
//   { reload }                      a new page subscribes (the old one was quiescent)
//   { gone }                        the page goes: what the program prints parks until the next reload
//   { depart }                      the next emit is handed to the page, which leaves before it parses it
//   { past: true }                  the case goes past the cap: only the tracked state is compared
export function rng(seed) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}
const utf8 = (s) => [...new TextEncoder().encode(s)]
const E = '\x1b', C = E + '['
export function makeGen(seed, { tracked = false } = {}) {
  const r = rng(seed)
  const int = (a, b) => a + Math.floor(r() * (b - a + 1))
  const pick = (xs) => xs[Math.floor(r() * xs.length)]
  const words = ['alpha', 'beta', 'gamma', 'prompt', '$ ', '> ', 'ok', 'ERROR', 'x', '0123456789', 'lorem ipsum', ' ', '  ', '--', '│', '─'.repeat(int(1, 12))]
  const wide = ['界', '漢字', '日本語', '🙂', '👍', 'é', 'é', '—', '…', '“q”', '’', '‍', 'ｱ', '👨‍👩‍👧', 'ab界cd']
  const sgr = () => C + pick(['0', '1', '4', '7', '31', '42;1', '38;5;' + int(0, 255), '48;2;' + int(0, 255) + ';' + int(0, 255) + ';' + int(0, 255), '0;1;32', '39', '49']) + 'm'
  const modeSet = tracked
    ? [1, 6, 7, 25, 47, 1047, 1049, 66, 1004, 2004, 1000, 1002, 1003, 1006, 1005, 1015, 9]
    : [1, 6, 7, 25, 47, 1047, 1049, 66, 1004, 2004, 1000, 1002, 1003, 1006, 45, 1048, 12, 2026]
  const tokens = [
    () => pick(words), () => pick(words) + pick(words), () => pick(wide),
    () => '\r\n', () => '\n', () => '\r', () => '\t', () => '\b', () => pick(['\x07', '\x0e', '\x0f']),
    () => C + int(1, 30) + ';' + int(1, 100) + 'H', () => C + int(1, 8) + pick(['A', 'B', 'C', 'D']), () => C + int(1, 100) + 'G', () => C + int(1, 30) + 'd',
    () => C + 'H', () => E + '7', () => E + '8', () => C + 's', () => C + 'u',
    () => E + pick(['D', 'E', 'M']), () => C + pick(['0', '1', '2', '']) + 'J', () => C + pick(['0', '1', '2', '']) + 'K',
    () => C + int(1, 20) + pick(['X', 'P', '@', 'L', 'M', 'S', 'T', 'b']),
    sgr, sgr,
    () => { const a = int(1, 12), b = int(a + 1, 40); return C + a + ';' + b + 'r' },
    () => C + int(1, 12) + 'r', () => C + 'r', () => C + int(1, 12) + ';0r', () => C + '6;2r',
    () => C + '?' + pick(modeSet) + pick(['h', 'l']),
    () => C + '?' + pick(modeSet) + 'h', () => C + '4' + pick(['h', 'l']),
    () => E + pick(['=', '>']), () => E + 'H', () => C + pick(['0g', '3g', 'g']),
    () => (r() < 0.12 ? pick([E + 'c', C + '!p']) : ''),
    () => E + '(0', () => E + '(B',
    () => E + ']0;' + pick(words) + pick(['\x07', E + '\\']),
    () => E + ']8;;http://x.y/' + int(1, 99) + '\x07' + pick(words) + E + ']8;;\x07',
    () => E + 'P' + pick(['q', '1$r', '+q4d73']) + pick(words) + E + '\\',
    () => E + '_' + pick(words) + E + '\\',
  ]
  const questions = [C + '5n', C + '6n', C + 'c', C + '0c', C + '>c', C + '?6n', C + '18t', C + '?2004$p', C + '?25$p']
  const atom = () => {
    const x = r()
    if (x < 0.12) return pick(questions)
    if (x < 0.14) return '\x7f'
    if (x < 0.15) return String.fromCharCode(int(0x80, 0xff))
    return pick(tokens)()
  }
  return { r, int, pick, atom, questions, words, wide }
}
const MIB = 1 << 20
/** Cut a byte list into emits at random positions (mid-sequence and mid-character included). */
function cuts(g, bytes, pieces) {
  const at = [0]
  for (let i = 1; i < pieces; i++) at.push(g.int(0, bytes.length))
  at.sort((a, b) => a - b)
  at.push(bytes.length)
  const out = []
  for (let i = 0; i + 1 < at.length; i++) if (at[i + 1] > at[i]) out.push(bytes.slice(at[i], at[i + 1]))
  return out
}
function size(g) { return [g.int(4, 100), g.int(2, 30)] }
export function randomCase(seed) {
  const g = makeGen(seed)
  const [cols, rows] = size(g)
  const atoms = g.int(1, 60)
  let all = []
  for (let i = 0; i < atoms; i++) all.push(...utf8(g.atom()))
  const emits = cuts(g, all, g.int(1, Math.min(30, atoms + 2)))
  const ops = []
  for (const emit of emits) {
    const x = g.r()
    if (x < 0.18) {
      const n = g.pick([1, 1, 1, 2, 3, 6])
      let last = [cols, rows]
      for (let k = 0; k < n; k++) {
        let s = size(g)
        if (g.r() < 0.4) s = [last[0], g.pick([rows, rows + 2, 2 + (rows % 5)])]
        if (g.r() < 0.1) s = last
        ops.push({ resize: s }); last = s
      }
    }
    if (x > 0.82 && x < 0.9) ops.push({ reload: true })
    if (x >= 0.9 && x < 0.935) ops.push({ gone: true }, ...(g.r() < 0.5 ? [] : [{ emit: utf8(g.pick(g.questions) + 'p') }]), { reload: true })
    if (x >= 0.935 && x < 0.955) { ops.push({ depart: true }, { emit }, { reload: true }); continue }
    ops.push({ emit })
    if (g.r() < 0.05) ops.push({ reload: true })
  }
  if (g.r() < 0.6) ops.push({ reload: true })
  // A size on its way as the page reloads, let go at one of the five points (T-2269).
  if (g.r() < 0.1) ops.push(...heldSize(g.pick([0, 3, 255, 256]), g.pick(HOLD_POINTS), cols, rows, C + '5n'))
  ops.push({ emit: utf8(g.pick(['X', 'Z\r\n', 'ABCDE', '界', C + '6n', '—'])) })
  return { name: 'random', seed, cols, rows, ops }
}
/** The points at which a held size is let go (`gate.mjs`). */
export const HOLD_POINTS = ['p1', 'p2', 'p3', 'p4', 'p5']
/**
 * `count` sizes the journal keeps, then a size held on its way and let go at `point` around a
 * reload, with `question` printed for the page that goes (p1, p2: before it subscribes anew) or
 * for the new one (p3 after its subscription and before its first pull, p4 between two pulls, p5
 * once its history is in).
 */
export function heldSize(count, point, cols, rows, question) {
  const ops = []
  for (let i = 0; i < count; i++) ops.push({ resize: [i % 2 === 0 ? cols + 1 : cols, rows] })
  // A history long enough for two pulls.
  if (point === 'p4') ops.push({ repeat: [0], count: Math.floor(1.2 * (1 << 20)) })
  ops.push({ hold: [cols + 2, rows], point })
  const asked = utf8(question + 'TAIL')
  if (point === 'p1' || point === 'p2') ops.push({ gone: true }, { emit: asked }, { reload: true })
  else if (point === 'p3') ops.push({ reload: true, after: asked })
  else if (point === 'p4') ops.push({ reload: true, after: asked }, { emit: asked })
  else ops.push({ reload: true }, { emit: asked })
  return ops
}
/** A case that goes past the 8 MiB cap, then more: only the tracked state is compared after. */
export function pastCapCase(seed) {
  const g = makeGen(seed, { tracked: true })
  const [cols, rows] = size(g)
  const ops = []
  const prefix = []
  for (let i = 0, n = g.int(2, 30); i < n; i++) prefix.push(...utf8(g.atom()))
  let sized = [cols, rows]
  for (const emit of cuts(g, prefix, g.int(1, 6))) {
    // a size after a region: of both kinds, a width alone as often as any (T-2171: xterm resets the margins on each)
    if (g.r() < 0.5) { sized = g.r() < 0.5 ? [g.int(4, 100), sized[1]] : size(g); ops.push({ resize: sized }) }
    ops.push({ emit })
  }
  const line = utf8(g.pick(g.words) + ' ' + g.pick(g.words) + g.pick(['\r\n', '\n\r']))
  const total = g.int(8.6 * MIB, 10 * MIB)
  const parts = g.int(1, 3)
  for (let p = 0; p < parts; p++) {
    ops.push({ repeat: line, count: Math.floor(total / parts / line.length) })
    if (g.r() < 0.5) {
      for (let i = 0, n = g.int(1, 8); i < n; i++) ops.push({ emit: utf8(g.atom()) })
    }
    if (g.r() < 0.4) { sized = g.r() < 0.5 ? [g.int(4, 100), sized[1]] : size(g); ops.push({ resize: sized }) }
  }
  ops.push({ reload: true })
  if (g.r() < 0.6) { ops.push({ emit: utf8('tail text\r\n') }, { reload: true }) }
  ops.push({ emit: utf8('X') })
  return { name: 'past-cap', seed, cols, rows, ops, past: true }
}
/** The T-2121 cases the review named, each as ops. */
export function namedCases() {
  const cs = []
  const add = (name, cols, rows, ops, past) => cs.push({ name, seed: 0, cols, rows, ops, past })
  const e = (s) => ({ emit: utf8(s) })
  add('5 resize at byte 1, then margins 2-6 and LF', 30, 8, [e('a'), { resize: [30, 10] }, e(C + '2;1HKEEP' + C + '2;6r'), { reload: true }, e(C + '6;1H\nX')])
  for (const prefix of ['ABCD ', 'ABCD—', 'ABC界', 'A界BC', 'ABCDE'])
    add('9 full row ' + prefix, 5, 8, [e(prefix), { reload: true }, e('X'), { reload: true }])
  add('8 one Z at resize 257', 30, 8, [e('\0'), ...Array.from({ length: 257 }, (_, i) => ({ resize: [31 + (i % 2), 8] })), { gone: true }, e('Z'), { reload: true }, { reload: true }])
  add('8 4096 resizes at one offset', 30, 8, [e('abc'), ...Array.from({ length: 4096 }, (_, i) => ({ resize: [31 + (i % 2), 8] })), e('Z'), { reload: true }, e('Q')])
  add('8 paste mode cut by a resize flood', 30, 8, [e('x'.repeat(100)), e(C + '?20'), ...Array.from({ length: 300 }, (_, i) => ({ resize: [31 + (i % 2), 8] })), e('04hA'), { reload: true }, e('B')])
  add('8 rows 8, 10, 8 then LF', 30, 8, [e(C + '2;1HKEEP' + C + '2;6r'), { resize: [30, 10] }, { resize: [30, 8] }, { reload: true }, e(C + '6;1H\nX')])
  for (const [name, phase] of [['already answered', 'a'], ['parked for new page', 'g'], ['live on current page', 'l'], ['accepted by old page before its draw', 'd']]) {
    const ops = []
    if (phase === 'a') ops.push(e(C + '5n'), { reload: true })
    if (phase === 'g') ops.push({ gone: true }, e(C + '5n'), { reload: true })
    if (phase === 'l') ops.push({ reload: true }, e(C + '5n'))
    if (phase === 'd') ops.push({ depart: true }, e(C + '5n'), { reload: true })
    ops.push(e('.'))
    add('7 question ' + name, 30, 8, ops)
  }
  add('7 two questions keep their order', 30, 8, [e(C + '5n' + C + '6n'), { gone: true }, e(C + 'c' + C + '5n'), { reload: true }, e(C + '6n')])
  add('past cap: modes and margins then a paste', 40, 12, [e(C + '2;9r' + C + '?2004h' + C + '?1h' + C + '?1006h' + C + '?1002h'), { repeat: utf8('filler line of text\r\n'), count: 520000 }, { reload: true }, e('X')], true)
  add('past cap: margins then a resize then margins', 40, 12, [e(C + '2;9r'), { resize: [40, 10] }, e(C + '3;5r'), { repeat: utf8('filler line of text\r\n'), count: 520000 }, { reload: true }, e('X')], true)
  add('past cap: a resize after the margins', 40, 12, [e(C + '2;9r'), { resize: [40, 10] }, { repeat: utf8('filler line of text\r\n'), count: 520000 }, { reload: true }, e('X')], true)
  // T-2171's margin cases: past the cap, past 256 sizes, against the shipped xterm's own rules.
  const C2 = '\x1b['
  const filler = { repeat: utf8('filler line of text\r\n'), count: 520000 }
  const widths = Array.from({ length: 257 }, (_, i) => ({ resize: [41 + (i % 2), 12] }))
  add('margins A past cap: a width-only size after them', 40, 12, [e(C2 + '2;9r'), { resize: [50, 12] }, filler, { reload: true }, e('X')], true)
  add('margins B past cap: the alternate screen after them', 40, 12, [e(C2 + '2;9r' + C2 + '?1049h'), filler, { reload: true }, e('X')], true)
  add('margins C past cap: set in the alternate screen, then left', 40, 12, [e(C2 + '?1049h' + C2 + '2;9r' + C2 + '?1049l'), filler, { reload: true }, e('X')], true)
  add('margins D past cap: a region below the screen after them', 40, 12, [e(C2 + '2;9r' + C2 + '20;30r'), filler, { reload: true }, e('X')], true)
  add('margins E past cap: a one-number region at the last row', 40, 12, [e(C2 + '2;9r' + C2 + '12r'), filler, { reload: true }, e('X')], true)
  add('margins F: 257 width changes after them', 40, 12, [e(C2 + '2;9r'), e('abc'), ...widths, { reload: true }, e('X')], false)
  add('margins G past cap: both screens, a size between', 40, 12, [e(C2 + '3;7r' + C2 + '?1049h' + C2 + '2;9r'), { resize: [44, 12] }, e(C2 + '4;8r'), filler, { reload: true }, e('X')], true)
  // T-2179's finding 3: a setting xterm refuses never costs the region the screen uses (the first, normal, the inactive normal, the alternate).
  const refused = (n) => Array.from({ length: n }, (_, i) => C2 + (20 + i) + ';' + (30 + i) + 'r').join('')
  for (const n of [16, 1000]) {
    add('margins H past cap: ' + n + ' refused settings, the normal screen', 40, 12, [e(C2 + '2;9r' + refused(n)), filler, { reload: true }, e('X')], true)
    add('margins I past cap: ' + n + ' refused settings, the inactive normal screen', 40, 12, [e(C2 + '2;9r' + refused(n) + C2 + '?1049h'), filler, { reload: true }, e('X')], true)
    add('margins J past cap: ' + n + ' refused settings, the alternate screen', 40, 12, [e(C2 + '?1049h' + C2 + '2;9r' + refused(n)), filler, { reload: true }, e('X')], true)
  }
  // T-2269's two orderings, at 255 sizes held and at 256 (the 257th starts the journal again),
  // and the other points.
  for (const [name, point] of [
    ['before subscription', 'p2'],
    ['after subscription', 'p3'],
    ['before the sink refuses', 'p1'],
    ['between two pulls', 'p4'],
    ['after the history', 'p5'],
  ]) {
    for (const count of point === 'p2' || point === 'p3' ? [255, 256] : [256]) {
      add(`T-2269 a size let go ${name}, ${count} held`, 80, 24, [
        e('HEADER'),
        ...heldSize(count, point, 80, 24, '\x1b[5n'),
      ])
    }
  }
  // T-2229's finding 7: a region far down a screen as tall as the host accepts, past the cap.
  for (const top of [1024, 2000, 4095]) {
    add(
      'tall screen past cap: a region from row ' + top,
      8,
      4096,
      [e(C2 + top + ';4096r'), { repeat: [0], count: 9 * MIB + 1 }, { reload: true }, e('X')],
      true,
    )
  }
  // T-2284's finding 10: what the journal holds after 257 sizes is still all there.
  add(
    'T-2284 finding 10: KEPT after 257 sizes',
    80,
    24,
    [e('HEADER'), ...Array.from({ length: 257 }, (_, i) => ({ resize: [81 + (i % 2), 24] })), e('KEPT'), { reload: true }, e('X')],
  )
  // T-2284's finding 14: after a cut history a question is answered as the program asked it.
  add(
    'T-2284 finding 14: a cursor report after a cut',
    80,
    24,
    [e('HEADER'), ...Array.from({ length: 257 }, (_, i) => ({ resize: [81 + (i % 2), 24] })), { reload: true }, e(C2 + '4;6H' + C2 + '6n'), e(C2 + '?6n')],
  )
  return cs
}
export function reviewCases(cases) {
  // T-2096's 51: ops with bytes and sizes, a suffix, and a size after it. A reload at the cut.
  return cases.map((c, i) => {
    const ops = []
    for (const op of c.ops) ops.push(op.resize ? { resize: op.resize } : { emit: [...op.bytes] })
    ops.push({ reload: true })
    if (c.suffix.length) ops.push({ emit: [...c.suffix] })
    if (c.afterResize) ops.push({ resize: c.afterResize })
    ops.push({ reload: true })
    return { name: `T-2096 [${c.from}] ${c.name}`, seed: i, cols: c.cols, rows: c.rows, ops }
  })
}
