import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { createPacket } from '../../hosts/lib/packets.js'
import {
  AGENT_PRESETS,
  agentFromPreset,
  driftedAgents,
  getPreset,
  listPresetIds,
} from '../../hosts/lib/presets.js'
import {
  buildRunnerInvocation,
  normalizeProcessOutput,
  runAgent,
  spawnWithInput,
  toolsForPi,
} from '../../hosts/lib/runners.js'
import {
  agentsPath,
  configRoot,
  getAgent,
  loadAgents,
  normalizeAgent,
  removeAgent,
  syncAgentsWithPresets,
  upsertAgent,
} from '../../hosts/lib/state.js'
import {
  parseAgentPrompt,
  parseOptions,
  resolveInside,
  slugify,
  tokenize,
} from '../../hosts/lib/utils.js'

async function withTempDir(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-cc-test-'))
  const oldHome = process.env.CONSENSFLOW_HOME
  process.env.CONSENSFLOW_HOME = path.join(dir, 'home', '.consensflow')
  try {
    return await fn(dir)
  } finally {
    if (oldHome === undefined) delete process.env.CONSENSFLOW_HOME
    else process.env.CONSENSFLOW_HOME = oldHome
    await rm(dir, { recursive: true, force: true })
  }
}

test('tokenize handles quotes and parseOptions handles flags', () => {
  assert.deepEqual(tokenize('add "Zeus Opus" --kind claude-code --model claude-opus-4-7'), [
    'add',
    'Zeus Opus',
    '--kind',
    'claude-code',
    '--model',
    'claude-opus-4-7',
  ])
  assert.deepEqual(parseOptions(['Athena', '--kind=codex', '--model', 'gpt-5.5']).flags, {
    kind: 'codex',
    model: 'gpt-5.5',
  })
  // `--no-*` negation flags are always boolean — they must never swallow the next token
  // (this is how `run @zeus --no-handoff <prompt>` used to eat the prompt).
  assert.deepEqual(parseOptions(['@zeus', '--no-handoff', 'what', 'about', 'this?']), {
    positional: ['@zeus', 'what', 'about', 'this?'],
    flags: { 'no-handoff': true },
  })
})

test('slugify creates stable mentions', () => {
  assert.equal(slugify('Zeus Opus 4.7'), 'zeus-opus-4-7')
  assert.equal(slugify(' Isis  '), 'isis')
})

test('agent CRUD persists global user-level JSON', async () => {
  await withTempDir(async (cwd) => {
    const athena = await upsertAgent(cwd, {
      name: 'Athena',
      kind: 'codex',
      model: 'gpt-5.5',
      effort: 'xhigh',
    })
    assert.equal(athena.id, 'athena')
    assert.equal((await getAgent(cwd, '@athena')).model, 'gpt-5.5')
    assert.equal((await loadAgents(cwd)).length, 1)
    assert.equal(await removeAgent(cwd, 'athena'), true)
    assert.equal((await loadAgents(cwd)).length, 0)
  })
})

