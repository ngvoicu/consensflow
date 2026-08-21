import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { serializeTranscript } from '../../hosts/lib/handoff.js'
import {
  buildImageRequestBody,
  decodeChatGptAccountId,
  extractImageFromEvents,
  imageFileToDataUrl,
  saveImagePng,
} from '../../hosts/lib/image.js'
import { createPacket } from '../../hosts/lib/packets.js'
import {
  driftedParticipants,
  getPreset,
  listPresetIds,
  PARTICIPANT_PRESETS,
  PRESET_OWNED_FIELDS,
  participantFromPreset,
} from '../../hosts/lib/presets.js'
import {
  buildRunnerInvocation,
  normalizeProcessOutput,
  runParticipant,
  spawnWithInput,
  toolsForPi,
} from '../../hosts/lib/runners.js'
import {
  configRoot,
  getParticipant,
  loadParticipants,
  normalizeParticipant,
  participantsPath,
  removeParticipant,
  syncParticipantsWithPresets,
  upsertParticipant,
} from '../../hosts/lib/state.js'
import {
  parseOptions,
  parseParticipantPrompt,
  resolveInside,
  slugify,
  tokenize,
} from '../../hosts/lib/utils.js'

async function withTempDir(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-pi-test-'))
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
  // (this is how a prompt placed after a negation flag used to get eaten).
  assert.deepEqual(parseOptions(['@zeus', '--no-handoff', 'what', 'about', 'this?']), {
    positional: ['@zeus', 'what', 'about', 'this?'],
    flags: { 'no-handoff': true },
  })
})

test('slugify creates stable mentions', () => {
  assert.equal(slugify('Zeus Opus 4.7'), 'zeus-opus-4-7')
  assert.equal(slugify(' Isis  '), 'isis')
})

test('participant CRUD persists global user-level JSON', async () => {
  await withTempDir(async (cwd) => {
    const athena = await upsertParticipant(cwd, {
      name: 'Athena',
      kind: 'codex',
      model: 'gpt-5.5',
      effort: 'xhigh',
    })
    assert.equal(athena.id, 'athena')
    assert.equal((await getParticipant(cwd, '@athena')).model, 'gpt-5.5')
    assert.equal((await loadParticipants(cwd)).length, 1)
    assert.equal(await removeParticipant(cwd, 'athena'), true)
    assert.equal((await loadParticipants(cwd)).length, 0)
  })
})

