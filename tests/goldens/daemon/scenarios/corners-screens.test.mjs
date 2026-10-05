import assert from 'node:assert/strict'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { agentsServer } from '../../../../app/tests/agents-server.mjs'
import { agentRow, listAgents, rosterPath } from '../../../../src/roster.js'
import { tempEnv } from '../../../helpers.mjs'
import { bearer, code, send } from './raw-http.mjs'

/**
 * The agents screens where the unit suite and the browser specs do not look
 * (TEST-BDC-24): what they take for the human's token, which check comes
 * first, how they read a body, what they write to the roster. They are served
 * by the browser specs' own fixture, and the daemon recorder takes what they
 * answer to each request for the Rust screens to answer the same. The words
 * of `JSON.parse` are the runtime's: they are recorded, not asserted here.
 */
async function withScreens(fn, options = {}) {
  const t = tempEnv()
  mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
  const server = await agentsServer(t.env, options)
  try {
    await fn({ server, env: t.env, auth: bearer(server.token), token: server.token })
  } finally {
    await server.close()
    t.cleanup()
  }
}

const post = (server, auth, target, body) =>
  send(server, { method: 'POST', target, headers: auth, body })

describe('the agents screens, where no other suite looks', () => {
  it('takes the UI token as a bearer, or from ?token= when there is no bearer, and answers any other with its own bare 401', async () => {
    await withScreens(async ({ server, token }) => {
      const wrong = { authorization: 'Bearer wrong' }
      const cases = [
        ['GET', '/', undefined, 401],
        ['GET', '/harnesses', undefined, 401],
        ['GET', '/api/agents', wrong, 401],
        // A bearer that says something is the token: ?token= is not looked at.
        ['GET', `/api/agents?token=${token}`, wrong, 401],
        // A bearer that says nothing is none.
        ['GET', `/api/agents?token=${token}`, { authorization: 'Bearer ' }, 200],
        ['GET', `/api/agents?token=${token}`, { authorization: 'Bearer' }, 200],
        ['GET', `/api/agents?token=${token}`, { authorization: `bearer ${token}` }, 200],
        ['GET', `/api/agents?token=${token}`, undefined, 200],
        ['GET', `/?token=${token}`, undefined, 200],
        ['GET', `/harnesses?token=${token}`, undefined, 200],
        // The first ?token= is the one.
        ['GET', `/api/agents?token=wrong&token=${token}`, undefined, 401],
        ['GET', `/api/agents?token=${token}&token=wrong`, undefined, 200],
        ['GET', '/api/agents?token=', undefined, 401],
        ['GET', `/api/agents?TOKEN=${token}`, undefined, 401],
      ]
      for (const [method, target, headers, status] of cases) {
        const answer = await send(server, { method, target, headers })
        assert.equal(answer.status, status, `${method} ${target} ${JSON.stringify(headers)}`)
        if (status === 401) assert.deepEqual(answer.json, { error: 'unauthorized' })
      }
    })
  })

  it('reads the body before it looks for a route, 404s a verb it has not, and leaves a path that is no agent’s name to the API', async () => {
    await withScreens(async ({ server, auth }) => {
      const status = async (method, target, body) =>
        (await send(server, { method, target, headers: auth, body })).status
      // A body that is no JSON is refused before the 404 a route that is not there would be.
      assert.equal(await status('POST', '/', '{'), 400)
      assert.equal(await status('POST', '/harnesses', '{'), 400)
      assert.equal(await status('POST', '/api/agents/mine', '{'), 400)
      const notFound = [
        ['POST', '/', '{}'],
        ['POST', '/harnesses', '{}'],
        ['HEAD', '/'],
        ['OPTIONS', '/api/agents'],
        ['DELETE', '/api/agents', '{}'],
        ['PUT', '/api/agents/mine', '{}'],
        ['GET', '/api/agents/mine'],
        ['POST', '/api/agents/mine', '{}'],
        ['GET', '/api/preferences'],
        ['GET', '/api/harnesses/check'],
        ['GET', '/api/harnesses/update'],
      ]
      for (const [method, target, body] of notFound) {
        const answer = await send(server, { method, target, headers: auth, body })
        assert.equal(answer.status, 404, `${method} ${target}`)
        if (method !== 'HEAD') assert.deepEqual(answer.json, { error: 'not found' })
      }
      // A name the screens' pattern does not take is not theirs: the API answers, for a window.
      for (const target of [
        '/api/agents/Bad',
        '/api/agents/9x',
        '/api/agents/a_b',
        '/api/agents/a/b',
        '/api/agents/',
      ]) {
        const answer = await send(server, { target, headers: auth })
        assert.deepEqual(code(answer), [401, 'unauthorized'], target)
        assert.equal(
          answer.json.message,
          'this window has no ConsensFlow access (it may have closed)',
        )
      }
    })
  })

  it('reads the body of a preference as JSON: nothing is {}, any JSON value goes in, and a value the preference is not says so', async () => {
    await withScreens(async ({ server, auth, env }) => {
      const sent = async (body) => post(server, auth, '/api/preferences', body)
      for (const body of ['{', '{"a"', '[1,]', 'nul', "'x'", '{"a":1}x', '  ', '﻿{}']) {
        const answer = await sent(body)
        assert.equal(answer.status, 400, JSON.stringify(body))
        assert.equal(typeof answer.json.error, 'string')
      }
      for (const body of [undefined, '', '{}', '[]', 'null', '1', 'true']) {
        const answer = await sent(body)
        assert.deepEqual(
          [answer.status, answer.json],
          [200, { preferences: { ownHarnessOnly: false } }],
          String(body),
        )
      }
      // A string is read as the object of its characters.
      assert.deepEqual(code(await sent('"x"')), [400, 'no preference named 0'])
      assert.deepEqual(code(await sent('{"x":true}')), [400, 'no preference named x'])
      assert.deepEqual(code(await sent('{"ownHarnessOnly":1}')), [
        400,
        'ownHarnessOnly is on or off',
      ])
      const on = await sent('{"ownHarnessOnly":true}')
      assert.deepEqual([on.status, on.json], [200, { preferences: { ownHarnessOnly: true } }])
      assert.equal(
        JSON.parse(readFileSync(rosterPath(env), 'utf8')).preferences.ownHarnessOnly,
        true,
      )
    })
  })

  it('keeps a body to 64 K characters, not bytes, and says the body was too large', async () => {
    await withScreens(async ({ server, auth }) => {
      const pad = (text, length) => JSON.stringify({ pad: text.repeat(length) })
      const sent = (body) => post(server, auth, '/api/preferences', body)
      // As many characters as 64 K lets in: the body is parsed, and the preference it names is not one.
      const exactly = await sent(JSON.stringify({ pad: 'a'.repeat(64 * 1024 - 10) }))
      assert.deepEqual([exactly.status, exactly.json.error], [400, 'no preference named pad'])
      const over = await sent(JSON.stringify({ pad: 'a'.repeat(64 * 1024 - 9) }))
      assert.deepEqual([over.status, over.json.error], [400, 'body too large'])
      // Two bytes a character: 80,000 bytes of 40,000 characters are let in.
      const wide = await sent(pad('é', 40_000))
      assert.deepEqual([wide.status, wide.json.error], [400, 'no preference named pad'])
    })
  })

  it('adds an agent on each harness, with the effort under the key its harness reads, edits and removes it, and writes the roster each time', async () => {
    await withScreens(async ({ server, auth, env }) => {
      const add = (body) => post(server, auth, '/api/agents', JSON.stringify(body))
      const patch = (name, body) =>
        send(server, {
          method: 'PATCH',
          target: `/api/agents/${name}`,
          headers: auth,
          body: JSON.stringify(body),
        })
      assert.equal((await add({ name: 'zed', harness: 'claude', model: 'm' })).status, 201)
      const pi = await add({ name: 'pip', harness: 'pi', model: 'm', effort: 'high' })
      assert.deepEqual([pi.status, pi.json.agent.effort], [201, 'high'])
      assert.equal(agentRow('pip', env).thinking, 'high', 'Pi reads thinking')
      assert.equal(agentRow('pip', env).effort, undefined)
      const image = await add({
        name: 'img',
        harness: 'codex',
        designer: true,
        model: 'codex-image',
      })
      assert.deepEqual([image.status, image.json.agent.designer], [201, true])
      assert.equal(
        (
          await add({
            name: 'oc',
            harness: 'opencode',
            model: 'm',
            effort: 'low',
            workTier: 'complex',
            description: 'Mine',
          })
        ).status,
        201,
      )
      assert.equal((await add({ name: 'dv', harness: 'devin', model: 'm' })).status, 201)
      // Clearing: an empty or null effort goes, a null tier goes, a description is set.
      assert.equal((await patch('oc', { effort: '' })).status, 200)
      assert.equal(agentRow('oc', env).effort, undefined)
      assert.equal((await patch('oc', { workTier: null })).status, 200)
      assert.equal(listAgents(env).find((a) => a.name === 'oc').workTier, undefined)
      assert.equal((await patch('pip', { effort: null })).status, 200)
      assert.equal(agentRow('pip', env).thinking, undefined)
      assert.equal(
        (await patch('zed', { description: 'hi', model: 'n' })).json.agent.description,
        'hi',
      )
      assert.equal((await patch('pip', { effort: 'low' })).json.agent.effort, 'low')
      for (const name of ['zed', 'pip', 'img', 'oc', 'dv']) {
        const removed = await send(server, {
          method: 'DELETE',
          target: `/api/agents/${name}`,
          headers: auth,
        })
        assert.deepEqual([removed.status, removed.text], [204, ''], name)
      }
      assert.equal(
        listAgents(env).some((agent) => agent.custom === true),
        false,
      )
    })
  })

  it('refuses in the roster’s own words what the roster refuses, and writes nothing for it', async () => {
    await withScreens(async ({ server, auth, env }) => {
      const add = (body) => post(server, auth, '/api/agents', body)
      assert.equal(
        (await add(JSON.stringify({ name: 'zed', harness: 'claude', model: 'm' }))).status,
        201,
      )
      const refused = [
        ['[]', 'agent names are lowercase'],
        ['"x"', 'agent names are lowercase'],
        ['{}', 'agent names are lowercase'],
        [
          JSON.stringify({ name: 'Zed', harness: 'claude', model: 'm' }),
          'agent names are lowercase',
        ],
        [JSON.stringify({ name: 'zed', harness: 'claude', model: 'm' }), 'already exists'],
        [JSON.stringify({ name: 'ab', harness: 'nope', model: 'm' }), 'unknown harness'],
        [JSON.stringify({ name: 'ab', harness: 'claude', model: '' }), 'needs a model'],
        [
          JSON.stringify({ name: 'ab', harness: 'claude', model: 'm', designer: 'yes' }),
          'image agent',
        ],
        [
          JSON.stringify({ name: 'ab', harness: 'claude', model: 'm', designer: true }),
          'Codex agent',
        ],
        [
          JSON.stringify({ name: 'ab', harness: 'claude', model: 'm', workTier: 'huge' }),
          'Work tier',
        ],
        [JSON.stringify({ name: 'gefjon', harness: 'claude', model: 'm' }), 'catalog agent'],
      ]
      for (const [body, words] of refused) {
        const answer = await add(body)
        assert.equal(answer.status, 400, body)
        assert.match(answer.json.error, new RegExp(words), body)
      }
      assert.equal((await add('null')).status, 400, 'a body of null is a throw like any other')
      const before = listAgents(env)
        .filter((agent) => agent.custom)
        .map((agent) => agent.name)
      assert.deepEqual(before, ['zed'])
      const patch = (name, body) =>
        send(server, {
          method: 'PATCH',
          target: `/api/agents/${name}`,
          headers: auth,
          body: JSON.stringify(body),
        })
      assert.match((await patch('nobody', { model: 'x' })).json.error, /no agent named nobody/)
      assert.match((await patch('zed', { model: '' })).json.error, /needs a model/)
      assert.match((await patch('zed', { workTier: 'huge' })).json.error, /Work tier/)
      const gone = await send(server, {
        method: 'DELETE',
        target: '/api/agents/nobody',
        headers: auth,
      })
      assert.deepEqual([gone.status, gone.json.error], [400, 'no agent named nobody'])
    })
  })

  it('says why the roster cannot be read, with the path of the file, and reads it again once it can', async () => {
    await withScreens(async ({ server, auth, env }) => {
      const file = rosterPath(env)
      writeFileSync(file, '{ "agents": [,] }')
      for (const answer of [
        await send(server, { target: '/api/agents', headers: auth }),
        await post(
          server,
          auth,
          '/api/agents',
          JSON.stringify({ name: 'zed', harness: 'claude', model: 'm' }),
        ),
        await post(server, auth, '/api/preferences', '{}'),
      ]) {
        assert.equal(answer.status, 400)
        assert.ok(answer.json.error.includes(file), 'the file is named')
        assert.match(answer.json.error, /is not valid JSON: fix it or move it away/)
      }
      assert.equal(readFileSync(file, 'utf8'), '{ "agents": [,] }', 'left as it was')
      writeFileSync(file, '[]')
      assert.match(
        (await send(server, { target: '/api/agents', headers: auth })).json.error,
        /is not an agents file/,
      )
      writeFileSync(file, JSON.stringify({ schemaVersion: 1, agents: [] }))
      assert.equal((await send(server, { target: '/api/agents', headers: auth })).status, 200)
    })
  })

  it('offers only the harnesses installed here, and hides the agents of the others', {
    skip:
      process.platform === 'win32' &&
      'the fixture’s stand-ins are shell scripts, which Windows does not find',
  }, async () => {
    await withScreens(
      async ({ server, auth }) => {
        const listed = (await send(server, { target: '/api/agents', headers: auth })).json
        assert.deepEqual(listed.harnesses, ['claude', 'pi'])
        const hidden = listed.agents.filter((agent) => agent.notInstalled === true)
        assert.ok(hidden.length > 0)
        for (const agent of hidden)
          assert.deepEqual([agent.hidden, ['claude', 'pi'].includes(agent.harness)], [true, false])
        const none = listed.agents.filter((agent) => ['claude', 'pi'].includes(agent.harness))
        assert.ok(none.every((agent) => agent.notInstalled === undefined))
      },
      { installed: ['claude', 'pi'] },
    )
  })

  it('refuses a harness it does not know on the harness routes before it asks the machine anything', async () => {
    await withScreens(async ({ server, auth }) => {
      for (const target of ['/api/harnesses/check', '/api/harnesses/update']) {
        for (const body of ['{"id":"nope"}', '{"id":5}', '{"id":""}', '{"id":["claude"]}']) {
          const answer = await post(server, auth, target, body)
          assert.deepEqual(
            [answer.status, answer.json],
            [400, { error: 'Unknown harness' }],
            `${target} ${body}`,
          )
        }
      }
      for (const body of ['{}', '{"id":null}']) {
        assert.deepEqual(
          (await post(server, auth, '/api/harnesses/update', body)).json,
          { error: 'Unknown harness' },
          body,
        )
      }
      // A body that is no object, or no JSON, is a throw of the runtime's, answered as any other.
      for (const body of ['null', '{'])
        assert.equal((await post(server, auth, '/api/harnesses/update', body)).status, 400)
    })
  })

  it('says which harness is not installed, by its own name, when it is asked to update one', async () => {
    await withScreens(
      async ({ server, auth }) => {
        const names = {
          claude: 'Claude',
          codex: 'Codex',
          opencode: 'OpenCode',
          pi: 'Pi',
          devin: 'Devin',
        }
        for (const [id, name] of Object.entries(names)) {
          const answer = await post(server, auth, '/api/harnesses/update', JSON.stringify({ id }))
          assert.deepEqual(
            [answer.status, answer.json],
            [400, { error: `${name} is not installed` }],
            id,
          )
        }
      },
      { installed: [] },
    )
  })
})
