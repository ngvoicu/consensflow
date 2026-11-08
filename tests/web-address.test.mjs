import assert from 'node:assert/strict'
import { test } from 'node:test'

import { QUOTED_ADDRESS, WEB_ADDRESS } from '../app/ui/link-pattern.js'

/**
 * The addresses xterm's web-links addon finds in `text`, as it finds them: the pattern again from
 * the start after each match, over the text of a row (the addon joins the rows a line wrapped over).
 */
const found = (text, pattern = WEB_ADDRESS) =>
  [...text.matchAll(new RegExp(pattern.source, `${pattern.flags}g`))].map((m) => m[0])

// An address is found whole: its parentheses and brackets in pairs, its commas, carets and equals
// signs, any letters of the scheme; what a sentence puts around it is not part of it (T-1307).
const WHOLE = [
  'https://example.com/a',
  'HTTPS://example.com/a',
  'hTtPs://example.com/a',
  'http://example.com',
  'https://xn--bcher-kva.example/a',
  'https://[2001:db8::1]/a',
  'http://[::1]',
  'http://[::1]:8080/a?b=c',
  'http://a@b/a',
  'https://example.com/a,b^c',
  'https://example.com/?x=1,/root,C:/Windows/',
  'https://example.com/%00%0A%0D',
  'https://en.wikipedia.org/wiki/Function_(mathematics)',
  'https://en.wikipedia.org/wiki/Foo_(bar_(baz))',
  'https://en.wikipedia.org/wiki/Foo_(bar_(baz_(qux)))',
  'https://example.com/?a[]=1&a[]=2',
  'https://example.com/t?x=abc==',
  'https://example.com/users/{id}',
  'https://example.com/a|b',
  'https://example.com/~user/',
  'https://example.com/a?b=c&d=e#f-g',
  // An apostrophe is part of an address (T-1315, the review of T-1314): Claude Code printed
  // `https://en.wikipedia.org/wiki/Bayes'_theorem` and the link opened `…/Bayes`.
  "https://en.wikipedia.org/wiki/Bayes'_theorem",
  "https://example.com/o'brien/x",
  "https://example.com/a''b",
  "https://example.com/(it's)",
]
for (const address of WHOLE) {
  test(`${address} is found whole`, () => {
    assert.deepEqual(found(address), [address])
  })
}

const AROUND = [
  ['See https://example.com/a.', ['https://example.com/a']],
  ['https://example.com/a, then', ['https://example.com/a']],
  ['https://example.com/a; and', ['https://example.com/a']],
  ['https://example.com/a: yes', ['https://example.com/a']],
  ['is it https://example.com/a?', ['https://example.com/a']],
  ['wow https://example.com/a!', ['https://example.com/a']],
  ['(https://example.com/a)', ['https://example.com/a']],
  ['[https://example.com/a]', ['https://example.com/a']],
  ['{https://example.com/a}', ['https://example.com/a']],
  ['<https://example.com/a>', ['https://example.com/a']],
  ['"https://example.com/a"', ['https://example.com/a']],
  ["'https://example.com/a'", ['https://example.com/a']],
  ['`https://example.com/a`', ['https://example.com/a']],
  ['“https://example.com/a”', ['https://example.com/a']],
  ['**https://example.com/a**', ['https://example.com/a']],
  ['https://example.com/a…', ['https://example.com/a']],
  ['https://example.com/a...', ['https://example.com/a']],
  ['[docs](https://example.com/a?b=1&c=2)', ['https://example.com/a?b=1&c=2']],
  [
    '(https://en.wikipedia.org/wiki/Function_(mathematics)).',
    ['https://en.wikipedia.org/wiki/Function_(mathematics)'],
  ],
  ['https://example.com/path_(unclosed', ['https://example.com/path_']],
  [
    'https://example.com/x)(https://example.com/y)',
    ['https://example.com/x', 'https://example.com/y'],
  ],
  [
    'https://a.example/x, https://b.example/y; and https://c.example/z!',
    ['https://a.example/x', 'https://b.example/y', 'https://c.example/z'],
  ],
  ['https://example.com/a#frag)', ['https://example.com/a#frag']],
  // An apostrophe is never an address's last character: a quotation round one ends it.
  ["https://example.com/a'", ['https://example.com/a']],
  ["it's https://example.com/a' here", ['https://example.com/a']],
  [
    "(https://en.wikipedia.org/wiki/Bayes'_theorem)",
    ["https://en.wikipedia.org/wiki/Bayes'_theorem"],
  ],
  [
    "'https://example.com/a', 'https://example.com/b'",
    ['https://example.com/a', 'https://example.com/b'],
  ],
]
for (const [text, addresses] of AROUND) {
  test(`${JSON.stringify(text)} holds ${JSON.stringify(addresses)}`, () => {
    assert.deepEqual(found(text), addresses)
  })
}

// An address a quotation mark opens is read with the pattern that ends at an apostrophe
// (`QUOTED_ADDRESS`), which the pane's reader takes for it: `['https://a.example/x','https://b.example/y']`
// is one address to the first pattern, whose apostrophes are inside it, and two to this one.
const QUOTED = [
  ["['https://a.example/x','https://b.example/y']", ['https://a.example/x', 'https://b.example/y']],
  ["'https://example.com/it's'", ['https://example.com/it']],
  ["https://en.wikipedia.org/wiki/Bayes'_theorem", ['https://en.wikipedia.org/wiki/Bayes']],
  ["'https://example.com/a'", ['https://example.com/a']],
]
for (const [text, addresses] of QUOTED) {
  test(`${JSON.stringify(text)} holds ${JSON.stringify(addresses)} where an apostrophe ends an address`, () => {
    assert.deepEqual(found(text, QUOTED_ADDRESS), addresses)
  })
}
test('the pattern that takes an apostrophe for the address’s reads two quoted addresses as one', () => {
  assert.deepEqual(found("['https://a.example/x','https://b.example/y']"), [
    "https://a.example/x','https://b.example/y",
  ])
})

// Not web addresses, or not ones this reads: a scheme of another kind, a bare scheme, and a
// form the addon cannot read as text (a host in letters of its own language, which it compares to
// the one it parsed, and an address without its slashes). The OSC 8 form of those opens.
for (const text of [
  'file:///etc/passwd',
  'javascript:alert(1)',
  'ftp://example.com/a',
  'mailto:me@example.com',
  'https://',
  'https://.',
  'http:example.com/a',
  'example.com/a',
]) {
  test(`${JSON.stringify(text)} holds no address`, () => {
    assert.deepEqual(found(text), [])
  })
}

// A row of brackets that never close, or of nesting past what is read, ends at once: nothing in
// the pattern takes more than a pass over the text however it is built.
for (const text of [
  `https://example.com/${'('.repeat(20_000)}`,
  `https://example.com/${'(a'.repeat(10_000)}`,
  `https://example.com/${'[('.repeat(10_000)}`,
  `https://example.com/${'((((a))))'.repeat(2_000)}`,
  `https://example.com/${"'a".repeat(10_000)}`,
  `https://example.com/${"'".repeat(20_000)}`,
  `https://${'a.'.repeat(10_000)}`,
  'https://example.com/a'.repeat(1_000),
]) {
  test(`a row of ${text.length} characters is read at once`, () => {
    const started = performance.now()
    found(text)
    found(text, QUOTED_ADDRESS)
    assert.ok(performance.now() - started < 500, `${performance.now() - started} ms`)
  })
}
