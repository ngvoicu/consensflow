import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { before, describe, it } from 'node:test'
import { cargoMissing, rustCodex, rustOpenCode, rustPi } from './rust-channels.mjs'

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
 * Codex's, OpenCode's and Pi's channels run every row here, through the Rust
 * send (`tests/rust-channels.mjs`), against stand-ins for the pane host and the
 * harness side. The channels that are pasted into their windows (Claude Code
 * and Devin) have no test binary: their rows run in Rust, through the same
 * `write_paste` and `Sent` the adapters read (`shared/pane/tests.rs`, and
 * `devin/channel/tests.rs` for Devin's own refusals), where the answer is read as
 * the adapters read it (`shared::admission`).
 */
const ROWS = {
  refused: { says: 'refused before its handover point', admitted: false },
  uncertain: { says: 'an error at or after its handover point', admitted: null },
  accepted: { says: 'accepted', admitted: true },
}

/**
 * The pane host a channel claims its pane of: it answers the claim as told, and
 * admits it by default (`rust-channels.mjs` asks it as the channel does).
 */
const claiming =
  (claim = { ok: true }) =>
  async () =>
    claim

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

/** The rows of Codex's channel, run through `send`. */
const throughCodex =
  (send) =>
  async (t, { claim, ...server }) =>
    send(
      {
        session: '01a0817b-e6b0-7f32-8e11-370dc000cbc0',
        pane: 'p1-diana',
        generation: 3,
        claim: claiming(claim),
        launch: {
          kind: 'codex-queue',
          launchId: 'launch-codex',
          sessionBridge: await harnessServer(t, server),
        },
      },
      'the cache key is per conversation',
    )

/** The rows of OpenCode's channel, run through `send`. */
const throughOpenCode =
  (send) =>
  async (t, { claim, ...server }) =>
    send(
      {
        session: 'ses_contract1',
        pane: 'p1-hera',
        generation: 3,
        claim: claiming(claim),
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

/** The rows of Pi's channel, run through `send`. */
const throughPi =
  (send) =>
  async (t, { answer = async () => {}, claim, ackTimeoutMs = 5_000, inbox } = {}) => {
    const window = await piWindow(t, answer)
    return send(
      {
        session: 'cf-1-zeus-0000abcd',
        pane: 'p1-zeus',
        generation: 3,
        claim: claiming(claim),
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

/** The channels that are reached by a send of their own, each row run through its `send` in `senders`. */
function postedChannels(senders) {
  const sendThroughPi = throughPi(senders.pi)
  return {
    'Codex, through its broker': posted(throughCodex(senders.codex), 'the broker'),
    'OpenCode, through its plugin': posted(throughOpenCode(senders.opencode), 'the plugin'),
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
}

/** One `describe` per channel, holding every row of the contract. */
function hold(channels) {
  for (const [channel, rows] of Object.entries(channels)) {
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
          })
        }
      }
    })
  }
}

describe("one contract for every channel's send", { skip: cargoMissing }, () => {
  let rust
  before(() => {
    rust = { codex: rustCodex(), opencode: rustOpenCode(), pi: rustPi() }
  })
  hold(
    postedChannels({
      codex: (target, text) => rust.codex.send(target, text),
      opencode: (target, text) => rust.opencode.send(target, text),
      pi: (target, text) => rust.pi.send(target, text),
    }),
  )
})