test('createPacket is conversational, mode-aware, and carries handoff + diff', async () => {
  await withTempDir(async (cwd) => {
    const participant = await upsertParticipant(cwd, {
      name: 'Zeus',
      kind: 'pi',
      model: 'openrouter/anthropic/claude-opus-4.7',
    })
    const packet = await createPacket({
      cwd,
      participant,
      kind: 'ask',
      task: 'Review the latest changes',
      handoff: 'User:\nhi\n\nLead:\nworking on the packet',
    })
    assert.match(packet, /## Message from the user/)
    assert.match(packet, /Review the latest changes/)
    assert.match(packet, /Read-write: you can read and modify this workspace/)
    assert.match(packet, /work iteratively/i) // nudge that keeps glm-style models from one-shotting big analysis
    assert.match(packet, /## Handoff — current session/)
    assert.match(packet, /working on the packet/)
    // The rigid old task template is gone.
    assert.doesNotMatch(packet, /Required output format/)
    assert.doesNotMatch(packet, /1\. Direct answer/)
    assert.doesNotMatch(packet, /Do not edit, write, or mutate files/)
  })
})

test('createPacket gives write-capable participants a read-write mode line', async () => {
  await withTempDir(async (cwd) => {
    const participant = await upsertParticipant(cwd, {
      name: 'Builder',
      kind: 'claude-code',
    })
    const packet = await createPacket({
      cwd,
      participant,
      kind: 'ask',
      task: 'add a health check endpoint',
    })
    assert.match(packet, /Read-write: you can read and modify this workspace/)
    assert.match(packet, /work iteratively/i) // nudge that keeps glm-style models from one-shotting big analysis
    assert.doesNotMatch(packet, /Read-only:/)
  })
})

test('participant presets expose the allowed creation list', () => {
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
    'metis',
    'prometheus',
    'endymion',
    'odin',
    'heimdall',
    'thor',
    'tyr',
    'mimir',
    'mani',
    'pygmalion',
  ])
  assert.equal(getPreset('zeus').kind, 'claude-code')
  assert.equal(getPreset('gaia').model, 'gpt-5.6-terra')
  assert.equal(getPreset('endymion').thinking, 'xhigh')
  assert.equal(getPreset('pygmalion').kind, 'image')
  // GPT 5.5 was retired in 1.7.0 — every GPT participant now runs a 5.6 variant.
  assert.ok(
    !PARTICIPANT_PRESETS.some((p) => p.kind !== 'image' && String(p.model).includes('gpt-5.5')),
  )
  // The frontier matrix: same model+effort family on every engine that runs it.
  assert.equal(getPreset('artemis').effort, 'medium')
  assert.equal(getPreset('hyperion').effort, 'ultra')
  assert.equal(getPreset('kronos').model, 'anthropic/claude-opus-5')
  assert.equal(getPreset('baldr').model, 'openrouter/anthropic/claude-opus-5')
  assert.equal(getPreset('saga').model, 'openrouter/anthropic/claude-fable-5')
  // Effort vocabularies are engine-real: claude-code tops out at "max", Codex's GPT 5.6 ladder
  // adds "ultra" above xhigh, the OpenRouter-backed presets stay at xhigh, and models without
  // catalog variants (e.g. the Kimi family on opencode) carry no effort at all.
  assert.equal(getPreset('baldr').effort, 'xhigh')
  assert.equal(getPreset('mani').effort, undefined)
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
  assert.equal(getPreset('endymion').thinking, 'xhigh')
  assert.equal(getPreset('mani').kind, 'opencode')
  assert.equal(getPreset('mani').model, 'openrouter/moonshotai/kimi-k3')
  assert.equal(getPreset('mani').effort, undefined)
  // Kimi K2.7 Code was retired in 1.9.0 (K3 supersedes it); Kimi is K3-only on both engines now.
  assert.ok(!PARTICIPANT_PRESETS.some((p) => String(p.model).includes('kimi-k2')))
  assert.equal(getPreset('endymion').model, 'openrouter/moonshotai/kimi-k3')
  assert.equal(getPreset('endymion').kind, 'pi')
  assert.equal(getPreset('mani').model, 'openrouter/moonshotai/kimi-k3')
  assert.equal(getPreset('mani').kind, 'opencode')
  // Grok and Gemini Flash track the current generation too (4.3 -> 4.5, 3.5 -> 3.6).
  assert.equal(getPreset('ares').model, 'openrouter/x-ai/grok-4.6')
  assert.equal(getPreset('thor').model, 'openrouter/x-ai/grok-4.6')
  assert.equal(getPreset('nike').model, 'openrouter/google/gemini-3.7-flash')
  assert.equal(getPreset('sif').model, 'openrouter/google/gemini-3.7-flash')
  // GLM 5.3 on pi (Greek model zoo, high thinking).
  assert.equal(getPreset('prometheus').kind, 'pi')
  assert.equal(getPreset('prometheus').model, 'openrouter/z-ai/glm-5.3')
  assert.equal(getPreset('prometheus').thinking, 'high')
  assert.equal(getPreset('heimdall').effort, 'high')
  assert.equal(getPreset('sif').effort, 'low')
  // Fable 5 family follows the same rules: claude-code gets max, the rest cap at xhigh.
  assert.equal(getPreset('calliope').effort, 'max')
  assert.equal(getPreset('calliope').model, 'claude-fable-5')
  assert.equal(getPreset('orpheus').model, 'anthropic/claude-fable-5')
  assert.equal(getPreset('saga').model, 'openrouter/anthropic/claude-fable-5')
  assert.equal(getPreset('saga').effort, 'xhigh')
  assert.equal(getPreset('euterpe').effort, 'high')
  assert.equal(getPreset('linus').thinking, 'high')
  assert.equal(getPreset('gunnlod').effort, 'high')
  const mani = participantFromPreset('mani', { cwd: 'frontend' })
  assert.equal(mani.id, 'mani')
  assert.equal(mani.name, 'Mani')
  assert.equal(mani.cwd, 'frontend')
  assert.equal(participantFromPreset('custom'), null)
})

// The selene/daedalus duplicate (same engine + model + tier under two names) sat in the catalog
// unnoticed until 1.9.0 retired it. Pin the structural invariants so the next one is caught here.
test('catalog invariants: unique mentions, no duplicate backends', () => {
  const ids = PARTICIPANT_PRESETS.map((preset) => preset.preset)
  assert.equal(new Set(ids).size, ids.length, 'preset ids are unique')
  // getParticipant resolves an @ref by id OR slugified name, so one preset's name slug must never
  // shadow another's id (same rule state.js enforces on the roster via assertUniqueParticipants).
  const claimed = new Map()
  for (const preset of PARTICIPANT_PRESETS) {
    for (const mention of new Set([preset.id, slugify(preset.name)].filter(Boolean))) {
      assert.ok(
        !claimed.has(mention),
        `@${preset.preset} collides with @${claimed.get(mention)} on '${mention}'`,
      )
      claimed.set(mention, preset.preset)
    }
  }
  const backends = PARTICIPANT_PRESETS.map((preset) =>
    [preset.kind, preset.model, preset.effort ?? '', preset.thinking ?? ''].join('|'),
  )
  assert.deepEqual(
    backends.filter((b, i) => backends.indexOf(b) !== i),
    [],
    'no two presets share an identical engine+model+tier',
  )
  assert.equal(
    PARTICIPANT_PRESETS.length,
    new Set(PARTICIPANT_PRESETS.map((p) => p.label)).size,
    'labels are unique too',
  )
})

