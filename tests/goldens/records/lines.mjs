/**
 * `JSON.parse` over the bytes of a line, kept as a schema says
 * (`cf_base::json::Keep`): what Node reads of each line, member by member,
 * for the reader that builds only some of a line to be held to.
 *
 * Each case is the bytes of a line (any bytes, so base64), the schema it is
 * kept by, and either `kept`, `JSON.stringify` of what Node's parse holds of
 * it under the schema, or `throws`. Node decodes the bytes as UTF-8 and reads
 * invalid bytes as U+FFFD; a lone surrogate it keeps is U+FFFD here (the
 * reader reads it so, on purpose), so `kept` is made well-formed first.
 *
 * Two kinds of line Node reads and the Rust reader does not, on purpose, are
 * marked in `differs`, and the Rust reader is held to failing them: a line
 * nested past 127 levels (`deep`), and one with a number past a double's
 * range, which Node reads as `Infinity` (`overflow`).
 */

/** A schema: `'all'`, `'scalar'`, `{ members: { name: schema } }` or `{ items: schema }`. */
const KEEPS = {
  all: 'all',
  scalar: 'scalar',
  record: {
    members: {
      type: 'scalar',
      timestamp: 'all',
      message: {
        members: {
          id: 'scalar',
          content: { items: { members: { type: 'scalar', text: 'scalar', content: 'all' } } },
        },
      },
    },
  },
  keyed: {
    members: { a: 'scalar', b: 'all', 1: 'scalar', 2: 'scalar', o: { members: { a: 'all' } } },
  },
  list: { items: 'scalar' },
}

/** What a parse of a line holds under a schema, as `Keep` says. */
function keepIn(value, keep) {
  if (keep === 'all') return value
  if (Array.isArray(value)) return keep.items ? value.map((item) => keepIn(item, keep.items)) : []
  if (value !== null && typeof value === 'object') {
    if (!keep.members) return {}
    return Object.fromEntries(
      Object.keys(value)
        .filter((key) => Object.hasOwn(keep.members, key))
        .map((key) => [key, keepIn(value[key], keep.members[key])]),
    )
  }
  return value
}

/** `value` with every lone surrogate of its texts and keys read as U+FFFD. */
function wellFormed(value) {
  if (typeof value === 'string') return value.toWellFormed()
  if (Array.isArray(value)) return value.map(wellFormed)
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.keys(value).map((key) => [key.toWellFormed(), wellFormed(value[key])]),
    )
  }
  return value
}

/** How many levels of lists and objects `value` nests, its own the first. */
function nesting(value) {
  let deepest = 0
  const stack = [[value, 1]]
  while (stack.length > 0) {
    const [next, level] = stack.pop()
    if (next === null || typeof next !== 'object') continue
    deepest = Math.max(deepest, level)
    for (const inner of Object.values(next)) stack.push([inner, level + 1])
  }
  return deepest
}

/** Whether a number of `value` is none a double holds: Node read it as an infinity. */
function overflows(value) {
  const stack = [value]
  while (stack.length > 0) {
    const next = stack.pop()
    if (typeof next === 'number' && !Number.isFinite(next)) return true
    if (next !== null && typeof next === 'object') stack.push(...Object.values(next))
  }
  return false
}

/** The bytes of a case, from its text or its bytes. */
const bytesOf = (line) => (typeof line === 'string' ? Buffer.from(line, 'utf8') : Buffer.from(line))

/** A list of `depth` levels. */
const nested = (depth, inner = '') => `${'['.repeat(depth)}${inner}${']'.repeat(depth)}`

