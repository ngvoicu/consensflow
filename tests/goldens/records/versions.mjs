/**
 * The cases of the harness-version suites (`tests/engine/claude-clear`,
 * `claude-v263`, `claude-v265`, `current-native-versions` and `pi-paths`), as
 * scenarios: a captured transcript or store, cut or changed as each test
 * does, and one fresh look.
 */
import { fixture, fixtureJson, fixtureLines } from './fixtures.mjs'

/** A Claude transcript of `records` at `$ROOT/projects/<folder>`, and one fresh look. */
function claude(name, session, records, folder = 'projects') {
  return {
    name,
    env: { CLAUDE_CONFIG_DIR: '$ROOT' },
    steps: [
      { mkdir: `$ROOT/${folder}` },
      {
        write: `$ROOT/${folder}/${session}.jsonl`,
        text: `${records.map((record) => JSON.stringify(record)).join('\n')}\n`,
      },
      { look: 'fresh', kind: 'claude-code', session },
    ],
  }
}

const records = (name) => fixtureLines(name).map((line) => JSON.parse(line))

export function versionScenarios() {
  return [...clear(), ...v263(), ...v265(), ...current(), ...piPaths()]
}

function clear() {
  const session = 'fb561379-bcab-4045-92d2-d460bb19ed36'
  const examine = (label, mutate = () => {}) => {
    const rows = records('claude-code/v268-clear.jsonl')
    mutate(rows)
    return claude(`claude-clear: ${label}`, session, rows, 'projects/fixture')
  }
  return [
    examine('native Claude 2.1.268 /clear is ready before any model turn'),
    ...[
      ['missing native boundary', (rows) => rows.pop()],
      ['wrong boundary parent', (rows) => (rows.at(-1).parentUuid = 'unrelated-user')],
      ['sidechain boundary', (rows) => (rows.at(-1).isSidechain = true)],
      ['foreign-session boundary', (rows) => (rows.at(-1).sessionId = 'foreign-session')],
      [
        'quoted command-shaped user text',
        (rows) => (rows.at(-2).message.content = `Please explain ${rows.at(-2).message.content}`),
      ],
      [
        'later queued turn',
        (rows) =>
          rows.push({
            type: 'queue-operation',
            operation: 'enqueue',
            content: 'pending real work',
            sessionId: session,
          }),
      ],
      [
        'later real user turn',
        (rows) =>
          rows.push({
            type: 'user',
            uuid: 'next-user',
            sessionId: session,
            message: { role: 'user', content: 'Do real work now' },
          }),
      ],
    ].map(([label, mutate]) => examine(`native /clear cannot settle ${label}`, mutate)),
    examine('a native continuation leaves the predecessor readable, with its completion', (rows) =>
      rows.push({
        type: 'continued-in',
        sessionId: session,
        continuedInSessionId: 'e6acbeb1-181f-4fdc-a963-6ff6d6d792ee',
        timestamp: '2026-09-12T13:57:18.549Z',
      }),
    ),
  ]
}

function v263() {
  const session = '5cbf8973-f472-448a-8763-59fb4268a9d7'
  const stage = (label, take, mutate = (rows) => rows) => {
    const rows = mutate(records('claude-code/v263-tool-loop.jsonl'))
    return claude(`claude-v263: ${label}`, session, rows.slice(0, take ?? rows.length))
  }
  return [
    stage('the installed 2.1.263 transcript is admitted, whole'),
    ...[8, 9, 19, 27].map((take) => stage(`a prefix of ${take} records stays unready`, take)),
    ...['pendingBackgroundAgentCount', 'pendingWorkflowCount'].flatMap((field) =>
      [1, -1, '0', null].map((count) =>
        stage(
          `turn_duration with ${field} ${JSON.stringify(count)} cannot settle a result`,
          undefined,
          (rows) =>
            rows.map((record) =>
              record.subtype === 'turn_duration' ? { ...record, [field]: count } : record,
            ),
        ),
      ),
    ),
    stage('a sidechain duration cannot close the root turn', undefined, (rows) =>
      rows.map((record) =>
        record.subtype === 'turn_duration' ? { ...record, isSidechain: true } : record,
      ),
    ),
  ]
}

