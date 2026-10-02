import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { PassThrough } from 'node:stream'
import { describe, it } from 'node:test'
import { admission } from '../src/adapters/shared.js'
import { Bridge } from '../src/bridge.js'
import { send as sendCodex } from '../src/channels/codex.js'
import { send as sendDevin } from '../src/channels/devin.js'
import { send as sendOpenCode } from '../src/channels/opencode.js'
import { send as sendPi } from '../src/channels/pi.js'
import { writePaste } from '../src/channels/pty.js'

/**
 * One contract for every channel's answer to a send (the owner's rule,
 * 2026-10-02): every harness treats a message that may have been sent the
 * same way. Each channel has a handover point (Pi's inbox rename, a paste's
 * first byte, Codex's and OpenCode's POST):
 * - refused before it, nothing reached the harness: `admitted: false` with
 *   `bytesWritten: 0`, and the dispatcher may send again at once;
 * - any error at or after it may have reached the harness: `admitted: null`,
 *   and the dispatcher waits for the harness's own record, sending again only
 *   if the record never shows the message in time;
 * - accepted: `admitted: true`.
 * Every channel runs every row through its real send function, against
 * stand-ins for the pane host and the harness side, and the adapters read
 * each answer the same way (`admission()`): a refusal read into an answer
 * that does not say one is a message delivered twice.
 */
const ROWS = {
  refused: { says: 'refused before its handover point', admitted: false },
  uncertain: { says: 'an error at or after its handover point', admitted: null },
  accepted: { says: 'accepted', admitted: true },
}

/**
 * The pane host at the other end of a real bridge: it admits a claim and
 * writes a paste, or answers either as told. A paste it never answers meets
 * the bridge's own deadline; `end` closes the host's side of the bridge.
 */
function paneHost(
  t,
  { claim = { ok: true }, paste = () => ({ ok: true }), deadlineMs = 5_000 } = {},
) {
  const toHost = new PassThrough()
  const toDaemon = new PassThrough()
  const daemon = new Bridge({ input: toDaemon, output: toHost, defaultDeadlineMs: deadlineMs })
  const host = new Bridge({ input: toHost, output: toDaemon, idPrefix: 'r-', peerIdPrefix: 'n-' })
  host.on('pane.claim', () => claim)
  host.on('pane.write_paste', () => paste({ end: () => toDaemon.end() }))
  t.after(() => {
    daemon.close()
    host.close()
  })
  return daemon
}

/** The pane host's answer to a paste it refused before writing a byte of it. */
const PASTE_REFUSED = {
  ok: false,
  admitted: false,
  bytesWritten: 0,
  error: 'stale-generation',
  cause: 'p1-zeus is at generation 4 now',
}
/** Its answer to a paste whose write failed once bytes may have gone out. */
const PASTE_UNCERTAIN = {
  ok: false,
  admitted: null,
  error: 'uncertain',
  cause: 'the pane input failed partway',
}
const never = () => new Promise(() => {})

/**
 * The rows of a paste into a window (Claude Code, Devin), whose handover
 * point is its first byte.
 */
function pasted(send, refused = {}) {
  return {
    refused: {
      ...refused,
      'the pane host refuses the paste before writing a byte': (t) =>
        send(t, { paste: () => PASTE_REFUSED }),
    },
    uncertain: {
      'the pane host fails the paste after writing bytes': (t) =>
        send(t, { paste: () => PASTE_UNCERTAIN }),
      "the bridge's own deadline passes before the host answers": (t) =>
        send(t, { paste: never, deadlineMs: 50 }),
      'the pane host goes away before it answers': (t) =>
        send(t, {
          paste: ({ end }) => {
            end()
            return never()
          },
        }),
    },
    accepted: {
      'the pane host writes the paste': (t) => send(t, {}),
    },
  }
}

/**
 * A harness's own server for a message (Codex's broker, OpenCode's plugin):
 * it reads the POST, then answers as told, or drops the connection.
 */
