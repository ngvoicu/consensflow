/**
 * Pi's scenarios (`src/adapters/pi.js`, with `src/channels/pi.js`,
 * `src/pi-install.js`, `src/private-integration.js` and `src/role-skills.js`):
 * its launches, fresh, resumed and refused, with the extension and the files
 * each leaves (here); its looks and its readiness, by the conversation its
 * extension says the window shows and by Pi's own record (`pi-looks.mjs`);
 * and its deliveries through the extension's inbox (`pi-deliveries.mjs`).
 * The extension's part is played by the scenario: its acknowledgement of a
 * message, and the file naming the conversation its window shows, are `write`
 * steps between `advance` steps. The cases of `tests/adapter-pi.test.mjs` and
 * `tests/pi-install.test.mjs`, and more.
 */
import { deliveryScenarios } from './pi-deliveries.mjs'
import { lookScenarios, readyScenarios } from './pi-looks.mjs'
import {
  chief,
  ENV,
  FOLDER,
  installed,
  launch,
  PUBLISHED,
  SECOND,
  SESSION,
  worker,
} from './pi-scenes.mjs'

export function piScenarios() {
  const prepared = (name, fields, before = [], { after = [], kept } = {}) => ({
    name: `pi: ${name}`,
    harness: 'pi',
    env: ENV,
    steps: [...before, { prepare: launch(fields), ...(kept ? { kept } : {}) }, ...after],
  })
  const plans = [
    prepared(
      'a fresh worker opens on a session of ours, with the extension loaded and the task last',
      {},
      [installed],
      { after: [{ started: {} }] },
    ),
    prepared(
      'the chief has no model of its own and no first message',
      { role: 'chief', participant: chief, agent: null, message: null },
      [installed],
    ),
    prepared(
      'a reviewer is given its task as a window takes text',
      {
        role: 'reviewer',
        message: 'Review\r\nthis \u001b[31mred\u001b[0m line\ttab\u0007bell\u0085',
        agent: { id: 'r', kind: 'pi', model: 'm-1', thinking: '' },
      },
      [installed],
    ),
    {
      name: 'pi: a model alone, a thinking level alone, and an effort never given',
      harness: 'pi',
      env: ENV,
      steps: [
        installed,
        { prepare: launch({ agent: { kind: 'pi', model: 'm-1' } }) },
        { prepare: launch({ agent: { kind: 'pi', thinking: 'low' } }) },
        { prepare: launch({ agent: { kind: 'pi', model: 'm-1', effort: 'max' } }) },
        { prepare: launch({ agent: { kind: 'pi', model: '', thinking: '' } }) },
      ],
    },
    prepared(
      'a conversation Pi kept is resumed on its session and draws nothing',
      { resume: SESSION, message: null },
      [installed],
    ),
    prepared(
      'a resumed conversation is given its follow-up',
      { resume: SESSION, message: 'And the tests' },
      [installed],
    ),
    prepared('a window with no first message', { message: null }, [installed]),
    {
      name: 'pi: two launches share one extension and draw two sessions',
      harness: 'pi',
      env: ENV,
      steps: [
        installed,
        { prepare: launch() },
        { prepare: launch({ launchId: SECOND, participant: { ...worker, handle: 'hera' } }) },
      ],
    },
    prepared('Pi not installed is refused, and nothing is written', {}),
    prepared(
      "an extension that cannot be made refuses the launch in Node's words, before any role is written",
      {},
      [installed, { write: '$ROOT/consensflow/extensions', text: 'x' }],
    ),
    prepared("a ConsensFlow folder that is a file refuses the launch in Node's words", {}, [
      installed,
      { write: '$ROOT/consensflow', text: 'x' },
    ]),
    prepared(
      'a window with no role text is refused, after its extension was made',
      { instructions: '' },
      [installed],
    ),
    prepared('an empty directory is refused, after its extension was made', { directory: '' }, [
      installed,
    ]),
    prepared('a role folder in the way refuses the launch, after its extension was made', {}, [
      installed,
      { write: `${FOLDER}/role`, text: 'x' },
    ]),
    {
      name: 'pi: an extension that differs from this build is refused, and left as it was',
      harness: 'pi',
      env: ENV,
      steps: [
        installed,
        { prepare: launch() },
        { write: PUBLISHED, text: 'damaged' },
        { prepare: launch({ launchId: SECOND }) },
      ],
    },
    {
      name: "pi: an extension that lost its file is refused in Node's words",
      harness: 'pi',
      env: ENV,
      steps: [
        installed,
        { prepare: launch() },
        { remove: PUBLISHED },
        { prepare: launch({ launchId: SECOND }) },
      ],
    },
    prepared(
      'a conversation of an empty id is refused',
      { resume: '', message: null },
      [installed],
      {
        kept: {
          why: "a sentence of Rust's own where V8 threw its TypeError: no ledger holds an empty id",
          answer: { throws: 'a Pi window opens on a session id' },
        },
      },
    ),
  ]
  return [...plans, ...lookScenarios(), ...readyScenarios(), ...deliveryScenarios()]
}