function v265() {
  const session = '17499106-8778-48e1-a306-87bd186c9f7e'
  const all = records('claude-code/v265-tool-loop.jsonl')
  const read = (label, take = all.length, mutate = (record) => record) =>
    claude(`claude-v265: ${label}`, session, all.slice(0, take).map(mutate))
  return [
    ...[1, 2, 3, 4, 5, 6, 7, 8].map((take) => read(`a prefix of ${take} records`, take)),
    read('the whole transcript'),
    ...[{ isSidechain: true }, { pendingBackgroundAgentCount: 1 }, { pendingWorkflowCount: 1 }].map(
      (fields) =>
        read(
          `a duration with ${JSON.stringify(fields)} cannot settle the root`,
          undefined,
          (record) => (record.subtype === 'turn_duration' ? { ...record, ...fields } : record),
        ),
    ),
    read('settlement ignores future version metadata', undefined, (record) => ({
      ...record,
      version: '2.1.999',
    })),
  ]
}

function current() {
  const rows = records('claude-code/v266-tool-loop.jsonl')
  const session = rows[0].sessionId
  const v130 = fixtureJson('opencode/v130-tool-loop.json')
  const tables = Object.entries(v130).flatMap(([table, tableRows]) => {
    const keys = Object.keys(tableRows[0])
    return [
      {
        db: 'stage',
        exec: `CREATE TABLE ${table} (${keys.map((key) => `"${key}" ${typeof tableRows[0][key] === 'number' ? 'INTEGER' : 'TEXT'}`).join(', ')})`,
      },
      ...tableRows.map((row) => ({
        db: 'stage',
        run: `INSERT INTO ${table} (${keys.map((key) => `"${key}"`).join(', ')}) VALUES (${keys.map(() => '?').join(', ')})`,
        params: keys.map((key) => row[key]),
      })),
    ]
  })
  return [
    ...[rows.length - 1, rows.length].map((n) =>
      claude(
        `current versions: Claude 2.1.266, ${n} of ${rows.length} records`,
        session,
        rows.slice(0, n),
      ),
    ),
    {
      name: 'current versions: OpenCode 1.18.30 uses captured native event order and final completion metadata',
      env: { XDG_DATA_HOME: '$ROOT' },
      steps: [
        { mkdir: '$ROOT/opencode' },
        { db: 'stage', open: '$ROOT/opencode/opencode.db' },
        ...tables,
        { db: 'stage', close: true },
        { look: 'fresh', kind: 'opencode', session: v130.session[0].id },
      ],
    },
  ]
}

function piPaths() {
  const sessionFile = '2026-08-24T18-00-00-000Z_hazy-ridge.jsonl'
  const decoyRecords = [
    { type: 'session', version: 3, id: 'hazy-ridge', timestamp: '2026-08-24T18:00:00.703Z' },
    {
      type: 'message',
      id: 'default-decoy-user',
      parentId: null,
      timestamp: '2026-08-24T18:00:00.935Z',
      message: {
        role: 'user',
        content: [{ type: 'text', text: 'DEFAULT_PI_PATH_DECOY' }],
        timestamp: 1787594400933,
      },
    },
  ]
  const at = (sessionRoot, text) => [
    { mkdir: `$ROOT/${sessionRoot}/project` },
    { write: `$ROOT/${sessionRoot}/project/${sessionFile}`, text },
  ]
  const real = (sessionRoot) => at(sessionRoot, fixture('pi/tool-loop.jsonl'))
  const decoy = (sessionRoot) =>
    at(sessionRoot, `${decoyRecords.map((record) => JSON.stringify(record)).join('\n')}\n`)
  const scenario = (name, env, steps) => ({
    name: `pi paths: ${name}`,
    env: { HOME: '$ROOT', ...env },
    steps: [...steps, { look: 'fresh', kind: 'pi', session: 'hazy-ridge' }],
  })
  return [
    scenario(
      'tilde-expanded PI_CODING_AGENT_DIR drives the reader',
      { PI_CODING_AGENT_DIR: '~/pi-custom' },
      [...real('pi-custom/sessions'), ...decoy('.pi/agent/sessions')],
    ),
    scenario(
      'PI_CODING_AGENT_SESSION_DIR takes precedence over PI_CODING_AGENT_DIR',
      { PI_CODING_AGENT_DIR: '~/pi-config', PI_CODING_AGENT_SESSION_DIR: '~/pi-sessions' },
      [...real('pi-sessions'), ...decoy('pi-config/sessions'), ...decoy('.pi/agent/sessions')],
    ),
    scenario(
      'empty path variables fall back to the default Pi session directory',
      { PI_CODING_AGENT_DIR: '', PI_CODING_AGENT_SESSION_DIR: '' },
      real('.pi/agent/sessions'),
    ),
    scenario(
      'an empty PI_CODING_AGENT_SESSION_DIR falls back under the custom agent directory',
      { PI_CODING_AGENT_DIR: '~/pi-custom', PI_CODING_AGENT_SESSION_DIR: '' },
      [...real('pi-custom/sessions'), ...decoy('.pi/agent/sessions')],
    ),
  ]
}