test('every preset survives normalize + runner invocation with correct flags (all models × all engines)', () => {
  const KIND_COMMAND = { pi: 'pi', 'claude-code': 'claude', codex: 'codex', opencode: 'opencode' }
  for (const preset of PARTICIPANT_PRESETS) {
    const participant = normalizeParticipant(participantFromPreset(preset.preset))
    assert.equal(participant.id, preset.id, `${preset.preset}: id survives the pipeline`)
    assert.equal(participant.kind, preset.kind, `${preset.preset}: kind`)
    assert.equal(participant.model, preset.model, `${preset.preset}: model`)

    if (preset.kind === 'image') {
      assert.throws(
        () => buildRunnerInvocation(participant, '/tmp/packet.md', '/repo'),
        /image participants/,
      )
      continue
    }
    const invocation = buildRunnerInvocation(participant, '/tmp/packet.md', '/repo')
    assert.equal(invocation.command, KIND_COMMAND[preset.kind], `${preset.preset}: engine command`)
    const modelIdx = invocation.args.indexOf(preset.model)
    assert.ok(modelIdx > 0, `${preset.preset}: model reaches the args`)
    assert.equal(invocation.args[modelIdx - 1], '--model', `${preset.preset}: model flag`)

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
  }
  // Billing guard: participant runs ride the configured logins, not a stray env API key.
  assert.deepEqual(claude.dropEnv, ['ANTHROPIC_API_KEY'])
  assert.deepEqual(codex.dropEnv, ['OPENAI_API_KEY'])
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

test('a participant carries no permission field at all', () => {
  // The permission concept was removed 2026-08-20: participants run with the
  // engine CLI's own defaults, so there is nothing to store and nothing to
  // escalate. A legacy value on an old roster row is simply not read.
  const p = normalizeParticipant({ name: 'Y', kind: 'codex' })
  assert.equal(p.toolsPolicy, undefined)
  const legacy = normalizeParticipant({ name: 'Z', kind: 'codex', toolsPolicy: 'full-auto' })
  assert.equal(legacy.toolsPolicy, undefined)
})

test('participantsPath and artifact root live directly under the shared ConsensFlow home [STRM-27]', async () => {
  await withTempDir(async (cwd) => {
    const home = process.env.CONSENSFLOW_HOME
    assert.equal(configRoot(), home)
    assert.equal(participantsPath(cwd), path.join(home, 'participants.json'))
    assert.ok(
      !participantsPath(cwd).includes('consensflow-cc') &&
        !participantsPath(cwd).includes('consensflow-pi'),
      'roster not under a per-tool subdir',
    )

    await upsertParticipant(cwd, { name: 'Shared', kind: 'codex', model: 'gpt-5.5' })
    assert.equal(
      JSON.parse(await readFile(path.join(home, 'participants.json'), 'utf8')).participants.length,
      1,
    )
    assert.equal((await getParticipant(cwd, '@shared')).model, 'gpt-5.5')
  })
})

test('legacy per-tool participant files migrate once when the shared root file is missing [STRM-27]', async () => {
  await withTempDir(async (cwd) => {
    const home = process.env.CONSENSFLOW_HOME
    await mkdir(path.join(home, 'consensflow-pi'), { recursive: true })
    await writeFile(
      path.join(home, 'consensflow-pi', 'participants.json'),
      JSON.stringify({
        schemaVersion: 1,
        participants: [{ id: 'pi-only', name: 'Pi Only', kind: 'pi' }],
      }),
      'utf8',
    )
    await mkdir(path.join(home, 'consensflow-cc'), { recursive: true })
    await writeFile(
      path.join(home, 'consensflow-cc', 'participants.json'),
      JSON.stringify({
        schemaVersion: 1,
        participants: [{ id: 'cc-only', name: 'CC Only', kind: 'claude-code' }],
      }),
      'utf8',
    )

    assert.deepEqual((await loadParticipants(cwd)).map((p) => p.id).sort(), ['cc-only', 'pi-only'])
    const root = JSON.parse(await readFile(path.join(home, 'participants.json'), 'utf8'))
    assert.deepEqual(root.participants.map((p) => p.id).sort(), ['cc-only', 'pi-only'])
    assert.equal((await getParticipant(cwd, '@cc-only')).kind, 'claude-code')

    // After the shared file exists, legacy files are ignored; root remains authoritative.
    await writeFile(
      path.join(home, 'consensflow-cc', 'participants.json'),
      JSON.stringify({
        schemaVersion: 1,
        participants: [{ id: 'ghost', name: 'Ghost', kind: 'codex' }],
      }),
      'utf8',
    )
    assert.equal(await getParticipant(cwd, '@ghost'), null)
    await upsertParticipant(cwd, { name: 'Root Only', kind: 'opencode', model: 'openrouter/test' })
    assert.deepEqual((await loadParticipants(cwd)).map((p) => p.id).sort(), [
      'cc-only',
      'pi-only',
      'root-only',
    ])
    assert.equal((await getParticipant(cwd, '@root-only')).model, 'openrouter/test')
  })
})

test('runParticipant rejects participant cwd that escapes workspace before spawning', async () => {
  await withTempDir(async (cwd) => {
    await assert.rejects(
      runParticipant({
        cwd,
        participant: { id: 'bad', name: 'Bad', kind: 'pi', cwd: '../outside' },
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

test('normalizeProcessOutput parses Codex JSONL agent message text', () => {
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

// [STRM-01] OpenCode answer text rides in `part.text` on `type:"text"` events (never a
// top-level `{text}` — that shape was a fiction; the real captured fixture disproves it).
// The blank-output bug returned only the LAST text part, which on a timed-out run is a
// trailing whitespace fragment. The fix concatenates all text parts in order and falls
// back to a short placeholder (OPENCODE_NO_ANSWER) — never the raw JSONL stream.
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

test('participantFromPreset can rename while keeping the backend', () => {
  const renamed = participantFromPreset('zeus', { name: 'Deepreview' })
  assert.equal(renamed.id, 'deepreview')
  assert.equal(renamed.name, 'Deepreview')
  assert.equal(renamed.kind, 'claude-code')
  assert.equal(renamed.model, 'claude-opus-5')
  assert.equal(renamed.preset, 'zeus')
  // Without a rename, the canonical preset id and name are kept.
  const mani = participantFromPreset('mani')
  assert.equal(mani.id, 'mani')
  assert.equal(mani.name, 'Mani')
})

test('serializeTranscript preserves chronological (root->leaf) order with role labels and tool calls', () => {
  // getBranch() returns entries root -> leaf (oldest first); serialization keeps that order.
  const branch = [
    { type: 'message', id: '0', message: { role: 'user', content: 'first question' } },
    {
      type: 'message',
      id: '1',
      message: { role: 'assistant', content: [{ type: 'text', text: 'First reply' }] },
    },
    { type: 'message', id: '2', message: { role: 'user', content: 'second question' } },
    {
      type: 'message',
      id: '3',
      message: {
        role: 'assistant',
        content: [
          { type: 'text', text: 'Second reply' },
          { type: 'toolCall', name: 'read', arguments: { path: 'a.ts' } },
        ],
      },
    },
  ]
  const text = serializeTranscript(branch, { maxBytes: 10000 })
  assert.match(text, /User:\nfirst question/)
  assert.match(text, /Lead:\nFirst reply/)
  assert.match(text, /→ read\(/)
  assert.ok(text.indexOf('first question') < text.indexOf('second question'), 'chronological order')
})

test('serializeTranscript honors the latest compaction summary', () => {
  // root -> leaf order: the dropped message precedes the compaction, kept messages follow it.
  const branch = [
    { type: 'message', id: 'old', message: { role: 'user', content: 'ancient dropped question' } },
    { type: 'compaction', id: 'c1', summary: 'the earlier stuff', firstKeptEntryId: 'k1' },
    { type: 'message', id: 'k1', message: { role: 'user', content: 'kept question' } },
    {
      type: 'message',
      id: 'k2',
      message: { role: 'assistant', content: [{ type: 'text', text: 'after compaction reply' }] },
    },
  ]
  const text = serializeTranscript(branch)
  assert.match(text, /\[Earlier conversation summary\]\nthe earlier stuff/)
  assert.match(text, /kept question/)
  assert.doesNotMatch(text, /ancient dropped question/)
})

test('serializeTranscript caps bytes keeping the tail, and handles empty input', () => {
  const long = Array.from({ length: 50 }, (_, i) => ({
    type: 'message',
    id: String(i),
    message: { role: 'user', content: 'x'.repeat(500) },
  }))
  const text = serializeTranscript(long, { maxBytes: 2000 })
  assert.ok(Buffer.byteLength(text, 'utf8') <= 2000)
  assert.match(text, /\[earlier handoff truncated\]/)
  assert.equal(serializeTranscript([]), '')
  assert.equal(serializeTranscript(null), '')
})

test('serializeTranscript honors the CONSENSFLOW_HANDOFF_MAX_BYTES env budget when no maxBytes is passed', () => {
  // ~25 KB of entries: under the 48 KB default (no truncation), so the marker proves the env took effect.
  const long = Array.from({ length: 50 }, (_, i) => ({
    type: 'message',
    id: String(i),
    message: { role: 'user', content: 'x'.repeat(500) },
  }))
  const prev = process.env.CONSENSFLOW_HANDOFF_MAX_BYTES
  try {
    process.env.CONSENSFLOW_HANDOFF_MAX_BYTES = '2000'
    const text = serializeTranscript(long) // no options -> env budget applies
    assert.ok(Buffer.byteLength(text, 'utf8') <= 2000)
    assert.match(text, /\[earlier handoff truncated\]/)
  } finally {
    if (prev === undefined) delete process.env.CONSENSFLOW_HANDOFF_MAX_BYTES
    else process.env.CONSENSFLOW_HANDOFF_MAX_BYTES = prev
  }
})

test('serializeTranscript surfaces prior ConsensFlow participant exchanges (cross-pollination)', () => {
  const branch = [
    { type: 'message', id: '0', message: { role: 'user', content: "let's design the cache" } },
    {
      type: 'custom_message',
      id: '1-stream',
      customType: 'consensflow',
      content: '→ read({"path":"cache.ts"})',
      details: { streamEvent: true, participant: { id: 'iris' } },
    },
    {
      type: 'custom_message',
      id: '1',
      customType: 'consensflow',
      content: '# @iris\n\nRun: ask-123\nExit: 0\n\nUse a write-through cache.',
      details: {
        participant: { id: 'iris' },
        prompt: 'which cache strategy?',
        output: 'Use a write-through cache.',
      },
    },
  ]
  const text = serializeTranscript(branch)
  assert.match(text, /User → @iris: which cache strategy\?/)
  assert.match(text, /@iris replied:\nUse a write-through cache\./)
  // The run-metadata noise from the rendered message is not used when structured details exist;
  // live stream crumbs are display-only and do not cross-pollinate later participant handoffs.
  assert.doesNotMatch(text, /Run: ask-123/)
  assert.doesNotMatch(text, /cache\.ts/)
})

test('serializeTranscript keeps cf_run_participant tool results near-whole (lead-initiated cross-pollination)', () => {
  const reply = 'A'.repeat(5000)
  const branch = [
    {
      type: 'message',
      id: '1',
      message: {
        role: 'toolResult',
        toolName: 'cf_run_participant',
        content: [{ type: 'text', text: reply }],
      },
    },
    {
      type: 'message',
      id: '2',
      message: { role: 'toolResult', toolName: 'read', content: [{ type: 'text', text: reply }] },
    },
  ]
  const text = serializeTranscript(branch)
  // The consultation survives beyond the generic cap; ordinary tool results stay tightly truncated.
  assert.ok(text.includes(reply))
  assert.match(text, /Tool result read:\nA{1500}…\[truncated\]/)
})

test('parseParticipantPrompt routes one mention anywhere, and never hijacks stray @tokens', () => {
  const known = new Set(['zeus', 'athena'])
  // Leading and trailing single mention are equivalent (the headline fix).
  assert.deepEqual(parseParticipantPrompt(['@zeus', 'hi'], known), {
    participant: 'zeus',
    prompt: 'hi',
  })
  assert.deepEqual(parseParticipantPrompt(['hi', '@zeus'], known), {
    participant: 'zeus',
    prompt: 'hi',
  })
  assert.deepEqual(parseParticipantPrompt(['summarize', '@zeus', 'please'], known), {
    participant: 'zeus',
    prompt: 'summarize please',
  })
  // "ask"/"to" verb prefix still addresses a leading participant.
  assert.deepEqual(parseParticipantPrompt(['ask', '@athena', 'review'], known), {
    participant: 'athena',
    prompt: 'review',
  })
  // A leading mention wins and later @names stay as quoted text (paste-prior-output intact).
  assert.deepEqual(parseParticipantPrompt(['@athena', 'agree', 'with', '@zeus?'], known), {
    participant: 'athena',
    prompt: 'agree with @zeus?',
  })
  // Multiple leading mentions are rejected.
  assert.ok(parseParticipantPrompt(['@zeus', '@athena', 'hi'], known)?.error)
  // A stray non-leading @token that is not a participant goes to the lead, not a subprocess.
  assert.equal(parseParticipantPrompt(['install', '@types/node', 'now'], known), null)
  // Two different participants, none leading -> ambiguous, lead handles.
  assert.equal(parseParticipantPrompt(['compare', '@zeus', 'and', '@athena'], known), null)
  // No mention at all.
  assert.equal(parseParticipantPrompt(['just', 'fix', 'the', 'bug'], known), null)
  // Leading mention without a prompt errors helpfully.
  assert.ok(parseParticipantPrompt(['@zeus'], known)?.error)
  // Without a known-set, a non-leading mention does not route (conservative default).
  assert.equal(parseParticipantPrompt(['hi', '@zeus']), null)
})

test('image helpers: JWT account id, request body, SSE extraction, PNG save', async () => {
  // decodeChatGptAccountId pulls the claim out of the openai-codex JWT.
  const payload = Buffer.from(
    JSON.stringify({ 'https://api.openai.com/auth': { chatgpt_account_id: 'acc_42' } }),
  ).toString('base64url')
  assert.equal(decodeChatGptAccountId(`h.${payload}.sig`), 'acc_42')
  assert.throws(() => decodeChatGptAccountId('not-a-jwt'), /JWT/)

  // buildImageRequestBody triggers exactly one image_generation call with the prompt.
  const body = buildImageRequestBody('a red cat', 'gpt-5.5')
  assert.equal(body.model, 'gpt-5.5')
  assert.equal(body.tools[0].type, 'image_generation')
  assert.equal(body.input[0].content[0].text, 'a red cat')
  // Without references the content is text-only — regression guard.
  assert.equal(body.input[0].content.length, 1)
  assert.equal(
    body.input[0].content.some((part) => part.type === 'input_image'),
    false,
  )

  // With reference images, each becomes an input_image part (image_url is a plain string), after the text.
  const refs = ['data:image/png;base64,AAA', 'data:image/jpeg;base64,BBB']
  const refBody = buildImageRequestBody('blend these', 'gpt-5.5', refs)
  assert.equal(refBody.input[0].content[0].type, 'input_text')
  const imageParts = refBody.input[0].content.filter((part) => part.type === 'input_image')
  assert.deepEqual(
    imageParts.map((part) => part.image_url),
    refs,
  )
  assert.match(refBody.instructions, /reference image/i)

  // extractImageFromEvents finds the base64 image + metadata across SSE events.
  const img = extractImageFromEvents([
    { type: 'response.created', response: { id: 'r1' } },
    {
      type: 'response.output_item.done',
      item: {
        type: 'image_generation_call',
        id: 'i1',
        status: 'completed',
        result: 'QkFTRTY0',
        revised_prompt: 'a bright red cat',
      },
    },
    { type: 'response.completed', response: { id: 'r1' } },
  ])
  assert.equal(img.base64, 'QkFTRTY0')
  assert.equal(img.revisedPrompt, 'a bright red cat')
  assert.equal(img.responseId, 'r1')
  assert.throws(() => extractImageFromEvents([{ type: 'error', message: 'boom' }]), /boom/)

  // saveImagePng decodes base64 and writes the file.
  await withTempDir(async (cwd) => {
    const b64 = Buffer.from('PNGDATA').toString('base64')
    const saved = await saveImagePng(b64, path.join(cwd, 'runs', 'img1'), 'image.png')
    assert.equal((await readFile(saved)).toString(), 'PNGDATA')
  })
})

test('reference images: imageFileToDataUrl encodes by extension; rejects unknown/missing', async () => {
  await withTempDir(async (dir) => {
    const pngPath = path.join(dir, 'ref.png')
    await writeFile(pngPath, Buffer.from('PNGBYTES'))
    assert.equal(
      await imageFileToDataUrl(pngPath),
      `data:image/png;base64,${Buffer.from('PNGBYTES').toString('base64')}`,
    )

    const jpgPath = path.join(dir, 'ref.JPG')
    await writeFile(jpgPath, Buffer.from('JPGBYTES'))
    assert.match(await imageFileToDataUrl(jpgPath), /^data:image\/jpeg;base64,/)

    await writeFile(path.join(dir, 'ref.txt'), 'nope')
    await assert.rejects(
      () => imageFileToDataUrl(path.join(dir, 'ref.txt')),
      /Unsupported reference image/,
    )
    await assert.rejects(
      () => imageFileToDataUrl(path.join(dir, 'missing.png')),
      /ENOENT|no such file/,
    )
  })
})

test('parseParticipantPrompt: ask/to verb prefixes and the ask-noise boundary', () => {
  const known = new Set(['zeus', 'athena'])
  // The "to" verb addresses a leading participant.
  assert.deepEqual(parseParticipantPrompt(['to', '@zeus', 'hi'], known), {
    participant: 'zeus',
    prompt: 'hi',
  })
  // A leading address routes even when the name is not in the known set (explicit address wins).
  assert.deepEqual(parseParticipantPrompt(['ask', '@ghost', 'hi'], known), {
    participant: 'ghost',
    prompt: 'hi',
  })
  // "ask <english-verb> @types/..." is not an address: the @token is unknown -> stays with the lead.
  assert.equal(parseParticipantPrompt(['ask', 'fix', '@types/node', 'please'], known), null)
})

test('the cf_run_participant consent gate and name-neutrality stay locked in the extension source', async () => {
  const src = await readFile(new URL('../../hosts/pi/index.ts', import.meta.url), 'utf8')
  // The consent gate must stay in the tool description + promptSnippet (its load-bearing home).
  assert.match(src, /do NOT apply/)
  assert.match(src, /without first showing the user/)
  assert.match(src, /no user permission needed/)
  // The personal name must not reappear anywhere in the extension.
  assert.doesNotMatch(src, /Gabriel/)
  // No hidden-workflow commands should be registered (match the command registration form only).
  assert.doesNotMatch(src, /registerCommand\("(grill|spec-review|council|handoff)"/)
  // Pi intentionally matches Claude Code's slash surface: only /consensflow:* commands, no
  // legacy /cf, /participants, or per-participant /<name> command registration.
  assert.doesNotMatch(src, /name:\s*"cf"/)
  assert.doesNotMatch(src, /name:\s*"participants"/)
  assert.doesNotMatch(src, /registerParticipantCommands/)
  assert.match(src, /name:\s*"consensflow:cf"/)
})

test('every lib symbol the extension imports is actually exported (boundary smoke)', async () => {
  const src = await readFile(new URL('../../hosts/pi/index.ts', import.meta.url), 'utf8')
  const importRe = /import\s+(?:type\s+)?\{([^}]+)\}\s+from\s+"(\.\.\/lib\/[^"]+)"/g
  let checked = 0
  for (const match of src.matchAll(importRe)) {
    const symbols = match[1]
      .split(',')
      .map((s) =>
        s
          .trim()
          .split(/\s+as\s+/)[0]
          .trim(),
      )
      .filter(Boolean)
    const loaded = await import(new URL(`../../hosts/pi/${match[2]}`, import.meta.url).href)
    for (const symbol of symbols) {
      assert.notEqual(loaded[symbol], undefined, `${match[2]} must export ${symbol}`)
      checked += 1
    }
  }
  assert.ok(checked >= 10, `expected to verify several lib imports, got ${checked}`)
})

test('image path safety: runner backstop throws and extractImageFromEvents handles failure/empty', () => {
  // The runner must never spawn a CLI for an image participant.
  assert.throws(
    () => buildRunnerInvocation({ kind: 'image', model: 'gpt-5.5' }, '/tmp/packet.md', '/tmp'),
    /image participants are generated/,
  )
  // response.failed surfaces the error message.
  assert.throws(
    () =>
      extractImageFromEvents([
        { type: 'response.failed', response: { error: { message: 'content blocked' } } },
      ]),
    /content blocked/,
  )
  // A text-only / empty stream yields no image rather than a bogus base64.
  assert.equal(
    extractImageFromEvents([{ type: 'response.output_text.delta', delta: 'hi' }]).base64,
    undefined,
  )
  assert.equal(extractImageFromEvents([]).base64, undefined)
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
// (Opus 4.8 -> Opus 5) does not reach participants added under the old one. `participants sync`
// re-resolves them; this pins down exactly what it may and may not touch.
test('participants sync re-resolves preset-backed entries and leaves everything else pinned', async () => {
  await withTempDir(async (dir) => {
    // Added under an older catalog: stale model + effort, plus a rename and a cwd to preserve.
    await upsertParticipant(dir, {
      name: 'Deepreview',
      id: 'deepreview',
      kind: 'claude-code',
      model: 'claude-opus-4-8',
      effort: 'medium',
      cwd: 'backend',
      preset: 'zeus',
      description: 'stale text from an older catalog',
    })
    // A hand-rolled participant: no preset, so sync must never rewrite it.
    await upsertParticipant(dir, {
      name: 'Builder',
      kind: 'codex',
      model: 'my-own-model',
      effort: 'high',
    })
    // Names a preset the catalog no longer carries — stays pinned to what it was created with.
    await upsertParticipant(dir, {
      name: 'Ghost',
      kind: 'codex',
      model: 'gpt-5.5',
      effort: 'xhigh',
      preset: 'retired-preset',
    })
    const current = await upsertParticipant(dir, participantFromPreset('nike'))

    const preview = await syncParticipantsWithPresets(dir, { dryRun: true })
    assert.equal(preview.synced.length, 1, 'only the drifted preset-backed entry is reported')
    assert.equal(preview.synced[0].id, 'deepreview')
    assert.deepEqual(preview.orphans, ['@ghost'])
    assert.equal(
      (await getParticipant(dir, '@deepreview')).model,
      'claude-opus-4-8',
      'dry run writes nothing',
    )

    const result = await syncParticipantsWithPresets(dir)
    assert.equal(result.synced.length, 1)
    const fields = result.synced[0].changes.map((change) => change.field).sort()
    assert.deepEqual(
      fields,
      ['effort', 'model'],
      'engine fields only — description is a user override, never synced',
    )

    const synced = await getParticipant(dir, '@deepreview')
    assert.equal(synced.model, getPreset('zeus').model, 'model tracks the catalog')
    assert.equal(synced.effort, getPreset('zeus').effort, 'so does the effort tier')
    assert.equal(synced.name, 'Deepreview', 'a rename survives')
    assert.equal(synced.id, 'deepreview')
    assert.equal(synced.cwd, 'backend', 'so does a per-participant cwd')
    assert.equal(synced.preset, 'zeus')
    // `add <preset> --description` is a documented override, so sync must leave the text alone —
    // and because the two hosts word a few descriptions differently while sharing one roster,
    // syncing it would also mean the drift nudge could never be cleared.
    assert.equal(
      synced.description,
      'stale text from an older catalog',
      'a user-authored description survives',
    )

    const custom = await getParticipant(dir, '@builder')
    assert.equal(custom.model, 'my-own-model', 'custom participants are never rewritten')
    assert.equal(custom.effort, 'high')
    const ghost = await getParticipant(dir, '@ghost')
    assert.equal(ghost.model, 'gpt-5.5', 'an orphaned preset stays pinned')
    assert.equal(
      (await getParticipant(dir, '@nike')).updatedAt,
      current.updatedAt,
      'up-to-date entries are left untouched',
    )

    const second = await syncParticipantsWithPresets(dir)
    assert.equal(second.synced.length, 0, 'sync is idempotent')
    assert.deepEqual(
      driftedParticipants(await loadParticipants(dir)),
      [],
      'and the roster reports no drift after it',
    )
  })
})

test("upsertParticipant rejects a name slug that collides with another participant's id", async () => {
  await withTempDir(async (dir) => {
    await upsertParticipant(dir, { name: 'Zeus', kind: 'claude-code', model: 'claude-opus-4-8' })
    await upsertParticipant(dir, { name: 'Athena', kind: 'codex', model: 'gpt-5.5' })
    // getParticipant resolves by id OR name slug, so a second participant whose NAME slugifies
    // to an existing id would shadow it — must be rejected, not silently saved.
    await assert.rejects(
      upsertParticipant(dir, { id: 'athena2', name: 'Zeus', kind: 'codex', model: 'gpt-5.5' }),
      /collides/,
    )
    // Updating the same participant in place stays allowed.
    await upsertParticipant(dir, {
      name: 'Zeus',
      kind: 'claude-code',
      model: 'claude-opus-4-8',
      effort: 'max',
    })
  })
})

// lib parity with the consensflow-cc sibling: these files are kept byte-identical by convention
// (see AGENTS.md). A divergence means a fix landed in one project and silently missed the other.
// Both hosts share ONE roster, so any field `sync` rewrites must be identical in both catalogs —
// otherwise each host sees the other's value as drift and the nudge can never be cleared. This is
// exactly how the pygmalion description (deliberately worded differently per host) broke sync
// before 1.9.0 shipped: guard it mechanically rather than by review.
test('parity: every preset-owned field matches the sibling catalog, so sync converges across hosts', async (t) => {
  let sibling
  try {
    sibling = await import(new URL('../../consensflow-cc/lib/presets.js', import.meta.url).href)
  } catch {
    t.skip('consensflow-cc sibling checkout not present')
    return
  }
  const theirs = new Map(sibling.PARTICIPANT_PRESETS.map((preset) => [preset.preset, preset]))
  assert.deepEqual(
    PARTICIPANT_PRESETS.map((p) => p.preset),
    sibling.PARTICIPANT_PRESETS.map((p) => p.preset),
    'both catalogs carry the same presets in the same order',
  )
  for (const ours of PARTICIPANT_PRESETS) {
    const other = theirs.get(ours.preset)
    for (const field of PRESET_OWNED_FIELDS) {
      assert.equal(
        ours[field],
        other[field],
        `${ours.preset}.${field} differs between hosts — sync would flip it back and forth forever`,
      )
    }
  }
})

test('parity: shared lib files stay identical with the consensflow-cc sibling', async (t) => {
  const siblingLib = new URL('../../consensflow-cc/lib/', import.meta.url)
  for (const file of ['utils.js', 'workflows.js', 'transcript-events.js']) {
    let sibling
    try {
      sibling = await readFile(new URL(file, siblingLib), 'utf8')
    } catch {
      t.skip('consensflow-cc sibling checkout not present')
      return
    }
    const ours = await readFile(new URL(`../hosts/lib/${file}`, import.meta.url), 'utf8')
    assert.equal(
      ours,
      sibling,
      `${file} diverged from consensflow-cc — change both or document the delta in both AGENTS.md files`,
    )
  }
})

test('docs describe the stream-first observability surface, transcript backstop, and conventions [STRM-21]', async () => {
  const readme = await readFile(new URL('../../README.md', import.meta.url), 'utf8')
  const agents = await readFile(new URL('../../AGENTS.md', import.meta.url), 'utf8')
  const docs = `${readme}\n${agents}`
  // Stream-first observability surface — pi streams normalized events via cf_run_participant's onUpdate.
  assert.match(docs, /onUpdate/, 'docs mention the pi onUpdate streaming surface')
  assert.match(docs, /foreground/i, 'docs note streaming is foreground-incremental')
  assert.match(docs, /non-optional/i, 'docs lock streaming as non-optional')
  // Durability backstop + the new parity-locked event module.
  assert.match(docs, /transcript\.md/, 'docs mention the transcript.md backstop')
  assert.match(docs, /transcript-events\.js/, 'docs mention the parity-locked event module')
  assert.match(docs, /shared/i, 'docs mention the shared cross-tool roster')
  // The runners.js mirrored-with-deltas convention.
})