test('the brief leads the packet, and nothing stands in for it when absent', async () => {
  const agent = normalizeAgent({ id: 'diana', name: 'Diana', kind: 'codex', model: 'gpt-5.6-luna' })

  const withBrief = await createPacket({
    cwd: '/repo',
    agent,
    task: 'check the export path',
    brief: 'You are reviewing this for GDPR: lawful basis, data minimisation, retention.',
  })
  assert.match(withBrief, /## Your brief for this run/)
  assert.match(withBrief, /lawful basis/)
  assert.ok(
    withBrief.indexOf('## Your brief') < withBrief.indexOf('## Message from the user'),
    'the brief comes before the task',
  )

  const plain = await createPacket({ cwd: '/repo', agent, task: 'check the export path' })
  assert.doesNotMatch(plain, /## Your brief/)
  // No persona is invented in its place, and no run is called a coding session.
  assert.doesNotMatch(plain, /You are Diana/)
  assert.doesNotMatch(plain, /joining a coding session/)
})

test('createPacket is conversational and carries handoff + diff', async () => {
  await withTempDir(async (cwd) => {
    const agent = await upsertAgent(cwd, {
      name: 'Zeus',
      kind: 'pi',
      model: 'openrouter/anthropic/claude-opus-4.7',
    })
    const packet = await createPacket({
      cwd,
      agent,
      kind: 'ask',
      task: 'Review the latest changes',
      handoff: 'User:\nhi\n\nLead:\nworking on the packet',
    })
    assert.match(packet, /## Message from the user/)
    assert.match(packet, /Review the latest changes/)
    assert.match(packet, /You can read and modify this workspace/)
    assert.match(packet, /work iteratively/i) // nudge that keeps glm-style models from one-shotting big analysis
    assert.match(packet, /## Handoff — current session/)
    assert.match(packet, /working on the packet/)
  })
})

test('createPacket tells every agent it can work in the project', async () => {
  await withTempDir(async (cwd) => {
    const agent = await upsertAgent(cwd, {
      name: 'Builder',
      kind: 'claude-code',
    })
    const packet = await createPacket({
      cwd,
      agent,
      kind: 'ask',
      task: 'add a health check endpoint',
    })
    assert.match(packet, /You can read and modify this workspace/)
    assert.match(packet, /work iteratively/i) // nudge that keeps glm-style models from one-shotting big analysis
    assert.doesNotMatch(packet, /Read-only:/)
  })
})

test('agent presets mirror consensflow-pi exactly (image preset included)', () => {
  assert.deepEqual(listPresetIds(), [
    'calliope',
    'clio',
    'euterpe',
    'thalia',
    'hyperion',
    'phoebus',
    'gaia',
    'diana',
    'aether',
    'rhea',
    'phoebe',
    'sunna',
    'jord',
    'bil',
    'zeus',
    'apollo',
    'artemis',
    'orpheus',
    'linus',
    'erato',
    'saga',
    'gunnlod',
    'kvasir',
    'kronos',
    'atlas',
    'baldr',
    'vali',
    'hermod',
    'nike',
    'freya',
    'zephyros',
    'sif',
    'hades',
    'helios',
    'ares',
    'hephaestus',
    'athena',
    'metis',
    'prometheus',
    'endymion',
    'nyx',
    'oceanus',
    'triton',
    'odin',
    'heimdall',
    'thor',
    'tyr',
    'bragi',
    'mimir',
    'mani',
    'nott',
    'ymir',
    'aegir',
    'ilmarinen',
    'seppo',
    'ahti',
    'pygmalion',
  ])
  // All four engines are integrated, same as consensflow-pi — plus the Codex-backend image kind.
  const kinds = new Set(AGENT_PRESETS.map((preset) => preset.kind))
  assert.deepEqual([...kinds].sort(), ['claude-code', 'codex', 'image', 'kimi', 'opencode', 'pi'])
  assert.equal(getPreset('pygmalion').kind, 'image')
  assert.equal(getPreset('zeus').kind, 'claude-code')
  assert.equal(getPreset('gaia').model, 'gpt-5.6-terra')
  // max, not xhigh: K3 takes low/high/max — see "Effort ceilings" in hosts/lib/presets.js.
  assert.equal(getPreset('endymion').thinking, 'max')
  // GPT 5.5 was retired in 1.7.0 — every GPT agent now runs a 5.6 variant.
  assert.ok(!AGENT_PRESETS.some((p) => p.kind !== 'image' && String(p.model).includes('gpt-5.5')))
  // The frontier matrix: same model+effort family on every engine that runs it.
  assert.equal(getPreset('artemis').effort, 'medium')
  assert.equal(getPreset('hyperion').effort, 'ultra')
  assert.equal(getPreset('kronos').model, 'anthropic/claude-opus-5')
  assert.equal(getPreset('baldr').model, 'openrouter/anthropic/claude-opus-5')
  assert.equal(getPreset('saga').model, 'openrouter/anthropic/claude-fable-5')
  // Effort vocabularies are engine-real: claude-code tops out at "max", Codex's GPT 5.6 ladder
  // adds "ultra" above xhigh, and a model whose catalog declares no effort parameter at all
  // (MiniMax M3, Laguna S 2.1) carries none — see "Effort ceilings" in hosts/lib/presets.js.
  assert.equal(getPreset('baldr').effort, 'xhigh')
  assert.equal(getPreset('mani').effort, 'max')
  assert.equal(getPreset('mimir').effort, undefined)
  assert.equal(getPreset('aegir').effort, undefined)
  // GPT 5.6 celestial trio on Codex: Sol (flagship) gets ultra + xhigh, Terra and Luna get xhigh.
  assert.equal(getPreset('hyperion').kind, 'codex')
  assert.equal(getPreset('hyperion').model, 'gpt-5.6-sol')
  assert.equal(getPreset('hyperion').effort, 'ultra')
  assert.equal(getPreset('phoebus').model, 'gpt-5.6-sol')
  assert.equal(getPreset('phoebus').effort, 'xhigh')
  assert.equal(getPreset('gaia').model, 'gpt-5.6-terra')
  assert.equal(getPreset('gaia').effort, 'xhigh')
  assert.equal(getPreset('diana').model, 'gpt-5.6-luna')
  assert.equal(getPreset('diana').effort, 'xhigh')
  // The same trio on the other two engines that reach it: pi through the openai-codex
  // (ChatGPT) login, opencode through OpenRouter. Both carry xhigh.
  assert.equal(getPreset('aether').kind, 'pi')
  assert.equal(getPreset('aether').model, 'openai-codex/gpt-5.6-sol')
  assert.equal(getPreset('aether').thinking, 'xhigh')
  assert.equal(getPreset('rhea').model, 'openai-codex/gpt-5.6-terra')
  assert.equal(getPreset('phoebe').model, 'openai-codex/gpt-5.6-luna')
  assert.equal(getPreset('sunna').kind, 'opencode')
  assert.equal(getPreset('sunna').model, 'openrouter/openai/gpt-5.6-sol')
  assert.equal(getPreset('sunna').effort, 'xhigh')
  assert.equal(getPreset('jord').model, 'openrouter/openai/gpt-5.6-terra')
  assert.equal(getPreset('bil').model, 'openrouter/openai/gpt-5.6-luna')
  // Kimi K3 runs on both engines: endymion (pi, xhigh thinking → K3 max via ~/.pi/agent/models.json)
  // and mani (opencode, no catalog variants → no effort flag).
  assert.equal(getPreset('endymion').kind, 'pi')
  assert.equal(getPreset('endymion').model, 'openrouter/moonshotai/kimi-k3')
  // max, not xhigh: K3 takes low/high/max — see "Effort ceilings" in hosts/lib/presets.js.
  assert.equal(getPreset('endymion').thinking, 'max')
  assert.equal(getPreset('mani').kind, 'opencode')
  assert.equal(getPreset('mani').model, 'openrouter/moonshotai/kimi-k3')
  // The twin of endymion, so the same level: one name, one meaning, both harnesses.
  assert.equal(getPreset('mani').effort, 'max')
  // Kimi K2.7 Code was retired in 1.9.0 (K3 supersedes it); Kimi is K3-only on both engines now.
  // The OpenRouter list stays K3-only; on the kimi harness itself the
  // code-specialist K2.7 models are the newest of a family K3 has no
  // counterpart for, and K2.6 (which K3 does supersede) is left out.
  assert.ok(!AGENT_PRESETS.some((p) => String(p.model).includes('openrouter/moonshotai/kimi-k2')))
  assert.ok(!AGENT_PRESETS.some((p) => String(p.model).includes('kimi-k2.6')))
  assert.equal(getPreset('endymion').model, 'openrouter/moonshotai/kimi-k3')
  assert.equal(getPreset('endymion').kind, 'pi')
  assert.equal(getPreset('mani').model, 'openrouter/moonshotai/kimi-k3')
  assert.equal(getPreset('mani').kind, 'opencode')
  // Grok and Gemini Flash track the current generation too (4.3 -> 4.5, 3.5 -> 3.6).
  assert.equal(getPreset('ares').model, 'openrouter/x-ai/grok-4.6')
  assert.equal(getPreset('thor').model, 'openrouter/x-ai/grok-4.6')
  assert.equal(getPreset('nike').model, 'openrouter/google/gemini-3.7-flash')
  assert.equal(getPreset('sif').model, 'openrouter/google/gemini-3.7-flash')
  // GLM 5.3 on pi (Greek model zoo), at the model's ceiling like every other single-tier preset.
  assert.equal(getPreset('prometheus').kind, 'pi')
  assert.equal(getPreset('prometheus').model, 'openrouter/z-ai/glm-5.3')
  assert.equal(getPreset('prometheus').thinking, 'max')
  // Gemini 3.1 Pro and 3.7 Flash both stop at high — there is nothing above it to ask for.
  assert.equal(getPreset('heimdall').effort, 'high')
  assert.equal(getPreset('sif').effort, 'high')
  // Fable 5 family follows the same rules: claude-code gets max, the rest cap at xhigh.
  assert.equal(getPreset('calliope').effort, 'max')
  assert.equal(getPreset('calliope').model, 'claude-fable-5')
  assert.equal(getPreset('orpheus').model, 'anthropic/claude-fable-5')
  assert.equal(getPreset('saga').model, 'openrouter/anthropic/claude-fable-5')
  assert.equal(getPreset('saga').effort, 'xhigh')
  assert.equal(getPreset('euterpe').effort, 'high')
  assert.equal(getPreset('linus').thinking, 'high')
  assert.equal(getPreset('gunnlod').effort, 'high')
  const mani = agentFromPreset('mani', { cwd: 'frontend' })
  assert.equal(mani.id, 'mani')
  assert.equal(mani.name, 'Mani')
  assert.equal(mani.cwd, 'frontend')
  assert.equal(agentFromPreset('custom'), null)
})

// The selene/daedalus duplicate (same engine + model + tier under two names) sat in the catalog
// unnoticed until 1.9.0 retired it. Pin the structural invariants so the next one is caught here.
test('catalog invariants: unique mentions, no duplicate backends', () => {
  const ids = AGENT_PRESETS.map((preset) => preset.preset)
  assert.equal(new Set(ids).size, ids.length, 'preset ids are unique')
  // getAgent resolves an @ref by id OR slugified name, so one preset's name slug must never
  // shadow another's id (same rule state.js enforces on the roster via assertUniqueAgents).
  const claimed = new Map()
  for (const preset of AGENT_PRESETS) {
    for (const mention of new Set([preset.id, slugify(preset.name)].filter(Boolean))) {
      assert.ok(
        !claimed.has(mention),
        `@${preset.preset} collides with @${claimed.get(mention)} on '${mention}'`,
      )
      claimed.set(mention, preset.preset)
    }
  }
  const backends = AGENT_PRESETS.map((preset) =>
    [preset.kind, preset.model, preset.effort ?? '', preset.thinking ?? ''].join('|'),
  )
  assert.deepEqual(
    backends.filter((b, i) => backends.indexOf(b) !== i),
    [],
    'no two presets share an identical engine+model+tier',
  )
  assert.equal(
    AGENT_PRESETS.length,
    new Set(AGENT_PRESETS.map((p) => p.label)).size,
    'labels are unique too',
  )
})

test('every preset survives normalize + runner invocation with correct flags (all models × all engines)', () => {
  const KIND_COMMAND = {
    pi: 'pi',
    'claude-code': 'claude',
    codex: 'codex',
    opencode: 'opencode',
    kimi: 'kimi',
  }
  for (const preset of AGENT_PRESETS) {
    const agent = normalizeAgent(agentFromPreset(preset.preset))
    assert.equal(agent.id, preset.id, `${preset.preset}: id survives the pipeline`)
    assert.equal(agent.kind, preset.kind, `${preset.preset}: kind`)
    assert.equal(agent.model, preset.model, `${preset.preset}: model`)

    if (preset.kind === 'image') {
      assert.throws(() => buildRunnerInvocation(agent, '/tmp/packet.md', '/repo'), /image agents/)
      continue
    }
    const invocation = buildRunnerInvocation(agent, '/tmp/packet.md', '/repo')
    assert.equal(invocation.command, KIND_COMMAND[preset.kind], `${preset.preset}: engine command`)
    assert.equal(invocation.env?.CONSENSFLOW_CHILD, '1', `${preset.preset}: child marker env`)
    const modelIdx = invocation.args.indexOf(preset.model)
    assert.ok(modelIdx > 0, `${preset.preset}: model reaches the args`)
    // Kimi Code spells it `-m`; every other engine spells it `--model`.
    assert.equal(
      invocation.args[modelIdx - 1],
      preset.kind === 'kimi' ? '-m' : '--model',
      `${preset.preset}: model flag`,
    )

    if (preset.kind === 'claude-code') {
      assert.equal(
        invocation.args[invocation.args.indexOf('--effort') + 1],
        preset.effort,
        `${preset.preset}: claude effort`,
      )
      assert.equal(
        invocation.args.includes('--disallowedTools'),
        false,
        `${preset.preset}: no claude deny list`,
      )
      assert.ok(
        !invocation.args.includes('--bare'),
        `${preset.preset}: --bare must NOT be passed (blocks OAuth/keychain auth; CONSENSFLOW_CHILD is the recursion guard)`,
      )
    }
    if (preset.kind === 'codex') {
      assert.ok(
        invocation.args.includes(`model_reasoning_effort="${preset.effort}"`),
        `${preset.preset}: codex effort`,
      )
      assert.ok(
        invocation.args.includes('--dangerously-bypass-approvals-and-sandbox'),
        `${preset.preset}: codex runs unsandboxed`,
      )
    }
    if (preset.kind === 'opencode') {
      if (preset.effort)
        assert.equal(
          invocation.args[invocation.args.indexOf('--variant') + 1],
          preset.effort,
          `${preset.preset}: opencode variant`,
        )
      else
        assert.equal(
          invocation.args.includes('--variant'),
          false,
          `${preset.preset}: no catalog variant → no flag`,
        )
      assert.equal(
        invocation.env?.OPENCODE_PERMISSION,
        undefined,
        `${preset.preset}: no opencode permission overlay`,
      )
    }
    if (preset.kind === 'pi') {
      assert.equal(
        invocation.args[invocation.args.indexOf('--thinking') + 1],
        preset.thinking ?? 'off',
        `${preset.preset}: pi thinking`,
      )
      assert.equal(
        invocation.args[invocation.args.indexOf('--tools') + 1],
        'read,grep,find,ls,bash,edit,write',
        `${preset.preset}: pi full tools (no read-only sandbox in pi)`,
      )
    }
  }
})

test('runner invocation maps tool policies', () => {
  assert.equal(toolsForPi(), 'read,grep,find,ls,bash,edit,write') // pi always full tools (no read-only bash sandbox)
  const pi = buildRunnerInvocation(
    { kind: 'pi', model: 'openrouter/moonshotai/kimi-k3', skillsPolicy: 'default' },
    '/tmp/packet.md',
    '/repo',
  )
  assert.equal(pi.command, 'pi')
  assert.deepEqual(pi.args.slice(0, 6), [
    '--mode',
    'json',
    '--no-session',
    '--no-extensions',
    '--model',
    'openrouter/moonshotai/kimi-k3',
  ])
  assert.ok(pi.args.includes('off'))
  assert.equal(pi.args.includes('--no-skills'), false)
  const sterilePi = buildRunnerInvocation(
    { kind: 'pi', skillsPolicy: 'none' },
    '/tmp/packet.md',
    '/repo',
  )
  assert.ok(sterilePi.args.includes('--no-skills'))
  const codex = buildRunnerInvocation(
    { kind: 'codex', model: 'gpt-5.5', effort: 'xhigh' },
    '/tmp/packet.md',
    '/repo',
  )
  assert.equal(codex.command, 'codex')
  assert.ok(codex.args.includes('--dangerously-bypass-approvals-and-sandbox'))
  assert.ok(codex.args.includes('--ephemeral'))
  assert.ok(codex.args.includes('--skip-git-repo-check'))
  assert.ok(codex.args.includes('--ignore-user-config'))
  assert.ok(codex.args.includes('--ignore-rules'))
  assert.ok(codex.args.includes('model_reasoning_effort="xhigh"'))
})

test('every engine runs with full permissions: no sandbox, no allowlist, no prompts', () => {
  // Claude: every tool, no permission prompts.
  const claude = buildRunnerInvocation({ kind: 'claude-code' }, '/tmp/packet.md', '/repo')
  assert.ok(claude.args.includes('--dangerously-skip-permissions'))
  assert.equal(claude.args.includes('--allowedTools'), false, 'no allowlist fences the tools')
  assert.equal(claude.args.includes('--disallowedTools'), false)
  // Regression guard (1.5.1): --bare forbids OAuth/keychain auth while ANTHROPIC_API_KEY is
  // stripped -> "Not logged in". Recursion/stomp is guarded by CONSENSFLOW_CHILD alone.
  assert.ok(!claude.args.includes('--bare'))
  assert.ok(claude.args.includes('--no-session-persistence'))

  // Codex: no sandbox — full disk and network, like running codex yourself.
  const codex = buildRunnerInvocation({ kind: 'codex' }, '/tmp/packet.md', '/repo')
  assert.ok(codex.args.includes('--dangerously-bypass-approvals-and-sandbox'))

  // OpenCode: auto-approves anything not explicitly denied.
  const opencode = buildRunnerInvocation({ kind: 'opencode' }, '/tmp/packet.md', '/repo')
  assert.ok(opencode.args.includes('--auto'))
  assert.equal(opencode.env?.OPENCODE_PERMISSION, undefined)

  // Full permissions everywhere: the engines that fence by default are told
  // not to. pi needs no flag (its tools are on by default) and must not grow
  // one, so it is checked from the other side.
  const FULL = {
    'claude-code': '--dangerously-skip-permissions',
    codex: '--dangerously-bypass-approvals-and-sandbox',
    opencode: '--auto',
  }
  for (const kind of ['pi', 'claude-code', 'codex', 'opencode']) {
    const invocation = buildRunnerInvocation({ kind }, '/tmp/packet.md', '/repo')
    if (FULL[kind] !== undefined) {
      assert.ok(invocation.args.includes(FULL[kind]), `${kind}: ${FULL[kind]}`)
    } else {
      assert.equal(
        invocation.args.some((a) => String(a).includes('--dangerously')),
        false,
        `${kind}: needs no bypass flag`,
      )
    }
    assert.equal(invocation.env?.CONSENSFLOW_CHILD, '1', `${kind}: CONSENSFLOW_CHILD`)
  }

  // Billing guard: agent runs ride the configured logins, not a stray env API key.
  assert.deepEqual(claude.dropEnv, ['ANTHROPIC_API_KEY'])
  assert.deepEqual(codex.dropEnv, ['OPENAI_API_KEY'])
})

test('image agents are valid config but never reach the CLI runner (backstop)', () => {
  // Image generation is handled upstream in cf.mjs (Codex backend); the runner must throw loudly
  // if one ever slips through to the spawn path.
  const agent = normalizeAgent({ name: 'Pygmalion', kind: 'image' })
  assert.equal(agent.kind, 'image')
  assert.throws(() => buildRunnerInvocation(agent, '/tmp/packet.md', '/repo'), /Codex backend/)
})

test('spawnWithInput strips the cmux control-socket env from every child, even when passed as an override', async () => {
  // CMUX_SOCKET_CAPABILITY is a bearer token that can type into any cmux pane; no child may hold it.
  const probe =
    'process.stdout.write(JSON.stringify({ cap: process.env.CMUX_SOCKET_CAPABILITY ?? null, bin: process.env.CMUX_CLAUDE_HOOK_CMUX_BIN ?? null, keep: process.env.CONSENSFLOW_TEST_KEEP ?? null }))'
  const result = await spawnWithInput(process.execPath, ['-e', probe], {
    timeoutMs: 10_000,
    env: {
      CMUX_SOCKET_CAPABILITY: 'leak-test',
      CMUX_CLAUDE_HOOK_CMUX_BIN: 'leak-test',
      CONSENSFLOW_TEST_KEEP: 'kept',
    },
  })
  const seen = JSON.parse(result.stdout)
  assert.equal(seen.cap, null, 'cmux socket token must never reach a child')
  assert.equal(seen.bin, null, 'cmux hook binary path must never reach a child')
  assert.equal(seen.keep, 'kept', 'unrelated env overrides still pass through')
})

test('spawnWithInput survives a child that exits without reading stdin (EPIPE)', async () => {
  // `true` exits immediately without consuming the 5MB packet; the stdin pipe raises EPIPE,
  // which must be captured, not thrown as an uncaughtException that kills the host.
  const result = await spawnWithInput('true', [], {
    input: 'x'.repeat(5 * 1024 * 1024),
    timeoutMs: 10_000,
  })
  assert.equal(result.exitCode, 0)
  assert.equal(result.timedOut, false)
})

test('spawnWithInput with timeoutMs:0 arms no timer — a child that runs past 0ms completes normally, not timed out', async () => {
  // Under the old setTimeout(...,0) behavior this child would have been SIGTERM'd at ~0ms
  // (timedOut:true). With the no-timeout guard it runs to completion.
  const result = await spawnWithInput(
    process.execPath,
    ['-e', "setTimeout(() => process.stdout.write('done'), 120)"],
    { timeoutMs: 0 },
  )
  assert.equal(result.timedOut, false)
  assert.equal(result.exitCode, 0)
  assert.equal(result.stdout, 'done')
})

test('spawnWithInput streams complete stdout lines via onStdoutLine: carry across splits, CRLF-stripped, blanks skipped, newline-less tail flushed [STRM-03]', async () => {
  await withTempDir(async (dir) => {
    const fake = path.join(dir, 'chunker.mjs')
    // Emits the byte stream one env-provided chunk at a time, with a gap between writes so the
    // partial-line carry path is actually exercised (the parent sees separate `data` events).
    await writeFile(
      fake,
      [
        'const chunks = JSON.parse(process.env.CHUNKS);',
        'let i = 0;',
        'const next = () => { if (i >= chunks.length) return; process.stdout.write(chunks[i++]); setTimeout(next, 30); };',
        'next();',
      ].join('\n'),
      'utf8',
    )

    // A whole line + the start of a second (carry), the rest of line 2 ending CRLF,
    // a blank line (must be skipped), and a final line with NO trailing newline.
    const chunks = ['{"n":1}\n{"par', 'tial":2}\r\n', '\n', '{"tail":3}']
    const lines = []
    const result = await spawnWithInput(process.execPath, [fake], {
      timeoutMs: 10_000,
      env: { CHUNKS: JSON.stringify(chunks) },
      onStdoutLine: (line) => lines.push(line),
    })

    assert.deepEqual(
      lines,
      ['{"n":1}', '{"partial":2}', '{"tail":3}'],
      'complete lines, CRLF stripped, blank skipped, tail flushed',
    )
    assert.equal(
      result.stdout,
      chunks.join(''),
      'buffered stdout bytes are unchanged by the line callback',
    )
    assert.equal(result.exitCode, 0)
    assert.equal(result.timedOut, false)
  })
})

test('an agent carries no permission field at all', () => {
  // The permission concept was removed 2026-08-20: agents run with the
  // engine CLI's own defaults, so there is nothing to store and nothing to
  // escalate. A legacy value on an old roster row is simply not read.
  const p = normalizeAgent({ name: 'Y', kind: 'codex' })
  assert.equal(p.toolsPolicy, undefined)
  const legacy = normalizeAgent({ name: 'Z', kind: 'codex', toolsPolicy: 'full-auto' })
  assert.equal(legacy.toolsPolicy, undefined)
})

test('agentsPath and artifact root live directly under the shared ConsensFlow home [STRM-27]', async () => {
  await withTempDir(async (cwd) => {
    const home = process.env.CONSENSFLOW_HOME
    assert.equal(configRoot(), home)
    assert.equal(agentsPath(cwd), path.join(home, 'agents.json'))
    assert.ok(
      !agentsPath(cwd).includes('consensflow-cc') && !agentsPath(cwd).includes('consensflow-pi'),
      'roster not under a per-tool subdir',
    )

    await upsertAgent(cwd, { name: 'Shared', kind: 'codex', model: 'gpt-5.5' })
    assert.equal(
      JSON.parse(await readFile(path.join(home, 'agents.json'), 'utf8')).agents.length,
      1,
    )
    assert.equal((await getAgent(cwd, '@shared')).model, 'gpt-5.5')
  })
})

test('legacy per-tool agent files migrate once when the shared root file is missing [STRM-27]', async () => {
  await withTempDir(async (cwd) => {
    const home = process.env.CONSENSFLOW_HOME
    await mkdir(path.join(home, 'consensflow-pi'), { recursive: true })
    await writeFile(
      path.join(home, 'consensflow-pi', 'agents.json'),
      JSON.stringify({
        schemaVersion: 1,
        agents: [{ id: 'pi-only', name: 'Pi Only', kind: 'pi' }],
      }),
      'utf8',
    )
    await mkdir(path.join(home, 'consensflow-cc'), { recursive: true })
    await writeFile(
      path.join(home, 'consensflow-cc', 'agents.json'),
      JSON.stringify({
        schemaVersion: 1,
        agents: [{ id: 'cc-only', name: 'CC Only', kind: 'claude-code' }],
      }),
      'utf8',
    )

    assert.deepEqual((await loadAgents(cwd)).map((p) => p.id).sort(), ['cc-only', 'pi-only'])
    const root = JSON.parse(await readFile(path.join(home, 'agents.json'), 'utf8'))
    assert.deepEqual(root.agents.map((p) => p.id).sort(), ['cc-only', 'pi-only'])
    assert.equal((await getAgent(cwd, '@cc-only')).kind, 'claude-code')

    // After the shared file exists, legacy files are ignored; root remains authoritative.
    await writeFile(
      path.join(home, 'consensflow-cc', 'agents.json'),
      JSON.stringify({
        schemaVersion: 1,
        agents: [{ id: 'ghost', name: 'Ghost', kind: 'codex' }],
      }),
      'utf8',
    )
    assert.equal(await getAgent(cwd, '@ghost'), null)
    await upsertAgent(cwd, { name: 'Root Only', kind: 'opencode', model: 'openrouter/test' })
    assert.deepEqual((await loadAgents(cwd)).map((p) => p.id).sort(), [
      'cc-only',
      'pi-only',
      'root-only',
    ])
    assert.equal((await getAgent(cwd, '@root-only')).model, 'openrouter/test')
  })
})

test('runAgent rejects agent cwd that escapes workspace before spawning', async () => {
  await withTempDir(async (cwd) => {
    await assert.rejects(
      runAgent({
        cwd,
        agent: { id: 'bad', name: 'Bad', kind: 'pi', cwd: '../outside' },
        packet: '# Packet',
        kind: 'ask',
      }),
      /Path escapes workspace/,
    )
  })
})

test('normalizeProcessOutput parses Claude JSON result', () => {
  const out = normalizeProcessOutput(
    'claude-code',
    JSON.stringify({ type: 'result', result: 'OK' }),
    '',
  )
  assert.equal(out.output, 'OK')
})

test('normalizeProcessOutput parses Claude JSON event array result', () => {
  const out = normalizeProcessOutput(
    'claude-code',
    JSON.stringify([
      { type: 'system' },
      { type: 'assistant', message: { content: [{ type: 'text', text: 'draft' }] } },
      { type: 'result', result: 'CLAUDE FINAL' },
    ]),
    '',
  )
  assert.equal(out.output, 'CLAUDE FINAL')
})

test('normalizeProcessOutput parses Codex JSONL harness message text', () => {
  const stdout = [
    JSON.stringify({ type: 'thread.started', thread_id: 't' }),
    JSON.stringify({ type: 'item.completed', item: { type: 'agent_message', text: 'draft' } }),
    JSON.stringify({
      type: 'item.completed',
      item: { type: 'agent_message', text: 'CODEX FINAL' },
    }),
  ].join('\n')
  const out = normalizeProcessOutput('codex', stdout, '')
  assert.equal(out.output, 'CODEX FINAL')
})

test('normalizeProcessOutput parses Pi JSON mode final assistant text', () => {
  const stdout = [
    JSON.stringify({ type: 'session', id: 's' }),
    JSON.stringify({
      type: 'message_end',
      message: { role: 'assistant', content: [{ type: 'text', text: 'PI OK' }] },
    }),
    JSON.stringify({
      type: 'agent_end',
      messages: [{ role: 'assistant', content: [{ type: 'text', text: 'PI FINAL' }] }],
    }),
  ].join('\n')
  const out = normalizeProcessOutput('pi', stdout, '')
  assert.equal(out.output, 'PI FINAL')
})

test('normalizeProcessOutput parses Pi JSON mode from a truncated tail', () => {
  const stdout = [
    '[truncated: kept tail]',
    JSON.stringify({
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'ignored' },
    }),
    JSON.stringify({
      type: 'agent_end',
      messages: [{ role: 'assistant', content: [{ type: 'text', text: 'TAIL FINAL' }] }],
    }),
  ].join('\n')
  const out = normalizeProcessOutput('pi', stdout, '')
  assert.equal(out.output, 'TAIL FINAL')
})

// [STRM-02] Kimi Code's stream is the chat shape — {role, content} — which carries none of
// the keys the generic extractor looks for (`result`, `output`, `text`). Without a branch of
// its own, every kimi run handed back its ENTIRE stream as "the answer": a real one measured
// 493,390 characters, and `cf last` pasted all of it into the lead that asked (found
// 2026-08-26 on a run captured 2026-08-24, the fixture below).
test('normalizeProcessOutput: kimi returns the last thing said, never the protocol log [STRM-02]', async () => {
  const fixture = await readFile(
    new URL('./fixtures/kimi-cutoff.sample.jsonl', import.meta.url),
    'utf8',
  )

  const real = normalizeProcessOutput('kimi', fixture, '').output

  assert.match(real, /Evals are solid/, 'the last assistant turn that actually said something')
  assert.doesNotMatch(real, /"role":\s*"tool"|system\.version/, 'never the raw stream')
  assert.ok(real.length < 500, `an answer, not a log (got ${real.length} chars)`)

  // A run that died among its tool calls understood its stream and found no answer in it:
  // that is a short placeholder, never the log and never empty.
  const noAnswer = [
    JSON.stringify({ role: 'meta', type: 'system.version', version: '0.38.0' }),
    JSON.stringify({
      role: 'assistant',
      content: '',
      tool_calls: [{ type: 'function', id: 'Bash_0' }],
    }),
    JSON.stringify({ role: 'tool', tool_call_id: 'Bash_0', content: 'output' }),
  ].join('\n')

  const empty = normalizeProcessOutput('kimi', noAnswer, '').output

  assert.ok(empty.trim().length > 0 && empty.length < 200, 'short placeholder')
  assert.doesNotMatch(empty, /system\.version/, 'never the raw stream')
})

// [STRM-01] OpenCode answer text rides in `part.text` on `type:"text"` events (never a
// top-level `{text}` — that shape was a fiction; the real captured fixture disproves it).
// The blank-output bug returned only the LAST text part, which on a timed-out run is a
// trailing whitespace fragment. The fix concatenates all text parts in order and falls
// back to a short placeholder (NO_ANSWER) — never the raw JSONL stream.
test('normalizeProcessOutput: OpenCode concats ordered text parts, never the trailing fragment or raw JSONL [STRM-01]', async () => {
  // (1) Real captured timeout fixture: the bug returned the trailing " " fragment and
  // discarded the substantive first text part.
  const fixture = await readFile(
    new URL('./fixtures/opencode-timeout.sample.jsonl', import.meta.url),
    'utf8',
  )
  const real = normalizeProcessOutput('opencode', fixture, '').output
  assert.match(real, /continue from where the lead left off/, 'keeps the substantive text part')
  assert.ok(real.trim().length > 1, "not the bare ' ' trailing fragment")
  assert.doesNotMatch(
    real,
    /"type":\s*"step_start"|sessionID/,
    'never the raw JSONL stream as the answer',
  )

  // (2) All-empty / whitespace-only text parts → a short clean placeholder, never the raw stream.
  const emptyStream = [
    JSON.stringify({ type: 'text', part: { text: '' } }),
    JSON.stringify({ type: 'text', part: { text: '   ' } }),
    JSON.stringify({ type: 'step_finish', part: { type: 'step-finish' } }),
  ].join('\n')
  const empty = normalizeProcessOutput('opencode', emptyStream, '').output
  assert.ok(
    empty.trim().length > 0 && empty.length < 200,
    'short clean placeholder, not empty/whitespace',
  )
  assert.doesNotMatch(empty, /"type"|sessionID|step_finish/, 'placeholder is not the raw JSONL')

  // (3) Multiple real-shaped text parts concatenate in order; interleaved tool events are ignored.
  const multi = [
    JSON.stringify({ type: 'text', part: { text: 'Hello ' } }),
    JSON.stringify({ type: 'tool_use', part: { type: 'tool', tool: 'read' } }),
    JSON.stringify({ type: 'text', part: { text: 'world' } }),
  ].join('\n')
  assert.equal(normalizeProcessOutput('opencode', multi, '').output, 'Hello world')
})

test('agentFromPreset can rename while keeping the backend', () => {
  const renamed = agentFromPreset('zeus', { name: 'Deepreview' })
  assert.equal(renamed.id, 'deepreview')
  assert.equal(renamed.name, 'Deepreview')
  assert.equal(renamed.kind, 'claude-code')
  assert.equal(renamed.model, 'claude-opus-5')
  assert.equal(renamed.preset, 'zeus')
  // Without a rename, the canonical preset id and name are kept.
  const mani = agentFromPreset('mani')
  assert.equal(mani.id, 'mani')
  assert.equal(mani.name, 'Mani')
})

test('parseAgentPrompt routes one mention anywhere, and never hijacks stray @tokens', () => {
  const known = new Set(['zeus', 'athena'])
  // Leading and trailing single mention are equivalent.
  assert.deepEqual(parseAgentPrompt(['@zeus', 'hi'], known), {
    agent: 'zeus',
    prompt: 'hi',
  })
  assert.deepEqual(parseAgentPrompt(['hi', '@zeus'], known), {
    agent: 'zeus',
    prompt: 'hi',
  })
  assert.deepEqual(parseAgentPrompt(['summarize', '@zeus', 'please'], known), {
    agent: 'zeus',
    prompt: 'summarize please',
  })
  // "ask"/"to" verb prefix still addresses a leading agent.
  assert.deepEqual(parseAgentPrompt(['ask', '@athena', 'review'], known), {
    agent: 'athena',
    prompt: 'review',
  })
  // A leading mention wins and later @names stay as quoted text (paste-prior-output intact).
  assert.deepEqual(parseAgentPrompt(['@athena', 'agree', 'with', '@zeus?'], known), {
    agent: 'athena',
    prompt: 'agree with @zeus?',
  })
  // Multiple leading mentions are rejected.
  assert.ok(parseAgentPrompt(['@zeus', '@athena', 'hi'], known)?.error)
  // A stray non-leading @token that is not an agent goes to the lead, not a subprocess.
  assert.equal(parseAgentPrompt(['install', '@types/node', 'now'], known), null)
  // Two different agents, none leading -> ambiguous, lead handles.
  assert.equal(parseAgentPrompt(['compare', '@zeus', 'and', '@athena'], known), null)
  // No mention at all.
  assert.equal(parseAgentPrompt(['just', 'fix', 'the', 'bug'], known), null)
  // Leading mention without a prompt errors helpfully.
  assert.ok(parseAgentPrompt(['@zeus'], known)?.error)
  // Without a known-set, a non-leading mention does not route (conservative default).
  assert.equal(parseAgentPrompt(['hi', '@zeus']), null)
})

test('resolveInside rejects symlinked escapes, not just lexical ../ ones', async () => {
  await withTempDir(async (dir) => {
    const ws = path.join(dir, 'ws')
    const outside = path.join(dir, 'outside')
    await mkdir(path.join(ws, 'sub'), { recursive: true })
    await mkdir(outside, { recursive: true })
    await symlink(outside, path.join(ws, 'link'), 'dir')
    assert.throws(() => resolveInside(ws, '../outside'), /escapes workspace/)
    // ws/link points outside the workspace: lexical containment passes, realpath must not.
    assert.throws(() => resolveInside(ws, 'link'), /escapes workspace/)
    // Normal subdirs — existing or not-yet-created — still resolve.
    assert.ok(resolveInside(ws, 'sub'))
    assert.ok(resolveInside(ws, 'brand-new/dir'))
  })
})

// The roster snapshots a preset's engine fields, so a ConsensFlow update that ships a new catalog
// (Opus 4.8 -> Opus 5) does not reach agents added under the old one. `agents sync`
// re-resolves them; this pins down exactly what it may and may not touch.
test('agents sync re-resolves preset-backed entries and leaves everything else pinned', async () => {
  await withTempDir(async (dir) => {
    // Added under an older catalog: stale model + effort, plus a rename and a cwd to preserve.
    await upsertAgent(dir, {
      name: 'Deepreview',
      id: 'deepreview',
      kind: 'claude-code',
      model: 'claude-opus-4-8',
      effort: 'medium',
      cwd: 'backend',
      preset: 'zeus',
      description: 'stale text from an older catalog',
    })
    // A hand-rolled agent: no preset, so sync must never rewrite it.
    await upsertAgent(dir, {
      name: 'Builder',
      kind: 'codex',
      model: 'my-own-model',
      effort: 'high',
    })
    // Names a preset the catalog no longer carries — stays pinned to what it was created with.
    await upsertAgent(dir, {
      name: 'Ghost',
      kind: 'codex',
      model: 'gpt-5.5',
      effort: 'xhigh',
      preset: 'retired-preset',
    })
    const current = await upsertAgent(dir, agentFromPreset('nike'))

    const preview = await syncAgentsWithPresets(dir, { dryRun: true })
    assert.equal(preview.synced.length, 1, 'only the drifted preset-backed entry is reported')
    assert.equal(preview.synced[0].id, 'deepreview')
    assert.deepEqual(preview.orphans, ['@ghost'])
    assert.equal(
      (await getAgent(dir, '@deepreview')).model,
      'claude-opus-4-8',
      'dry run writes nothing',
    )

    const result = await syncAgentsWithPresets(dir)
    assert.equal(result.synced.length, 1)
    const fields = result.synced[0].changes.map((change) => change.field).sort()
    assert.deepEqual(
      fields,
      ['effort', 'model'],
      'engine fields only — description is a user override, never synced',
    )

    const synced = await getAgent(dir, '@deepreview')
    assert.equal(synced.model, getPreset('zeus').model, 'model tracks the catalog')
    assert.equal(synced.effort, getPreset('zeus').effort, 'so does the effort tier')
    assert.equal(synced.name, 'Deepreview', 'a rename survives')
    assert.equal(synced.id, 'deepreview')
    assert.equal(synced.cwd, 'backend', 'so does a per-agent cwd')
    assert.equal(synced.preset, 'zeus')
    // `add <preset> --description` is a documented override, so sync must leave the text alone —
    // and because the two hosts word a few descriptions differently while sharing one roster,
    // syncing it would also mean the drift nudge could never be cleared.
    assert.equal(
      synced.description,
      'stale text from an older catalog',
      'a user-authored description survives',
    )

    const custom = await getAgent(dir, '@builder')
    assert.equal(custom.model, 'my-own-model', 'custom agents are never rewritten')
    assert.equal(custom.effort, 'high')
    const ghost = await getAgent(dir, '@ghost')
    assert.equal(ghost.model, 'gpt-5.5', 'an orphaned preset stays pinned')
    assert.equal(
      (await getAgent(dir, '@nike')).updatedAt,
      current.updatedAt,
      'up-to-date entries are left untouched',
    )

    const second = await syncAgentsWithPresets(dir)
    assert.equal(second.synced.length, 0, 'sync is idempotent')
    assert.deepEqual(
      driftedAgents(await loadAgents(dir)),
      [],
      'and the roster reports no drift after it',
    )
  })
})

test("upsertAgent rejects a name slug that collides with another agent's id", async () => {
  await withTempDir(async (dir) => {
    await upsertAgent(dir, { name: 'Zeus', kind: 'claude-code', model: 'claude-opus-4-8' })
    await upsertAgent(dir, { name: 'Athena', kind: 'codex', model: 'gpt-5.5' })
    // getAgent resolves by id OR name slug, so a second agent whose NAME slugifies
    // to an existing id would shadow it — must be rejected, not silently saved.
    await assert.rejects(
      upsertAgent(dir, { id: 'athena2', name: 'Zeus', kind: 'codex', model: 'gpt-5.5' }),
      /collides/,
    )
    // Updating the same agent in place stays allowed.
    await upsertAgent(dir, {
      name: 'Zeus',
      kind: 'claude-code',
      model: 'claude-opus-4-8',
      effort: 'max',
    })
  })
})

// lib parity with the consensflow-pi sibling: these files are kept byte-identical by convention
// (see AGENTS.md). A divergence means a fix landed in one project and silently missed the other.
// Both hosts share ONE roster, so any field `sync` rewrites must be identical in both catalogs —
// otherwise each host sees the other's value as drift and the nudge can never be cleared. This is
// exactly how the pygmalion description (deliberately worded differently per host) broke sync
// before 1.9.0 shipped: guard it mechanically rather than by review.
// The cross-repo parity test lived here: it imported consensflow-pi's copy of
// the engine and asserted the two matched line for line. The merge deleted the
// second copy, so the invariant it guarded is now structural.

test('parity: shared lib files stay identical with the consensflow-pi sibling', async (t) => {
  const siblingLib = new URL('../../consensflow-pi/extensions/consensflow/lib/', import.meta.url)
  for (const file of ['utils.js', 'transcript-events.js']) {
    let sibling
    try {
      sibling = await readFile(new URL(file, siblingLib), 'utf8')
    } catch {
      t.skip('consensflow-pi sibling checkout not present')
      return
    }
    const ours = await readFile(new URL(`../lib/${file}`, import.meta.url), 'utf8')
    assert.equal(
      ours,
      sibling,
      `${file} diverged from consensflow-pi — change both or document the delta in both AGENTS.md files`,
    )
  }
})

test('docs describe the stream-first observability surface, transcript backstop, and conventions [STRM-21]', async () => {
  const readme = await readFile(new URL('../../README.md', import.meta.url), 'utf8')
  const harnesses = await readFile(new URL('../../AGENTS.md', import.meta.url), 'utf8')
  const docs = `${readme}\n${harnesses}`
  // Stream-first observability surface (primary), foreground-incremental.
  assert.match(docs, /stream/i, 'docs describe the automatic live stream')
  assert.match(docs, /foreground/i, 'docs note runs are foreground')
  assert.match(docs, /never .*background/i, 'docs forbid background runs (a tool property)')
  assert.match(docs, /non-optional/i, 'docs lock foreground streaming as non-optional')
  // Durability backstop + the new parity-locked event module.
  assert.match(docs, /transcript\.md/, 'docs mention the transcript.md backstop')
  assert.match(docs, /transcript-events\.js/, 'docs mention the parity-locked event module')
  // The shared cross-tool roster (--rw is gone: it's a redundant no-op, workspace-write is the default).
  assert.match(docs, /shared/i, 'docs mention the shared cross-tool roster')
  // The runners.js mirrored-with-deltas convention.
})