const CASES = [
  // A key said twice is the last value in the place of the first.
  ['a key twice', 'keyed', '{"a":1,"b":2,"a":3}'],
  ['a key twice, an object each time', 'keyed', '{"o":{"a":1},"b":2,"o":{"a":2,"c":3}}'],
  ['a key twice, of another type', 'keyed', '{"a":[1],"b":0,"a":"s","b":null}'],
  ['a key twice, by an escape', 'keyed', '{"a":1,"b":2,"\\u0061":3}'],
  ['a key twice, an index', 'all', '{"1":"a","b":1,"1":"c","2":0}'],
  ['a key twice, nested whole', 'all', '{"o":{"x":1,"y":2,"x":3},"o":{"z":0,"z":4}}'],
  ['a key twice, dropped', 'keyed', '{"zz":[1],"a":1,"zz":{"deep":[[]]},"a":2}'],
  [
    'a key twice, in a record',
    'record',
    '{"type":"a","message":{"id":"x"},"type":"b","message":{"content":"s"}}',
  ],
  // Keys enumerate as JavaScript enumerates them: indices first, ascending.
  [
    'keys, indices first',
    'all',
    '{"b":1,"2":2,"a":3,"1":4,"01":5,"4294967294":6,"4294967295":7,"-1":8,"10":9}',
  ],
  ['keys, kept in order', 'keyed', '{"b":1,"2":2,"a":3,"1":4}'],
  ['keys, nested', 'all', '{"z":{"b":0,"10":1,"9":2},"y":[{"2":0,"1":1}]}'],
  [
    'a key proper to an object',
    'all',
    '{"__proto__":1,"constructor":2,"toString":3,"hasOwnProperty":4}',
  ],
  ['a key proper to an object, kept', 'keyed', '{"a":1,"__proto__":{"a":2}}'],
  // Numbers are the doubles they read as, and are written as JavaScript writes them.
  [
    'numbers, small and plain',
    'all',
    '[0,-0,1,-1,1.0,1.5,100,2e0,0e5,-0.0,0.1,0.000001,0.0000001]',
  ],
  ['numbers, large', 'all', '[1e21,1E+21,1e22,1e2,100e-2,123456789012345678901234567890,1e300]'],
  [
    'numbers, past 2^53',
    'all',
    '[9007199254740991,9007199254740992,9007199254740993,-9007199254740993,18446744073709551615,18446744073709551616]',
  ],
  [
    'numbers, at the edges',
    'all',
    '[5e-324,4.9406564584124654e-324,2.2250738585072014e-308,1.7976931348623157e308,1e-400,-1e-400,2.5e-324]',
  ],
  [
    'numbers, that need their digits',
    'all',
    '[0.30000000000000004,1000000000000000.1,0.1e1,1.7976931348623157000e308,9007199254740993.0]',
  ],
  ['numbers, kept alone', 'scalar', '-9007199254740993'],
  ['numbers, in a record', 'record', '{"type":9007199254740993,"timestamp":[1e21,0.5,{"n":1.0}]}'],
  ['numbers, in a list', 'list', '[1e21,2.50,1e-7,{"a":1},[2]]'],
  // Invalid UTF-8 is U+FFFD.
  ['bytes, one that begins nothing', 'all', [0x5b, 0x22, 0xff, 0x22, 0x5d]],
  ['bytes, a continuation without its start', 'all', [0x5b, 0x22, 0x80, 0xbf, 0x22, 0x5d]],
  ['bytes, a start without its end', 'all', [0x5b, 0x22, 0xe2, 0x82, 0x22, 0x5d]],
  ['bytes, an emoji cut', 'all', [0x5b, 0x22, 0xf0, 0x9f, 0x98, 0x22, 0x5d]],
  ['bytes, a surrogate in three bytes', 'all', [0x5b, 0x22, 0xed, 0xa0, 0x80, 0x22, 0x5d]],
  ['bytes, an overlong slash', 'all', [0x5b, 0x22, 0xc0, 0xaf, 0x22, 0x5d]],
  ['bytes, past U+10FFFF', 'all', [0x5b, 0x22, 0xf4, 0x90, 0x80, 0x80, 0x22, 0x5d]],
  ['bytes, five-byte start', 'all', [0x5b, 0x22, 0xf8, 0x88, 0x80, 0x80, 0x80, 0x22, 0x5d]],
  ['bytes, in a key', 'all', [0x7b, 0x22, 0xff, 0x22, 0x3a, 0x31, 0x7d]],
  [
    'bytes, between good ones',
    'scalar',
    [0x22, 0x61, 0xc3, 0x28, 0xe6, 0x97, 0xa5, 0xff, 0x62, 0x22],
  ],
  [
    'bytes, in what is dropped',
    'keyed',
    [0x7b, 0x22, 0x7a, 0x22, 0x3a, 0x22, 0xff, 0x22, 0x2c, 0x22, 0x61, 0x22, 0x3a, 0x31, 0x7d],
  ],
  // A lone surrogate's escape is U+FFFD; a pair, and an escaped backslash, are as written.
  ['surrogates, a high alone', 'all', '["\\ud800"]'],
  ['surrogates, a low alone', 'all', '["\\udc00"]'],
  ['surrogates, a high then text', 'all', '["\\ud800x","x\\udc00","\\ud800\\u0041"]'],
  ['surrogates, a pair', 'all', '["\\ud83d\\ude00","\\uD83D\\uDE00"]'],
  ['surrogates, a pair reversed', 'all', '["\\ude00\\ud83d"]'],
  ['surrogates, a lone one before a pair', 'all', '["\\ud83d\\ud83d\\ude00"]'],
  ['surrogates, an escaped backslash', 'all', '["\\\\ud83d","\\\\ud800\\\\udc00"]'],
  ['surrogates, in a key', 'all', '{"\\ud800":1}'],
  ['surrogates, kept alone', 'scalar', '"a\\udfffb"'],
  ['surrogates, a pair written raw', 'all', '["😀","\\u2028\u2028\\u2029"]'],
  // Text of other kinds.
  ['texts, escapes', 'all', '["\\/\\b\\f\\n\\r\\t\\"\\\\\\u0000\\u001f\\u007f\\u00e9"]'],
  ['texts, raw and long', 'all', `["日本語 😀 é ","${'x'.repeat(5000)}"]`],
  ['white space', 'all', ' \t\r\n{ "a" : [ 1 , 2 ] , "b" : { } }\n\r\t '],
  // Lines that are not JSON, wherever they are wrong.
  ['malformed, empty', 'all', ''],
  ['malformed, white space', 'all', ' \n'],
  ['malformed, a byte order mark', 'all', '\ufeff{}'],
  ['malformed, a trailing comma', 'all', '{"a":1,}'],
  ['malformed, a list with a trailing comma', 'all', '[1,2,]'],
  ['malformed, single quotes', 'all', "{'a':1}"],
  ['malformed, a leading zero', 'all', '[01]'],
  ['malformed, a plus', 'all', '[+1]'],
  ['malformed, a leading dot', 'all', '[.5]'],
  ['malformed, a trailing dot', 'all', '[1.]'],
  ['malformed, an exponent without digits', 'all', '[1e]'],
  ['malformed, NaN', 'all', '[NaN]'],
  ['malformed, Infinity', 'all', '[Infinity]'],
  ['malformed, undefined', 'all', '[undefined]'],
  ['malformed, a comment', 'all', '{"a":1/*x*/}'],
  ['malformed, a raw line break in a text', 'all', '["a\nb"]'],
  ['malformed, a raw tab in a text', 'all', '["a\tb"]'],
  ['malformed, an escape that is none', 'all', '["\\x"]'],
  ['malformed, a short unicode escape', 'all', '["\\u12"]'],
  ['malformed, an unterminated text', 'all', '{"a":"b'],
  ['malformed, an unterminated object', 'all', '{"a":1'],
  ['malformed, an unterminated list', 'all', '[1,2'],
  ['malformed, a missing colon', 'all', '{"a" 1}'],
  ['malformed, a key that is no text', 'all', '{1:2}'],
  ['malformed, two values', 'all', '{"a":1}{"b":2}'],
  ['malformed, text after the value', 'all', '{"a":1} x'],
  ['malformed, a close too many', 'all', '[1]]'],
  ['malformed, in what is dropped', 'keyed', '{"zz":[1,}'],
  ['malformed, a number in what is dropped', 'keyed', '{"zz":01,"a":1}'],
  ['malformed, past a number that overflows', 'keyed', '{"zz":1e999,"a":}'],
  ['malformed, past the depth', 'keyed', `{"zz":${nested(200)} x}`],
  // Nesting. Node reads any; the Rust reader reads 127 levels.
  ['nesting, 126 levels', 'keyed', `{"zz":${nested(125)}}`],
  ['nesting, 127 levels', 'keyed', `{"zz":${nested(126)}}`],
  ['nesting, 128 levels', 'keyed', `{"zz":${nested(127)}}`],
  ['nesting, 127 levels of lists', 'all', nested(127)],
  ['nesting, 128 levels of lists', 'all', nested(128)],
  ['nesting, 128 levels in what is kept', 'all', `{"a":${nested(127)}}`],
  ['nesting, objects to 127 levels', 'all', `${'{"a":'.repeat(127)}1${'}'.repeat(127)}`],
  ['nesting, objects to 128 levels', 'all', `${'{"a":'.repeat(128)}1${'}'.repeat(128)}`],
  ['nesting, 5000 levels', 'keyed', `{"zz":${nested(5000)}}`],
  // A number past a double's range is infinite to Node.
  ['overflow, a number', 'all', '1e400'],
  ['overflow, a negative one', 'all', '[-1e400]'],
  ['overflow, in what is dropped', 'keyed', '{"zz":[0,{"n":1e999}],"a":1}'],
  ['overflow, in what is kept', 'record', '{"type":"a","timestamp":{"n":1e309}}'],
  ['overflow, a scalar', 'record', '{"type":1e309}'],
  ['overflow, just past the edge', 'all', '[1.797693134862316e308]'],
  ['overflow, long digits', 'keyed', `{"zz":1${'0'.repeat(400)}}`],
  // Lines like a transcript's.
  [
    'a record',
    'record',
    '{"type":"assistant","uuid":"u1","message":{"id":"m1","role":"assistant","content":[{"type":"thinking","thinking":"hm","signature":"s"},{"type":"text","text":"Hi \\"there\\""},{"type":"tool_use","id":"t1","input":{"x":[1,2,{"y":null}]}}],"usage":{"input_tokens":3,"iterations":[{"a":1}]}},"toolUseResult":{"stdout":"x"},"timestamp":"2026-09-19T10:00:00.000Z"}',
  ],
  [
    'a record, its content a text',
    'record',
    '{"type":"user","message":{"id":"m","content":"plain"},"timestamp":17.5}',
  ],
  [
    'a record, its message no object',
    'record',
    '{"type":"user","message":[1,{"id":"x"}],"timestamp":null}',
  ],
  ['a record, its content no list', 'record', '{"message":{"content":{"type":"text","text":"x"}}}'],
  [
    'a record, its blocks of every kind',
    'record',
    '{"message":{"content":[null,1,"s",[2],{"type":["x"],"text":{"y":1}},{"content":[{"a":1}]}]}}',
  ],
]

/** The table: the schemas, and for each case the bytes and what Node made of them. */
export function linesTable() {
  const cases = CASES.map(([name, keep, line]) => {
    const bytes = bytesOf(line)
    const text = bytes.toString('utf8')
    const entry = { name, keep, bytes: bytes.toString('base64') }
    let parsed
    try {
      parsed = JSON.parse(text)
    } catch {
      return { ...entry, node: { throws: true } }
    }
    const differs = nesting(parsed) > 127 ? 'deep' : overflows(parsed) ? 'overflow' : undefined
    const kept = JSON.stringify(wellFormed(keepIn(parsed, KEEPS[keep])))
    return { ...entry, node: { kept }, ...(differs === undefined ? {} : { differs }) }
  })
  return { keeps: KEEPS, cases }
}