async function harnessServer(t, { answer = { ok: true, admitted: true }, drop = false }) {
  const server = createServer(async (request, response) => {
    for await (const _chunk of request);
    if (drop) return request.socket.destroy()
    response.writeHead(200, { 'content-type': 'application/json' })
    response.end(JSON.stringify(answer))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(async () => {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
  })
  return { endpoint: `http://127.0.0.1:${server.address().port}`, token: 't'.repeat(32) }
}

/** The rows of a message posted to a harness's own server, whose handover point is the POST. */
function posted(send, server) {
  return {
    refused: {
      'the pane host refuses its claim': (t) =>
        send(t, { claim: { ok: false, error: 'the pane is gone' } }),
      [`${server} refuses it`]: (t) =>
        send(t, {
          answer: { ok: false, admitted: false, bytesWritten: 0, error: 'native-session-changed' },
        }),
    },
    uncertain: {
      [`${server} drops the connection after the POST`]: (t) => send(t, { drop: true }),
    },
    accepted: {
      [`${server} takes it`]: (t) => send(t, {}),
    },
  }
}

async function sendThroughCodex(t, { claim, ...server }) {
  return sendCodex(
    {
      session: '01a0817b-e6b0-7f32-8e11-370dc000cbc0',
      pane: 'p1-diana',
      generation: 3,
      deadlineMs: 3_000,
      bridge: paneHost(t, { claim }),
      launch: {
        kind: 'codex-queue',
        launchId: 'launch-codex',
        sessionBridge: await harnessServer(t, server),
      },
    },
    'the cache key is per conversation',
  )
}

async function sendThroughOpenCode(t, { claim, ...server }) {
  return sendOpenCode(
    {
      session: 'ses_contract1',
      pane: 'p1-hera',
      generation: 3,
      bridge: paneHost(t, { claim }),
      launch: {
        channel: {
          kind: 'opencode-server',
          launchId: 'launch-opencode',
          sessionBridge: await harnessServer(t, server),
        },
      },
    },
    'the cache key is per conversation',
  )
}

const DEVIN_SESSION = 'b7c2a0f4-5d1e-4a8b-9c3f-1e2d3c4b5a69'

/** Devin's own wire log, naming the conversation its window shows, when Devin wrote one. */
async function sendThroughDevin(t, host, { shows = DEVIN_SESSION, logged = true } = {}) {
  const root = await mkdtemp(join(tmpdir(), 'cf-contract-devin-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const wire = join(root, 'wire.jsonl')
  const selected = {
    sessionId: shows,
    update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'mode' }] },
  }
  if (logged) await writeFile(wire, `${JSON.stringify(selected)}\n`)
  return sendDevin(
    {
      channel: { wire },
      session: DEVIN_SESSION,
      bridge: paneHost(t, host),
      pane: 'p1-hera',
      generation: 3,
    },
    'the cache key is per conversation',
  )
}

/**
 * Pi's side of its inbox: ConsensFlow's extension takes each record in turn
 * and answers it at its acknowledgement path, as `answer` says.
 */
async function piWindow(t, answer) {
  const root = await mkdtemp(join(tmpdir(), 'cf-contract-pi-'))
  const inbox = join(root, 'inbox')
  const ack = join(root, 'ack')
  let pass = Promise.resolve()
  let failed = null
  const extension = setInterval(() => {
    pass = pass
      .then(async () => {
        const names = await readdir(inbox).catch(() => [])
        for (const name of names.filter((n) => n.endsWith('.json'))) {
          const { id } = JSON.parse(await readFile(join(inbox, name), 'utf8'))
          await mkdir(ack, { recursive: true })
          await answer(join(ack, `${id}.json`), id)
          await rm(join(inbox, name), { force: true })
        }
      })
      .catch((cause) => {
        failed ??= cause
      })
  }, 5)
  t.after(async () => {
    clearInterval(extension)
    await pass
    await rm(root, { recursive: true, force: true })
    if (failed !== null) throw failed
  })
  return { root, inbox, ack }
}

async function sendThroughPi(
  t,
  { answer = async () => {}, claim, ackTimeoutMs = 5_000, inbox } = {},
) {
  const window = await piWindow(t, answer)
  return sendPi(
    {
      session: 'cf-1-zeus-0000abcd',
      pane: 'p1-zeus',
      generation: 3,
      bridge: paneHost(t, { claim }),
      launch: {
        channel: {
          kind: 'pi-extension',
          launchId: 'launch-pi',
          inbox: inbox === undefined ? window.inbox : await inbox(window),
          ack: window.ack,
          ackTimeoutMs,
        },
      },
    },
    'the cache key is per conversation',
  )
}

const acknowledge = (fields) => async (path, id) =>
  writeFile(path, JSON.stringify({ id, ...fields }))

const CHANNELS = {
  'Claude Code, pasted into its window': pasted((t, host) =>
    writePaste(
      paneHost(t, host),
      { id: 'p1-zeus', generation: 3 },
      'the cache key is per conversation',
    ),
  ),
  'Devin, pasted into its window': pasted((t, host) => sendThroughDevin(t, host), {
    'Devin shows another conversation': (t) =>
      sendThroughDevin(t, {}, { shows: '0e9d8c7b-6a5f-4e3d-8c2b-1a0f9e8d7c6b' }),
    'Devin has no wire log to say which conversation it shows': (t) =>
      sendThroughDevin(t, {}, { logged: false }),
  }),
  'Codex, through its broker': posted(sendThroughCodex, 'the broker'),
  'OpenCode, through its plugin': posted(sendThroughOpenCode, 'the plugin'),
  'Pi, through its extension inbox': {
    refused: {
      'the pane host refuses its claim': (t) =>
        sendThroughPi(t, { claim: { ok: false, error: 'pane p1-zeus is gone' } }),
      'its inbox cannot be written': (t) =>
        sendThroughPi(t, {
          inbox: async (window) => {
            const file = join(window.root, 'not-a-folder')
            await writeFile(file, '')
            return file
          },
        }),
      'the extension refuses it before sending': (t) =>
        sendThroughPi(t, {
          answer: acknowledge({ admitted: false, bytesWritten: 0, reason: 'chief busy' }),
        }),
    },
    uncertain: {
      // The case a loaded CI runner hit (2026-10-02): something threw while
      // the channel waited for the acknowledgement, after the record was in.
      'an error after the inbox rename': (t) =>
        sendThroughPi(t, { answer: (path) => mkdir(path, { recursive: true }) }),
      'no acknowledgement before the record expires': (t) =>
        sendThroughPi(t, { ackTimeoutMs: 100 }),
    },
    accepted: {
      'the extension shows it in Pi': (t) =>
        sendThroughPi(t, { answer: acknowledge({ admitted: true, mode: 'tui' }) }),
    },
  },
}

describe("one contract for every channel's send", () => {
  for (const [channel, rows] of Object.entries(CHANNELS)) {
    describe(channel, () => {
      it('expresses every row', () => {
        assert.deepEqual(Object.keys(rows), Object.keys(ROWS))
      })
      for (const [row, ways] of Object.entries(rows)) {
        for (const [way, send] of Object.entries(ways)) {
          it(`${ROWS[row].says}: ${way}`, async (t) => {
            const sent = await send(t)
            assert.equal(sent.admitted, ROWS[row].admitted, JSON.stringify(sent))
            if (row === 'refused') assert.equal(sent.bytesWritten, 0, JSON.stringify(sent))
            assert.equal(admission(sent, 'refused').admitted, ROWS[row].admitted)
          })
        }
      }
    })
  }
})
