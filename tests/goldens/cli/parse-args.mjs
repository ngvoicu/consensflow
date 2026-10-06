/**
 * What Node's `util.parseArgs` answers for the words after each verb of
 * bin/cf.mjs, the oracle `cf_base::args` is held to
 * (crates/cf-base/tests/args.rs): the option sets the verbs call it with, and
 * lists of words from the ones people get wrong (a flag given a value, a text
 * option given none or given what looks like an option, a word of one dash, of
 * two, of an equals sign in the wrong place). Each answer is the values and
 * the positionals, or the message of the error it threw, as it goes out on the
 * error output (a lone half of a pair of units is U+FFFD there).
 *
 * The lists are every one of up to two words from a vocabulary, every one of
 * up to three from a few, and each option's every form alone.
 */
import { parseArgs } from 'node:util'

/**
 * The option sets the verbs of bin/cf.mjs pass, whether they take
 * positionals, and the options whose forms the lists combine (the rest are
 * the same code, and are met alone).
 */
export const SPECS = {
  ui: {
    options: {
      json: { type: 'boolean', default: false },
      'no-open': { type: 'boolean', default: false },
    },
    allowPositionals: true,
    focus: ['json', 'no-open'],
  },
  catalog: {
    options: { harness: { type: 'string' }, json: { type: 'boolean', default: false } },
    allowPositionals: true,
    focus: ['harness', 'json'],
  },
  agent: {
    options: {
      harness: { type: 'string' },
      model: { type: 'string' },
      effort: { type: 'string' },
      'work-tier': { type: 'string' },
      description: { type: 'string' },
      designer: { type: 'boolean', default: false },
      json: { type: 'boolean', default: false },
    },
    allowPositionals: true,
    focus: ['harness', 'designer', 'work-tier'],
  },
  setup: { options: {}, allowPositionals: false, focus: [] },
}

/** Words that are no verb's option, and the shapes a word can have. */
const SHAPES = [
  '',
  'x',
  '-',
  '--',
  '-x',
  '-xy',
  '-😀',
  '---x',
  '--x',
  '--x=1',
  '--=a',
  '--=a=b',
  '--😀',
  '--"',
]

/** The words an option makes: bare, given nothing, given something, given what looks like an option. */
const forms = (name) => [`--${name}`, `--${name}=`, `--${name}=v`, `--${name}=--json`]

/** The words every list of up to two is made of, for a verb. */
const vocabulary = ({ focus }) => [
  ...SHAPES,
  ...focus.flatMap(forms),
  ...(focus.length ? ['v'] : []),
]

/** The words every list of three is made of: few, so that three is not a million. */
function core({ focus }) {
  const [first, second] = focus
  return first === undefined
    ? ['x', '-x', '--', '--x']
    : ['x', '-', '--', '-x', `--${first}`, `--${first}=v`, ...(second ? [`--${second}`] : [])]
}

/** Every list of at most `length` words over `words`, shortest first. */
function lists(words, length) {
  const all = [[]]
  let layer = [[]]
  for (let size = 1; size <= length; size++) {
    layer = layer.flatMap((rest) => words.map((word) => [...rest, word]))
    all.push(...layer)
  }
  return all
}

/** What `parseArgs` makes of `args`: its values and positionals, or its message. */
function answer({ options, allowPositionals }, args) {
  try {
    const { values, positionals } = parseArgs({ args, strict: true, options, allowPositionals })
    return { values, positionals }
  } catch (error) {
    return { error: error.message.toWellFormed() }
  }
}

/** Every case: `[verb, args, answer]`, a line of the golden each. */
export function parseCases() {
  const cases = []
  for (const [verb, spec] of Object.entries(SPECS)) {
    const seen = new Set()
    const alone = Object.keys(spec.options).flatMap((name) => forms(name).map((form) => [form]))
    for (const args of [...lists(vocabulary(spec), 2), ...alone, ...lists(core(spec), 3)]) {
      const key = JSON.stringify(args)
      if (seen.has(key)) continue
      seen.add(key)
      cases.push([verb, args, answer(spec, args)])
    }
  }
  return cases
}

/** The golden's text: the option sets, then each case on a line of its own. */
export function parseGolden() {
  const specs = Object.fromEntries(
    Object.entries(SPECS).map(([verb, spec]) => [
      verb,
      {
        options: Object.fromEntries(
          Object.entries(spec.options).map(([name, { type }]) => [name, type]),
        ),
        positionals: spec.allowPositionals,
      },
    ]),
  )
  const lines = parseCases().map((one) => `    ${JSON.stringify(one)}`)
  return `{\n  "specs": ${JSON.stringify(specs)},\n  "cases": [\n${lines.join(',\n')}\n  ]\n}\n`
}
