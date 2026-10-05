/**
 * A window's first message through its own server (`started`,
 * `src/adapters/opencode.js`, through `seedSession`,
 * `src/channels/opencode.js`): the server's health polled until it answers,
 * the resumed conversation's own model and effort read, and the task posted
 * once, never retried. The server is scripted by route.
 */
import {
  CREATED,
  created,
  ENV,
  HEALTHY,
  installed,
  KEPT,
  launch,
  MESSY,
  MODEL,
  patient,
  settings,
  workspace,
} from './opencode-scenes.mjs'

const HEALTH = 'GET /global/health'
const PROMPT = `POST /session/${CREATED}/prompt_async`
const KEPT_PROMPT = `POST /session/${KEPT}/prompt_async`
const NATIVE = `GET /session/${KEPT}`
const UP = { status: 204 }

/** A server that does not answer: `times` polls, each held until it is given up on. */
const silent = (times) => Array.from({ length: times }, () => ({ held: true }))

/** The agent a window runs on. */
const agent = (fields) => ({ agent: { kind: 'opencode', ...fields } })

/**
 * A window whose first message is seeded, then `steps`: a fresh one, its
 * conversation made first, or one resumed on `KEPT` (`resumed`), whose folder
 * is there unless `folder` says it is not.
 */
const seeding = (name, steps, fields = {}, { resumed = false, folder = [workspace] } = {}) => ({
  name: `opencode: the first message ${name}`,
  harness: 'opencode',
  env: ENV,
  steps: patient([
    installed,
    ...folder,
    resumed
      ? { prepare: launch({ resume: KEPT, message: 'Write the parser', ...fields }) }
      : { prepare: launch(fields), ...created() },
    ...steps,
  ]),
})

/** The task is seeded, the server answering as `served` says. */
const started = (served, step = {}) => ({ started: {}, served, ...step })

/** A server that is up, and takes the task. */
const takes = started({ [HEALTH]: [HEALTHY], [PROMPT]: [UP] })

export function seedScenarios() {
  return [
    seeding('goes through the window’s own server once it is up, with its model and effort', [
      takes,
    ]),
    seeding('is written as a window takes text', [takes], { message: MESSY }),
    seeding('names a provider and a model that holds more slashes', [takes], {
      ...agent({ model: 'openrouter/openai/gpt-6-astra', effort: 'low' }),
    }),
    seeding('of a window with no model and no effort sends neither', [takes], { ...agent({}) }),
    seeding('of a window with an empty model sends none', [takes], {
      ...agent({ model: '', effort: 'high' }),
    }),
    seeding('of a window with an effort of 256 characters is sent', [takes], {
      ...agent({ model: MODEL, effort: 'e'.repeat(256) }),
    }),
    seeding('of a window opened with none submits nothing', [started({})], { message: null }),
    seeding('with nothing in it is refused before any request', [started({})], { message: '' }),
    seeding('is refused for an empty effort before any request', [started({})], {
      ...agent({ model: MODEL, effort: '' }),
    }),
    seeding('is refused for an effort of more than 256 characters', [started({})], {
      ...agent({ model: MODEL, effort: 'e'.repeat(257) }),
    }),
    ...['noslash', '/model', 'provider/'].map((model) =>
      seeding(`is refused for a model that is no provider/model: ${model}`, [started({})], {
        ...agent({ model, effort: 'high' }),
      }),
    ),
    seeding(
      'is refused for a conversation whose id is no OpenCode id',
      [started({})],
      { resume: 'not-an-id' },
      { resumed: true },
    ),
    seeding(
      'is refused where the folder is gone, in Node’s words',
      [started({})],
      { directory: '$ROOT/missing' },
      { resumed: true, folder: [] },
    ),
    seeding('waits for the server to answer, polling every tenth of a second', [
      started({
        [HEALTH]: [{ status: 503, body: '{}' }, { noHead: true }, HEALTHY],
        [PROMPT]: [UP],
      }),
      { advance: 100 },
      { advance: 100 },
    ]),
    seeding('gives up on a poll that stalls at half a second, and tries again', [
      started({ [HEALTH]: [{ held: true }, HEALTHY], [PROMPT]: [UP] }),
      { advance: 499 },
      { advance: 101 },
    ]),
    seeding('gives up on a poll whose body never ends at half a second, though its head said up', [
      started({
        [HEALTH]: [{ status: 200, body: { held: true } }, HEALTHY],
        [PROMPT]: [UP],
      }),
      { advance: 499 },
      { advance: 101 },
    ]),
    seeding('is refused where the server says it is not allowed in', [
      started({ [HEALTH]: [{ status: 401, body: 'unauthorized' }] }),
    ]),
    seeding('is refused where the server forbids it', [
      started({ [HEALTH]: [{ status: 403, body: 'no' }] }),
    ]),
    seeding('is refused for a poll whose body is too large, which is thrown', [
      started({ [HEALTH]: [{ status: 200, body: { repeat: 'x', times: 64 * 1024 + 1 } }] }),
    ]),
    seeding('is refused for a poll whose body is cut off, in undici’s word', [
      started({ [HEALTH]: [{ status: 503, body: { cut: true } }] }),
    ]),
    seeding('reads a poll’s body of exactly 64 kibibytes', [
      started({
        [HEALTH]: [{ status: 200, body: { repeat: 'x', times: 64 * 1024 } }],
        [PROMPT]: [UP],
      }),
    ]),
    seeding('is given up on at sixty seconds if the server never comes up', [
      started({
        [HEALTH]: [{ status: 503, body: '{}' }, { status: 503, body: '{}' }, ...silent(100)],
      }),
      { advance: 60000 },
    ]),
    seeding('is posted once, and never again, whatever came of it', [
      started({ [HEALTH]: [HEALTHY], [PROMPT]: [{ noHead: true }] }),
    ]),
    seeding('is uncertain for a status that is no 204, and says which', [
      started({ [HEALTH]: [HEALTHY], [PROMPT]: [{ status: 500, body: 'oops' }] }),
    ]),
    seeding('is uncertain for a status of 200 too', [
      started({ [HEALTH]: [HEALTHY], [PROMPT]: [{ status: 200, body: '{}' }] }),
    ]),
    seeding('is polled again for a server that answers with a redirect', [
      started({ [HEALTH]: [{ status: 300, body: '{}' }, HEALTHY], [PROMPT]: [UP] }),
      { advance: 100 },
    ]),
    seeding('is uncertain for a post held until the sixty seconds are up', [
      started({ [HEALTH]: [HEALTHY], [PROMPT]: [{ held: true }] }),
      { advance: 60000 },
    ]),
    ...resumedScenarios(),
  ]
}

