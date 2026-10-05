/**
 * Devin's launches (`prepare`, `src/devin-install.js`, the Devin branch of
 * `src/role-skills.js`): fresh, resumed and refused, over the owner's config
 * as Devin reads it, with the files each leaves.
 */
import {
  CONFIG,
  chief,
  devin,
  ENV,
  FOLDER,
  launch,
  prepared,
  refusedInWords,
  SECOND,
  SESSION,
  saying,
  THIRD,
  WINDOWS,
  windowsDevin,
} from './devin-fixtures.mjs'

/** Characters a source file spells out, which JavaScript's `\s` holds and Unicode's white space does not, or the other way. */
const NEXT_LINE = String.fromCharCode(0x85)
const LINE_SEPARATOR = String.fromCharCode(0x2028)
const BYTE_ORDER_MARK = String.fromCharCode(0xfeff)

/** A config of the owner's, written where Devin keeps it, and a launch prepared over it. */
const over = (name, text, fields = {}, options = {}) =>
  prepared(name, fields, [devin(), { write: CONFIG, text }], options)

function plans() {
  return [
    prepared(
      'a fresh worker opens on its own config, in full permission, the task in a prompt file',
      {},
      [devin()],
    ),
    prepared(
      'the chief asks the human in its own window, has no model of its own and no first message',
      { role: 'chief', participant: chief, agent: null, message: null },
      [devin()],
    ),
    prepared(
      'a reviewer is given its task as a window takes text',
      {
        role: 'reviewer',
        message: 'Review\r\nthis \u001b[31mred\u001b[0m line\ttab\u0007bell\u0085',
        agent: { id: 'r', kind: 'devin', model: 'claude-opus-5-5', effort: '' },
      },
      [devin()],
    ),
    prepared(
      'an agent that names a family and a level opens on the id that joins them',
      { agent: { kind: 'devin', model: 'claude-opus-5-5', effort: 'max' } },
      [devin()],
    ),
    prepared(
      'an effort with no model opens on no model of its own',
      { agent: { kind: 'devin', effort: 'max' } },
      [devin()],
    ),
    prepared('a window with no first message has no prompt file', { message: null }, [devin()]),
    prepared('a window with an empty first message has none either', { message: '' }, [devin()]),
    prepared(
      'a conversation Devin kept is resumed on its session',
      { resume: SESSION, message: null },
      [devin()],
    ),
    prepared(
      'a resumed conversation is given its follow-up in a prompt file',
      { resume: SESSION, message: 'And the tests' },
      [devin()],
    ),
    prepared(
      'a designer has a role text of its own, in the launch folder',
      {
        role: 'designer',
        instructions: 'DESIGNER TEXT',
      },
      [devin()],
    ),
    prepared(
      'a window on a machine whose environment says Windows is told how to name files there',
      {},
      [windowsDevin()],
      { env: WINDOWS },
    ),
    prepared(
      "a Windows machine's chief is told the same, after the text it was given",
      {
        role: 'chief',
        participant: chief,
        agent: null,
        message: null,
        instructions: '# ConsensFlow chief\n\n| saved-worker | worker | Complex work |',
      },
      [windowsDevin()],
      { env: WINDOWS },
    ),
    {
      name: 'devin: two launches ask the version once, and a Devin updated since is asked again',
      harness: 'devin',
      env: ENV,
      steps: [
        devin(),
        { prepare: launch() },
        { prepare: launch({ launchId: SECOND }) },
        // Updated, and too old now: another file, of another size.
        devin('3000.9.1'),
        { prepare: launch({ launchId: THIRD }) },
      ],
    },
  ]
}

function refusals() {
  return [
    prepared('Devin not installed is refused, and nothing is written', {}),
    prepared(
      'a window with no role text is refused, after its config was written',
      {
        instructions: '',
      },
      [devin()],
    ),
    prepared("a ConsensFlow folder that is a file refuses the launch in Node's words", {}, [
      devin(),
      { write: '$ROOT/consensflow', text: 'x' },
    ]),
    prepared('a role folder in the way refuses the launch, after its config was written', {}, [
      devin(),
      { write: `${FOLDER}/role`, text: 'x' },
    ]),
    prepared(
      'a conversation of an empty id is refused, after its config and role were written',
      { resume: '', message: null },
      [devin()],
      {
        kept: refusedInWords(
          "a sentence of Rust's own where V8 threw its TypeError: no ledger holds an empty id",
          'a Devin window opens on a session id',
        ),
      },
    ),
    {
      name: 'devin: a second window of one launch is refused, its config being there already',
      harness: 'devin',
      env: ENV,
      steps: [
        devin(),
        { prepare: launch() },
        { prepare: launch({ resume: SESSION, message: null }) },
      ],
    },
    prepared(
      'a prompt file already there refuses the launch, after its config and role were written',
      {},
      [devin(), { write: `${FOLDER}/prompt.txt`, text: 'left behind' }],
    ),
  ]
}

/** What each version Devin may answer to `--version` makes of a launch. */
function versions() {
  return [
    ['an older Devin than the minimum is refused, and nothing is written', 'devin 3000.9.1'],
    ['a Devin one patch short of the minimum is refused', 'devin 3000.10.20'],
    ['a Devin of the minimum is accepted', 'devin 3000.10.21'],
    ['a Devin of the next patch is accepted', 'devin 3000.10.22'],
    ['a Devin of the next minor is accepted, however low its patch', 'devin 3000.11.0'],
    ['a Devin of the next major is accepted, however low the rest', 'devin 3001.0.0'],
    ['a Devin of the last major is refused, however high the rest', 'devin 2999.99.99'],
    ['a Devin of a lower minor is refused, however high its patch', 'devin 3000.9.99'],
    ['a version of two numbers is no version', 'devin 3000.10'],
    ['a version stuck to a letter before it is none', 'devin v3000.10.21'],
    ['a version stuck to a letter after it is none', 'devin 3000.10.21abc'],
    ['the first version in what Devin says is its version', 'devin 1.2.3 3000.11.3'],
    ['a Devin that says no version is refused', 'devin'],
    ['a version with a build after it is accepted', 'devin 3000.11.3 (9c803229faa4)'],
    ['a very long number is a very high one', `devin ${'9'.repeat(400)}.0.0`],
  ].map(([name, output]) => prepared(name, {}, [saying(output)]))
}

