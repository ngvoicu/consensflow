import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { chmod, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { codexAuthPath, loadCodexAuth } from '../../hosts/lib/codex-auth.js'
import { spawnWithInput } from '../../hosts/lib/runners.js'
import { workspaceKey } from '../../hosts/lib/state.js'

// The payload CLI these tests used to drive is gone: a mode installs the
// generated skill and nothing else, so there is one CLI left — the manager's.
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const ENGINE = path.join(ROOT, 'hosts', 'lib')
const CF = path.join(ROOT, 'bin', 'cf.mjs')

async function withTempDir(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-cc-test-'))
  // Point the shared home at the temp dir for IN-PROCESS lib calls too, not just the
  // subprocess helpers that pass it explicitly. Without this, a direct saveSession()/
  // loadSession() in a test writes run artifacts into the developer's real ~/.consensflow.
  const oldHome = process.env.CONSENSFLOW_HOME
  process.env.CONSENSFLOW_HOME = path.join(dir, 'home')
  try {
    return await fn(dir)
  } finally {
    if (oldHome === undefined) delete process.env.CONSENSFLOW_HOME
    else process.env.CONSENSFLOW_HOME = oldHome
    await rm(dir, { recursive: true, force: true })
  }
}

// --- CLI end-to-end with fake engine binaries -------------------------------
// Per project policy, live harness CLIs are never invoked from tests. Each engine gets a PATH shim
// that dumps its argv/env/stdin to a file and prints engine-shaped output, so the full spawn →
// packet → parse → artifact path is exercised for all four engines.

const SHIM_BODIES = {
  claude: `console.log(JSON.stringify({ type: "system", subtype: "init" }));
console.log(JSON.stringify({ type: "assistant", message: { role: "assistant", content: [{ type: "text", text: "CLAUDE OK" }] } }));
console.log(JSON.stringify({ type: "result", subtype: "success", is_error: false, result: "CLAUDE OK" }));`,
  codex: `console.log(JSON.stringify({ type: "thread.started", thread_id: "t" }));\nconsole.log(JSON.stringify({ type: "item.completed", item: { type: "agent_message", text: "CODEX OK" } }));`,
  pi: `console.log(JSON.stringify({ type: "agent_end", messages: [{ role: "assistant", content: [{ type: "text", text: "PI OK" }] }] }));`,
  opencode: `console.log(JSON.stringify({ type: "text", part: { type: "text", text: "OPENCODE OK" } }));`,
}

async function makeFakeEngines(dir) {
  const bin = path.join(dir, 'fakebin')
  const out = path.join(dir, 'engine-out')
  await mkdir(bin, { recursive: true })
  await mkdir(out, { recursive: true })
  for (const [name, body] of Object.entries(SHIM_BODIES)) {
    const source = [
      '#!/usr/bin/env node',
      'const fs = require("node:fs");',
      'const path = require("node:path");',
      'let stdin = "";',
      'try { stdin = fs.readFileSync(0, "utf8"); } catch {}',
      'let packetFromFile = null;',
      'const fileIdx = process.argv.indexOf("--file");',
      'if (fileIdx >= 0) { try { packetFromFile = fs.readFileSync(process.argv[fileIdx + 1], "utf8"); } catch {} }',
      'const dump = {',
      `  name: ${JSON.stringify(name)},`,
      '  argv: process.argv.slice(2),',
      '  stdin,',
      '  packetFromFile,',
      '  env: {',
      '    CONSENSFLOW_CHILD: process.env.CONSENSFLOW_CHILD ?? null,',
      '    OPENCODE_PERMISSION: process.env.OPENCODE_PERMISSION ?? null,',
      '    ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY ?? null,',
      '    OPENAI_API_KEY: process.env.OPENAI_API_KEY ?? null,',
      '    CMUX_SOCKET_CAPABILITY: process.env.CMUX_SOCKET_CAPABILITY ?? null,',
      '    CMUX_CLAUDE_HOOK_CMUX_BIN: process.env.CMUX_CLAUDE_HOOK_CMUX_BIN ?? null,',
      '  },',
      '};',
      `fs.writeFileSync(path.join(process.env.FAKE_ENGINE_OUT, ${JSON.stringify(name)} + ".json"), JSON.stringify(dump, null, 2));`,
      body,
    ].join('\n')
    const shimPath = path.join(bin, name)
    await writeFile(shimPath, source, 'utf8')
    await chmod(shimPath, 0o755)
  }
  return { bin, out }
}

async function runCf(args, { ws, dir, fake }, extraEnv = {}) {
  return await spawnWithInput(process.execPath, [CF, ...args], {
    cwd: ws,
    timeoutMs: 30000,
    // spawnWithInput merges process.env underneath these, so a suite run from
    // INSIDE an agent (where the marker is set) would spawn a cf that refuses
    // itself as a nested run. The guard is real and tested below on purpose —
    // it just must not fire on the manager the scenarios drive.
    dropEnv: ['CONSENSFLOW_CHILD'],
    env: {
      CONSENSFLOW_HOME: path.join(dir, 'home'),
      PATH: `${fake.bin}${path.delimiter}${process.env.PATH}`,
      FAKE_ENGINE_OUT: fake.out,
      // Billing guard probe: these must NOT reach claude/codex children.
      ANTHROPIC_API_KEY: 'leak-test',
      OPENAI_API_KEY: 'leak-test',
      // cmux control-socket guard probe: the bearer token must NOT reach any child.
      CMUX_SOCKET_CAPABILITY: 'leak-test',
      CMUX_CLAUDE_HOOK_CMUX_BIN: 'leak-test',
      ...extraEnv,
    },
  })
}

async function latestPacket(ws, dir) {
  const current = JSON.parse(
    await readFile(path.join(dir, 'home', 'workspaces', workspaceKey(ws), 'current.json'), 'utf8'),
  )
  return {
    current,
    packet: await readFile(path.join(current.latestRunDir, 'packet.md'), 'utf8'),
    result: JSON.parse(await readFile(path.join(current.latestRunDir, 'result.json'), 'utf8')),
  }
}

test('e2e: all four engines run, parse, and persist artifacts through the real spawn path', async () => {
  await withTempDir(async (dir) => {
    const ws = path.join(dir, 'ws')
    await mkdir(ws, { recursive: true })
    const fake = await makeFakeEngines(dir)
    const ctx = { ws, dir, fake }
    for (const preset of ['zeus', 'gaia', 'kronos', 'mani']) {
      const add = await runCf(['agent', 'add', preset], ctx)
      assert.equal(add.exitCode, 0, add.stderr)
    }

    const cases = [
      { ref: '@zeus', engine: 'claude', expect: 'CLAUDE OK' },
      { ref: '@gaia', engine: 'codex', expect: 'CODEX OK' },
      { ref: '@kronos', engine: 'pi', expect: 'PI OK' },
      { ref: '@mani', engine: 'opencode', expect: 'OPENCODE OK' },
    ]
    for (const { ref, engine, expect } of cases) {
      const run = await runCf(['run', ref, 'ping', 'from', 'the', 'test'], ctx)
      assert.equal(run.exitCode, 0, `${ref}: ${run.stderr}`)
      // Attributed either way: `# @name` with the answer under it, or `— @name`
      // alone when the answer already streamed and repeating it would be noise.
      assert.match(run.stdout, new RegExp(`[#—] ${ref}`), `${ref}: attributed`)
      assert.ok(run.stdout.includes(expect), `${ref}: parsed engine output`)
      // Clean output: a successful read-only run shows just the answer — no run metadata, no
      // consent boilerplate. (The `Handoff: empty` warning IS expected here: this test never
      // stashes a transcript; the handoff e2e below covers when that line appears.)
      assert.doesNotMatch(run.stdout, /Run:|Exit:|Artifacts:|approval/, `${ref}: no metadata noise`)

      const { packet, result } = await latestPacket(ws, dir)
      assert.match(packet, /# ConsensFlow Packet/)
      assert.match(packet, /You can read and modify this workspace/)
      assert.match(packet, /ping from the test/)
      assert.equal(result.output, expect)
      assert.equal(result.exitCode, 0)

      const dump = JSON.parse(await readFile(path.join(fake.out, `${engine}.json`), 'utf8'))
      assert.equal(
        dump.env.CONSENSFLOW_CHILD,
        '1',
        `${engine}: child marker reaches the subprocess`,
      )
      assert.equal(dump.env.CMUX_SOCKET_CAPABILITY, null, `${engine}: cmux socket token stripped`)
      assert.equal(
        dump.env.CMUX_CLAUDE_HOOK_CMUX_BIN,
        null,
        `${engine}: cmux hook binary path stripped`,
      )
      const packetSeen = engine === 'opencode' ? dump.packetFromFile : dump.stdin
      assert.ok(
        String(packetSeen ?? '').includes('# ConsensFlow Packet'),
        `${engine}: packet delivered`,
      )
    }

    // The relocation contract: a run never creates anything inside the project workspace.
    assert.ok(
      !(await readdir(ws)).some((entry) => entry.startsWith('.consensflow')),
      'no .consensflow* dir in the workspace',
    )

    // Engine-specific guards observed from inside the children.
    const claude = JSON.parse(await readFile(path.join(fake.out, 'claude.json'), 'utf8'))
    assert.ok(
      !claude.argv.includes('--bare'),
      'claude child must NOT run --bare: it forbids OAuth/keychain auth while ANTHROPIC_API_KEY is stripped (1.5.1 regression guard); recursion is covered by CONSENSFLOW_CHILD',
    )
    assert.equal(claude.argv.includes('--disallowedTools'), false, 'no claude deny list')
    assert.equal(claude.argv[claude.argv.indexOf('--model') + 1], 'claude-opus-5')
    assert.equal(claude.argv[claude.argv.indexOf('--effort') + 1], 'max')
    assert.equal(claude.env.ANTHROPIC_API_KEY, null, 'billing guard strips ANTHROPIC_API_KEY')
    const codex = JSON.parse(await readFile(path.join(fake.out, 'codex.json'), 'utf8'))
    assert.equal(codex.argv.includes('--sandbox'), false, 'no sandbox policy is selected')
    assert.ok(
      codex.argv.includes('--dangerously-bypass-approvals-and-sandbox'),
      'codex runs unsandboxed',
    )
    assert.equal(codex.env.OPENAI_API_KEY, null, 'billing guard strips OPENAI_API_KEY')
    const piDump = JSON.parse(await readFile(path.join(fake.out, 'pi.json'), 'utf8'))
    assert.ok(piDump.argv.includes('--no-extensions'), 'pi child runs without extensions')
    assert.equal(piDump.argv[piDump.argv.indexOf('--thinking') + 1], 'xhigh')
    const opencode = JSON.parse(await readFile(path.join(fake.out, 'opencode.json'), 'utf8'))
    assert.equal(opencode.env.OPENCODE_PERMISSION, null, 'no opencode permission overlay')
  })
})

test('e2e: streaming is the default (thinking always visible); --json is the only quiet mode [STRM-17]', async () => {
  await withTempDir(async (dir) => {
    const ws = path.join(dir, 'ws')
    await mkdir(ws, { recursive: true })
    const bin = path.join(dir, 'fakebin')
    await mkdir(bin, { recursive: true })
    // A fake opencode that emits a real-shaped tool_use + text part, then exits cleanly.
    const shim = [
      '#!/usr/bin/env node',
      `console.log(JSON.stringify({ type: "tool_use", part: { type: "tool", tool: "read", state: { input: { path: "f.txt" }, output: "body" } } }));`,
      `console.log(JSON.stringify({ type: "text", part: { text: "the streamed answer" } }));`,
    ].join('\n')
    const shimPath = path.join(bin, 'opencode')
    await writeFile(shimPath, shim, 'utf8')
    await chmod(shimPath, 0o755)
    // A fake pi that reports its final answer only in agent_end. The stream adapter intentionally
    // skips agent_end to avoid duplicate message_end text, so the CLI must print the parsed final
    // result after --stream when no answer text streamed.
    const piShim = [
      '#!/usr/bin/env node',
      `console.log(JSON.stringify({ type: "agent_end", messages: [{ role: "assistant", content: [{ type: "text", text: "PI FALLBACK FINAL" }] }] }));`,
    ].join('\n')
    const piShimPath = path.join(bin, 'pi')
    await writeFile(piShimPath, piShim, 'utf8')
    await chmod(piShimPath, 0o755)
    const ctx = { ws, dir, fake: { bin, out: dir } }
    await runCf(['agent', 'add', 'mani'], ctx)

    // Streaming needs no flag. The parsed final reply is still printed after the child exits,
    // so a foreground run always ends with a durable, attributed answer section.
    const streamed = await runCf(
      // One quoted task, the way the skill teaches it — so a `--stat` inside the
      // prompt is text, not an option the manager's strict parser must guess at.
      ['run', '@mani', 'go check git diff --stat'],
      ctx,
    )
    assert.match(streamed.stdout, /→ .*read/, 'the tool call is streamed live')
    assert.match(streamed.stdout, /the streamed answer/, 'the text is streamed live')
    assert.match(
      streamed.stdout,
      /[#—] @mani/,
      'the final answer section is printed after the stream',
    )
    assert.match(
      (await latestPacket(ws, dir)).packet,
      /go check git diff --stat/,
      'flag-like text inside the task survives into the packet',
    )

    // Streaming is the default — thinking/tools are ALWAYS visible. A run without --stream still
    // streams; only --json suppresses the live trail (machine-readable output).
    const plain = await runCf(['run', '@mani', 'go'], ctx)
    assert.match(plain.stdout, /the streamed answer/, 'the answer is present')
    assert.match(
      plain.stdout,
      /→ .*read/,
      'event lines stream by default — no --stream flag needed',
    )
    const quiet = await runCf(['run', '@mani', 'go', '--json'], ctx)
    assert.doesNotMatch(quiet.stdout, /→ .*read|← .*read/, '--json is the only quiet mode')

    assert.equal(
      // The manager validates names rather than slugifying them, so it is spelled
      // here exactly as `@pionly` refers to it below.
      (await runCf(['agent', 'add', 'pionly', '--harness', 'pi', '--model', 'fake'], ctx)).exitCode,
      0,
    )
    const fallback = await runCf(['run', '@pionly', 'go'], ctx)
    assert.match(
      fallback.stdout,
      /PI FALLBACK FINAL/,
      'the final output is printed even when no text event streamed',
    )
  })
})

test('e2e: runAgent writes a transcript.md backstop (event trail) and sets transcriptPath [STRM-19]', async () => {
  await withTempDir(async (dir) => {
    const ws = path.join(dir, 'ws')
    await mkdir(ws, { recursive: true })
    const bin = path.join(dir, 'fakebin')
    await mkdir(bin, { recursive: true })
    const shim = [
      '#!/usr/bin/env node',
      `console.log(JSON.stringify({ type: "tool_use", part: { type: "tool", tool: "read", state: { input: { path: "f.txt" }, output: "file body" } } }));`,
      `console.log(JSON.stringify({ type: "text", part: { text: "the backstopped answer" } }));`,
    ].join('\n')
    const shimPath = path.join(bin, 'opencode')
    await writeFile(shimPath, shim, 'utf8')
    await chmod(shimPath, 0o755)
    const ctx = { ws, dir, fake: { bin, out: dir } }
    await runCf(['agent', 'add', 'mani'], ctx)

    const run = await runCf(['run', '@mani', 'go', '--json'], ctx)
    const result = JSON.parse(run.stdout)
    assert.ok(result.transcriptPath, 'result.json carries transcriptPath')
    const transcript = await readFile(result.transcriptPath, 'utf8')
    assert.ok(transcript.trim().length > 0, 'transcript.md is non-empty')
    assert.match(transcript, /read/, 'transcript includes the tool call')
    assert.match(transcript, /the backstopped answer/, 'transcript includes the answer, in order')
  })
})

test('e2e: a run has full permissions, and there is still no knob to turn [STRM-25]', async () => {
  await withTempDir(async (dir) => {
    const ws = path.join(dir, 'ws')
    await mkdir(ws, { recursive: true })
    const fake = await makeFakeEngines(dir)
    const ctx = { ws, dir, fake }
    await runCf(['agent', 'add', 'zeus'], ctx) // claude-code

    // Every run is full-permission: no allowlist, no deny list, no prompts.
    await runCf(['run', '@zeus', 'go'], ctx)
    let claude = JSON.parse(await readFile(path.join(fake.out, 'claude.json'), 'utf8'))
    assert.ok(
      claude.argv.includes('--dangerously-skip-permissions'),
      'default run bypasses prompts',
    )
    assert.equal(claude.argv.includes('--allowedTools'), false, 'no allowlist fences the tools')
    assert.equal(claude.argv.includes('--disallowedTools'), false, 'no deny list')

    // There is still no permission knob: asking for one changes nothing, because
    // every run already has everything.
    await runCf(['run', '@zeus', 'go', '--tools', 'full-auto'], ctx)
    claude = JSON.parse(await readFile(path.join(fake.out, 'claude.json'), 'utf8'))
    assert.ok(
      claude.argv.includes('--dangerously-skip-permissions'),
      'the flag is the default, not an escalation',
    )
  })
})

test('e2e: nested runs are refused and unknown agents error cleanly', async () => {
  await withTempDir(async (dir) => {
    const ws = path.join(dir, 'ws')
    await mkdir(ws, { recursive: true })
    const fake = await makeFakeEngines(dir)
    const ctx = { ws, dir, fake }
    assert.equal((await runCf(['agent', 'add', 'zeus'], ctx)).exitCode, 0)

    const unknown = await runCf(['run', '@ghost', 'hi'], ctx)
    assert.equal(unknown.exitCode, 1)
    assert.match(unknown.stderr, /no agent named "ghost"/)
    assert.match(unknown.stderr, /zeus/, 'it names who you do have')

    const nested = await spawnWithInput(process.execPath, [CF, 'run', '@zeus', 'hi'], {
      cwd: ws,
      timeoutMs: 15000,
      env: { CONSENSFLOW_HOME: path.join(dir, 'home'), CONSENSFLOW_CHILD: '1' },
    })
    assert.equal(nested.exitCode, 1)
    assert.match(nested.stderr, /already an agent run — an agent does not spawn agents/)
  })
})

// --- Image agents (Codex backend) -------------------------------------

function fakeJwt(claims) {
  const payload = Buffer.from(JSON.stringify(claims)).toString('base64url')
  return `header.${payload}.signature`
}

// `parseRunOptions` was the payload CLI's hand-rolled argument parser, and it
// went with the payload. The manager parses with node's own `parseArgs`
// (`image: { type: 'string', multiple: true }`, bin/cf.mjs), which is where
// `--image x` / `--image=x` / repeats are handled now.

test('loadCodexAuth reads the Codex CLI login (account_id field, JWT fallback, clear errors)', async () => {
  await withTempDir(async (dir) => {
    const oldCodexHome = process.env.CODEX_HOME
    process.env.CODEX_HOME = path.join(dir, 'codex-home')
    try {
      // No auth file at all → fix-it error naming the path.
      assert.match(codexAuthPath(), /codex-home/)
      await assert.rejects(loadCodexAuth(), /codex login/)

      // access_token + explicit account_id → both returned verbatim.
      await mkdir(process.env.CODEX_HOME, { recursive: true })
      const token = fakeJwt({ 'https://api.openai.com/auth': { chatgpt_account_id: 'acc_jwt' } })
      await writeFile(
        path.join(process.env.CODEX_HOME, 'auth.json'),
        JSON.stringify({ tokens: { access_token: token, account_id: 'acc_field' } }),
        'utf8',
      )
      assert.deepEqual(await loadCodexAuth(), { token, accountId: 'acc_field' })

      // Missing account_id field → decoded from the JWT claim.
      await writeFile(
        path.join(process.env.CODEX_HOME, 'auth.json'),
        JSON.stringify({ tokens: { access_token: token } }),
        'utf8',
      )
      assert.deepEqual(await loadCodexAuth(), { token, accountId: 'acc_jwt' })

      // Claim-less access token falls back to the id_token's claim.
      const bare = fakeJwt({ sub: 'x' })
      const idToken = fakeJwt({
        'https://api.openai.com/auth': { chatgpt_account_id: 'acc_id_token' },
      })
      await writeFile(
        path.join(process.env.CODEX_HOME, 'auth.json'),
        JSON.stringify({ tokens: { access_token: bare, id_token: idToken } }),
        'utf8',
      )
      assert.equal((await loadCodexAuth()).accountId, 'acc_id_token')

      // API-key-only auth (no ChatGPT tokens) → clear error.
      await writeFile(
        path.join(process.env.CODEX_HOME, 'auth.json'),
        JSON.stringify({ OPENAI_API_KEY: 'sk-test' }),
        'utf8',
      )
      await assert.rejects(loadCodexAuth(), /no ChatGPT access token/)
    } finally {
      if (oldCodexHome === undefined) delete process.env.CODEX_HOME
      else process.env.CODEX_HOME = oldCodexHome
    }
  })
})

test('e2e: @pygmalion without a Codex login errors cleanly before any network call', async () => {
  await withTempDir(async (dir) => {
    const ws = path.join(dir, 'ws')
    await mkdir(ws, { recursive: true })
    const fake = await makeFakeEngines(dir)
    const ctx = { ws, dir, fake }
    assert.equal((await runCf(['agent', 'add', 'pygmalion'], ctx)).exitCode, 0)
    const run = await runCf(['run', '@pygmalion', 'a', 'minimalist', 'logo'], ctx, {
      CODEX_HOME: path.join(dir, 'empty-codex-home'),
    })
    assert.equal(run.exitCode, 1)
    assert.match(run.stderr, /No Codex CLI login found/)
    assert.match(run.stderr, /codex login/)
  })
})

// --- Plugin packaging: consent gate, hooks wiring, import boundaries --------

test('the consent gate and name-neutrality stay locked into the generated skill', async () => {
  // There is one skill now, and it is generated — so the gate has to live in
  // the generator, not in a hand-written copy per host. The payload skills
  // that used to carry their own copy are gone.
  const { generateSkill } = await import('../../src/skill.js')
  const skill = generateSkill([
    { name: 'zeus', harness: 'claude', model: 'claude-opus-5', effort: 'max' },
  ])
  assert.match(skill, /Advice is free; acting is gated/)
  assert.match(skill, /without the user's explicit approval/)
  assert.match(skill, /You do not need permission to consult/)

  // No host payload ships any more, so no second copy of the gate can drift.
  assert.equal(existsSync(path.join(ROOT, 'hosts', 'claude')), false, 'no claude payload')
  assert.equal(existsSync(path.join(ROOT, 'hosts', 'pi')), false, 'no pi payload')

  // The personal name must not appear in anything that ships.
  for (const base of [ENGINE, path.join(ROOT, 'bin'), path.join(ROOT, 'src')]) {
    for (const file of await readdir(base, { recursive: true })) {
      const full = path.join(base, file)
      const content = await readFile(full, 'utf8').catch(() => '')
      assert.doesNotMatch(content, /Gabriel/, `${base}/${file}`)
    }
  }
})