/** A resumed conversation's settings answered as `native` says. */
const resuming = (name, native, step = {}) =>
  seeding(name, [started({ [HEALTH]: [HEALTHY], [NATIVE]: [native] }, step)], {}, { resumed: true })

/** `value` as the server answers it. */
const json = (value) => ({ status: 200, body: JSON.stringify(value) })

/** A resumed conversation's own model, effort and agent, read and refused. */
function resumedScenarios() {
  const model = (fields) => settings(KEPT, { model: { id: 'm', providerID: 'p', ...fields } })
  const taken = (name, value) =>
    seeding(
      name,
      [started({ [HEALTH]: [HEALTHY], [NATIVE]: [json(value)], [KEPT_PROMPT]: [UP] })],
      {},
      { resumed: true },
    )
  const refused = (name, value) => resuming(name, json(value))
  return [
    taken('of a resumed conversation keeps the model, effort and agent it ran on', settings(KEPT)),
    taken('of a resumed conversation on the native default says the default', model({})),
    taken(
      'of a resumed conversation whose names are 256 characters long are kept',
      settings(KEPT, {
        agent: 'a'.repeat(256),
        model: { id: 'm'.repeat(256), providerID: 'p'.repeat(256), variant: 'v'.repeat(256) },
      }),
    ),
    taken('of a resumed conversation with no agent names none', {
      id: KEPT,
      model: { id: 'm', providerID: 'p' },
    }),
    seeding(
      'of a resumed conversation never looks at the roster’s model or effort',
      [
        started({
          [HEALTH]: [HEALTHY],
          [NATIVE]: [json(settings(KEPT))],
          [KEPT_PROMPT]: [UP],
        }),
      ],
      { ...agent({ model: 'noslash', effort: 'e'.repeat(300) }) },
      { resumed: true },
    ),
    resuming('is refused where the settings cannot be read: a status of 503', {
      status: 503,
      body: '{}',
    }),
    resuming('is refused where the settings come with a redirect, whatever they say', {
      status: 300,
      body: JSON.stringify(settings(KEPT)),
    }),
    resuming(
      'is refused where the settings are no JSON',
      { status: 200, body: 'not json' },
      {
        kept: {
          why: "a sentence of Rust's own where V8 threw its SyntaxError",
          answer: { throws: 'the current OpenCode session settings are no JSON' },
        },
      },
    ),
    resuming(
      'is refused where the settings are JSON of null',
      { status: 200, body: 'null' },
      {
        kept: {
          why: "a sentence of Rust's own where V8 threw its TypeError, which no object is read from",
          answer: { throws: 'OpenCode session has no valid current model and effort' },
        },
      },
    ),
    refused('is refused where the settings name no model', { id: KEPT }),
    refused('is refused where the settings’ model is empty', { id: KEPT, model: {} }),
    refused('is refused where the settings are another conversation’s', settings('ses_other')),
    refused('is refused where the settings’ provider is empty', model({ providerID: '' })),
    refused('is refused where the settings’ model is no text', model({ id: 5 })),
    refused(
      'is refused where the settings’ model is longer than 256',
      model({ id: 'm'.repeat(257) }),
    ),
    refused('is refused where the settings’ variant is null', model({ variant: null })),
    refused('is refused where the settings’ variant is empty', model({ variant: '' })),
    refused('is refused where the settings’ agent is empty', settings(KEPT, { agent: '' })),
    refused('is refused where the settings’ agent is null', settings(KEPT, { agent: null })),
    resuming('is refused where the settings’ body is too large', {
      status: 200,
      body: { repeat: 'x', times: 64 * 1024 + 1 },
    }),
    resuming('is refused where the settings’ body is cut off', {
      status: 200,
      body: { cut: true },
    }),
    resuming('is refused where nobody answers for the settings', { noHead: true }),
    seeding(
      'is given up on at sixty seconds where the settings’ request is held',
      [started({ [HEALTH]: [HEALTHY], [NATIVE]: [{ held: true }] }), { advance: 60000 }],
      {},
      { resumed: true },
    ),
    seeding(
      'is given up on at sixty seconds where the settings’ body is held',
      [
        started({ [HEALTH]: [HEALTHY], [NATIVE]: [{ status: 200, body: { held: true } }] }),
        { advance: 60000 },
      ],
      {},
      { resumed: true },
    ),
  ]
}