/** The owner's config as Devin reads it: what cannot be read, and what is read and kept. */
function configs() {
  const unreadable = [
    ['is no JSON', '{not json'],
    ['is empty', ''],
    ['is a list', '[]'],
    ['is text', '"devin"'],
    ['is a number', '7'],
    ['is null', 'null'],
    ['has its hooks a list', '{"hooks": []}'],
    ['has its hooks text', '{"hooks": "x"}'],
    ['has its hooks a number', '{"hooks": 5}'],
    ['has its hooks true', '{"hooks": true}'],
    ['opens on a byte order mark', '\ufeff{}'],
    ['is left unfinished', '{"hooks": {"SessionStart": ['],
    ['has a comment that never closes', '{} /* open'],
    ['has a string that never closes, so what follows it is refused', '{"a": 1} "open /* gone */ '],
  ].map(([name, text]) =>
    over(`an owner config that ${name} is refused, and nothing is written`, text),
  )
  const shapes = [
    [
      'a hook list that is no list is refused, after the folder was made',
      '{"hooks": {"SessionStart": "x"}}',
    ],
    [
      'a hook list of the other name that is no list is refused too',
      '{"hooks": {"PreToolUse": {}}}',
    ],
    ['a hook list that is false is no list', '{"hooks": {"SessionStart": false}}'],
    [
      'a hook list that is null is a list with nothing in it',
      '{"hooks": {"SessionStart": null, "PreToolUse": null}}',
    ],
    ['hooks that are null are none', '{"hooks": null}'],
  ].map(([name, text]) => over(name, text))
  const refusedLikeV8 = [
    ['a zero', '0', 'a number'],
    ['false', 'false', 'a flag'],
    ['empty text', '""', 'text'],
  ].map(([name, text, kind]) =>
    over(
      `hooks that are ${name} are refused, after the folder was made`,
      `{"hooks": ${text}}`,
      {},
      {
        kept: refusedInWords(
          `a sentence of Rust's own where V8 threw its TypeError, on a property it could not make on ${kind}`,
          'the hooks of the native Devin configuration cannot take a hook',
        ),
      },
    ),
  )
  const kept = [
    over(
      'the owner config is kept, its hooks first and ours after, its auto update turned off',
      JSON.stringify({
        theme: 'dark',
        auto_update: true,
        hooks: {
          SessionStart: [{ matcher: 'startup', hooks: [{ type: 'command', command: 'echo hi' }] }],
          PreToolUse: [{ matcher: 'shell', hooks: [{ type: 'command', command: 'echo pre' }] }],
          Stop: [],
        },
      }),
    ),
    over(
      'the chief keeps the owner hooks and adds none for its questions',
      '{"hooks": {"PreToolUse": [{"matcher": "shell", "hooks": []}]}}',
      { role: 'chief', participant: chief, agent: null, message: null },
    ),
    over(
      'comments and trailing commas of the owner config are taken out, and nothing inside a string',
      `// the owner's own
{
  /* where it goes */
  "url": "https://example.com/a//b",
  "quoted": "a \\" // not a comment \\" /* nor this */ b",
  "path": "C:\\\\dir\\\\",
  "comma": "x,}",
  "list": [1, 2, 3,],
  "nested": {"a": [{"b": 1,},], },
  "hooks": {"Stop": [],},
}
// the end`,
    ),
    over(
      'a comma and a gap of any white space before a closer are taken out',
      '{"list": [1,\u00a0\n\t\ufeff], "other": {"a": 1,\r\n\u2028}}',
    ),
    over(
      'a comment of a line runs to its end and no further, a block to its first close',
      '{"a": 1, // one\n"b": /* two */ 2, /* three * / */ "c": 3 /**/, "d": 4}',
    ),
    over('a quote inside a comment opens no string', '{"a": 1 /* "quote */, "b": 2 // it\'s "q\n}'),
    over(
      'a comment holds any character, a next-line mark and a line separator among them',
      `{"a": 1 /* ${NEXT_LINE} ${LINE_SEPARATOR} ${BYTE_ORDER_MARK} */, // ${LINE_SEPARATOR} still one\n"b": 2}`,
    ),
    over(
      'keys that are numbers go first, as JavaScript lists them',
      '{"b": 1, "10": 2, "a": 3, "2": 4}',
    ),
    over(
      'a number past what a double holds is written as the double it reads as',
      '{"big": 12345678901234567890, "small": 1.50, "exp": 1E3}',
    ),
  ]
  const folders = [
    prepared("a config that is a folder is refused in Node's words", {}, [
      devin(),
      { write: `${CONFIG}/inside`, text: 'x' },
    ]),
    prepared("a config folder that is a file is refused in Node's words", {}, [
      devin(),
      { write: '$ROOT/xdg/devin', text: 'x' },
    ]),
  ]
  return [...unreadable, ...shapes, ...refusedLikeV8, ...kept, ...folders]
}

/** Every launch of Devin's: what is prepared, and what is refused. */
export function devinLaunches() {
  return [...plans(), ...refusals(), ...versions(), ...configs()]
}
