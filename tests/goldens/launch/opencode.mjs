/**
 * OpenCode's scenarios (`src/adapters/opencode.js`, with
 * `src/channels/opencode.js`, `src/channels.js`, `src/opencode-install.js`,
 * `src/private-integration.js` and `src/role-skills.js`): its launches,
 * fresh, resumed and refused, with the files each leaves (here); the
 * conversation a fresh window opens on, made on a throwaway server
 * (`opencode-creates.mjs`); the first message through the window's own
 * server (`opencode-seeds.mjs`); its looks and its readiness, by what the
 * plugin inside the window says it shows (`opencode-looks.mjs`); and its
 * deliveries through the plugin (`opencode-deliveries.mjs`). The cases of
 * `tests/adapter-opencode.test.mjs`, `tests/opencode-install.test.mjs` and
 * `tests/opencode-launch.test.mjs` that a stand-in can play, and more.
 */
import { createScenarios } from './opencode-creates.mjs'
import { deliveryScenarios } from './opencode-deliveries.mjs'
import { lookScenarios, readyScenarios } from './opencode-looks.mjs'
import {
  chief,
  created,
  ENV,
  installed,
  KEPT,
  LAUNCH,
  launch,
  SECOND,
  worker,
  workspace,
} from './opencode-scenes.mjs'
import { seedScenarios } from './opencode-seeds.mjs'

const FOLDER = `$ROOT/consensflow/integrations/opencode/${LAUNCH}`

/** The refusal of a configuration that is no JSON: Rust's sentence, where V8 threw its own. */
const NO_JSON = {
  why: "a sentence of Rust's own where V8 threw its SyntaxError",
  answer: { throws: 'OpenCode process configuration must be JSON' },
}

export function opencodeScenarios() {
  const prepared = (name, fields, before, { after = [], env = ENV, ...step } = {}) => ({
    name: `opencode: ${name}`,
    harness: 'opencode',
    env,
    steps: [...before, { prepare: launch(fields), ...step }, ...after],
  })
  const fresh = (name, fields, before = [], options = {}) =>
    prepared(name, fields, [installed, workspace, ...before], { ...created(), ...options })
  const keeping = (name, fields, options = {}) =>
    prepared(name, { resume: KEPT, message: null, ...fields }, [installed, workspace], options)
  const refused = (name, fields, before, options = {}) => prepared(name, fields, before, options)
  const plans = [
    fresh(
      'a fresh worker opens on a conversation made first, then on a server of its own with its plugin',
      {},
    ),
    fresh('the chief has no model of its own and no first message', {
      role: 'chief',
      participant: chief,
      agent: null,
      message: null,
    }),
    {
      name: 'opencode: a model alone, an effort alone, and an empty model',
      harness: 'opencode',
      env: ENV,
      steps: [
        installed,
        workspace,
        { prepare: launch({ agent: { kind: 'opencode', model: 'm-1' } }), ...created() },
        { prepare: launch({ agent: { kind: 'opencode', effort: 'low' } }), ...created() },
        {
          prepare: launch({ agent: { kind: 'opencode', model: 'm-1', thinking: 'max' } }),
          ...created(),
        },
        { prepare: launch({ agent: { kind: 'opencode', model: '', effort: '' } }), ...created() },
      ],
    },
    keeping('a conversation OpenCode kept is resumed on its session and nothing is made first', {}),
    keeping('a resumed conversation is given its follow-up, and no model of the roster', {
      message: 'And the tests',
    }),
    {
      name: 'opencode: two launches share one plugin, and each draws ports and tokens of its own',
      harness: 'opencode',
      env: ENV,
      steps: [
        installed,
        workspace,
        { prepare: launch(), ...created() },
        {
          prepare: launch({ launchId: SECOND, participant: { ...worker, handle: 'hera' } }),
          ...created(),
        },
      ],
    },
    refused('OpenCode not installed is refused, and nothing is written', {}, []),
    refused(
      "a plugin that cannot be made refuses the launch in Node's words, before any role is written",
      {},
      [installed, workspace, { write: '$ROOT/consensflow/extensions', text: 'x' }],
    ),
    refused("a ConsensFlow folder that is a file refuses the launch in Node's words", {}, [
      installed,
      workspace,
      { write: '$ROOT/consensflow', text: 'x' },
    ]),
    refused(
      'a window with no role text is refused, after its plugin was made',
      { instructions: '' },
      [installed, workspace],
    ),
    refused(
      'an empty directory is refused, after its plugin and its role were made',
      { directory: '' },
      [installed],
    ),
    refused('a role folder in the way refuses the launch, after its plugin was made', {}, [
      installed,
      workspace,
      { write: `${FOLDER}/role`, text: 'x' },
    ]),
    refused(
      'a settings file of the human’s own refuses the launch before any port or token is drawn',
      {},
      [installed, workspace],
      { env: { ...ENV, OPENCODE_TUI_CONFIG: '$ROOT/home/tui.json' } },
    ),
    fresh('a settings file set to nothing is no settings file', {}, [], {
      env: { ...ENV, OPENCODE_TUI_CONFIG: '' },
    }),
    refused(
      'a conversation of an empty id is refused, after its role was written and its ports drawn',
      { resume: '', message: null },
      [installed, workspace],
      {
        kept: {
          why: "a sentence of Rust's own where V8 threw its TypeError: no ledger holds an empty id",
          answer: { throws: 'an OpenCode window opens on a session id' },
        },
      },
    ),
  ]
  const configured = (name, config, options = {}) =>
    keeping(name, {}, { env: { ...ENV, OPENCODE_CONFIG_CONTENT: config }, ...options })
  const configurations = [
    configured(
      'what the human set is kept, and the role’s folder and file are added to it once',
      '{"theme":"user","skills":{"paths":["/p","/p"],"urls":["u"]},"instructions":["/i"],"z":1,"1":2}',
    ),
    configured('a configuration that is no object is refused', '[]'),
    configured('a configuration that is null is refused', 'null'),
    configured('skill paths that are no list of texts are refused', '{"skills":{"paths":[1]}}'),
    configured(
      'instructions that are no list are refused, a null one too',
      '{"instructions":null}',
    ),
    configured(
      'a skills setting that is a text is spread as JavaScript spreads it',
      '{"skills":"abc"}',
    ),
    configured('a skills setting that is a list is spread by its indexes', '{"skills":["a","b"]}'),
    configured('a configuration set to nothing is none', ''),
    configured('a configuration that is no JSON is refused', 'not json', { kept: NO_JSON }),
    configured('a configuration of one space is no JSON either', ' ', { kept: NO_JSON }),
  ]
  return [
    ...plans,
    ...configurations,
    ...createScenarios(),
    ...seedScenarios(),
    ...lookScenarios(),
    ...readyScenarios(),
    ...deliveryScenarios(),
  ]
}
