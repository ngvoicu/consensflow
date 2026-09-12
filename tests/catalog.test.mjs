import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { agentProfile, CATALOG, catalogEntry, EFFORTS } from '../src/catalog.js'
import { HARNESSES } from '../src/roster.js'

describe('every tool ships a list of ready-made agents', () => {
  it('covers every harness, each with a real list (image has one)', () => {
    assert.deepEqual(Object.keys(CATALOG).sort(), [...HARNESSES].sort())
    for (const [harness, entries] of Object.entries(CATALOG)) {
      const least = ['image', 'kimi'].includes(harness) ? 1 : 3
      assert.ok(entries.length >= least, `${harness} needs a real list`)
    }
  })

  it('gives every entry a name, a model and a description', () => {
    for (const entries of Object.values(CATALOG)) {
      for (const entry of entries) {
        assert.match(entry.name, /^[a-z][a-z0-9-]*$/)
        assert.ok(entry.model.length > 0)
        assert.ok(entry.description.length > 0)
      }
    }
  })

  it('never repeats a name across the whole catalog', () => {
    const names = Object.values(CATALOG).flatMap((entries) => entries.map((e) => e.name))
    assert.equal(new Set(names).size, names.length)
  })

  it('uses only efforts its harness actually accepts', () => {
    for (const [harness, entries] of Object.entries(CATALOG)) {
      for (const entry of entries) {
        if (entry.effort === undefined) continue
        assert.ok(
          EFFORTS[harness].includes(entry.effort),
          `${entry.name}: ${harness} has no effort ${entry.effort}`,
        )
      }
    }
  })

  // An entry with no effort draws a bare harness tag in the roster UI, so a
  // level nobody chose looks exactly like a level the catalog forgot. Where the
  // harness HAS levels, every entry names one — unless it is listed here, and
  // the file says why beside it.
  it('names an effort wherever its harness has one, or is a listed exception', () => {
    // The three models that take no effort parameter at all — see "Effort ceilings"
    // in hosts/lib/presets.js for how that was established.
    const blankOnPurpose = new Set([
      'metis', // MiniMax M3, on pi
      'mimir', // MiniMax M3, on opencode
      'triton', // Laguna S 2.1 free, on pi
      'aegir', // Laguna S 2.1 free, on opencode
      // Added 2026-09-06 with the OpenCode Go and Zen roads — same rule, new
      // route. MiniMax M3 takes a reasoning toggle and no levels on Go too, and
      // Zen's Nemotron 3 Ultra entry publishes no reasoning options at all,
      // where OpenRouter's does: that is why ymir names `high` and audhumla,
      // the same model on the other road, names nothing.
      'kairos', // MiniMax M3 on OpenCode Go, through pi
      'andvari', // MiniMax M3 on OpenCode Go, through opencode
      'audhumla', // Nemotron 3 Ultra free on OpenCode Zen
    ])
    for (const [harness, entries] of Object.entries(CATALOG)) {
      if ((EFFORTS[harness] ?? []).length === 0) continue
      for (const entry of entries) {
        assert.ok(
          entry.effort !== undefined || blankOnPurpose.has(entry.name),
          `${entry.name}: ${harness} has effort levels, so this one must name one`,
        )
      }
    }
  })

  // The zoo is the same models through OpenCode: a pair that shares a model
  // must share the level too, or one name means two different agents.
  it("keeps each opencode twin at its pi twin's effort", () => {
    const level = (entry) => entry.effort
    for (const entry of CATALOG.opencode) {
      const twins = CATALOG.pi.filter((p) => p.model === entry.model)
      const twin = twins.find((p) => level(p) === level(entry)) ?? twins[0]
      if (twin === undefined || level(entry) === undefined) continue
      assert.equal(
        level(entry),
        level(twin),
        `${entry.name} and ${twin.name} share ${entry.model} but not the effort`,
      )
    }
  })

  it('carries the models verified live on 2026-08-21, newest of each family', () => {
    const models = Object.values(CATALOG).flatMap((entries) => entries.map((e) => e.model))
    assert.ok(models.includes('openrouter/z-ai/glm-5.3'))
    assert.ok(models.includes('openrouter/qwen/qwen3.8-max'))
    assert.ok(models.includes('openrouter/moonshotai/kimi-k3'))
    // GPT 6 Astra, probed 2026-09-05: the id answers on codex where `gpt-6`,
    // `gpt-6-sol`, `gpt-6-pro` and `gpt-5.6-pro` are all refused, and its
    // ladder was walked level by level (minimal refused; low..ultra answer).
    assert.ok(models.includes('gpt-6-astra'))
    // Added 2026-08-24, each confirmed present in `pi --list-models` and
    // `opencode models` before it was written down — two free tiers and one
    // unbadged stealth model, on both open-model harnesses. The stealth one
    // ended its testing period on 2026-08-27 (404 naming its own model), so
    // nyx and nott moved to it under its real name: z-ai/glm-5.3-flash,
    // verified that day in OpenRouter's /api/v1/models and by a live one-shot
    // on each CLI — neither harness catalog lists it yet, both run it.
    // Added 2026-09-03, both probed live on each harness at the level the row
    // names. Muse Spark 1.3 needed two probes: the first answered 403 on BOTH
    // harnesses ("18+ age confirmation"), an account attestation no catalog
    // can show, and the rows were held back until a probe answered. See the
    // Gemini 3.8 / Muse Spark paragraph in presets.js.
    for (const model of [
      'openrouter/z-ai/glm-5.3-flash',
      'openrouter/nvidia/nemotron-3-ultra-550b-a55b:free',
      'openrouter/poolside/laguna-s-2.1:free',
      'openrouter/google/gemini-3.8-flash',
      'openrouter/meta/muse-spark-1.3',
    ]) {
      assert.ok(
        CATALOG.pi.some((e) => e.model === model),
        `pi is missing ${model}`,
      )
      assert.ok(
        CATALOG.opencode.some((e) => e.model === model),
        `opencode is missing ${model}`,
      )
    }
    // Superseded versions must not linger in a curated list — and a retired
    // endpoint is superseded twice over: stealth/ox-alpha answers 404 now.
    assert.ok(!models.some((m) => m.includes('glm-5.2')))
    assert.ok(!models.some((m) => m.includes('qwen3.7')))
    assert.ok(!models.some((m) => m.includes('ox-alpha')))
    assert.ok(!models.some((m) => m.includes('gemini-3.7')))
    assert.ok(!models.some((m) => m.includes('muse-spark-1.2')))
  })

  // Added 2026-09-06. Two new roads to models this catalog already carried, and
  // one new road for GPT 6 Astra. Every id below was probed on the CLI that
  // runs it, at the level its row names, before it was written down.
  it('reaches the OpenCode Go models on both harnesses, at one shared level', () => {
    // Go's ids are identical on pi and opencode — `opencode-go/<model>`, byte
    // for byte — so each of these must appear on BOTH, and the twin rule above
    // then holds them to the same level. That pairing is the whole reason a
    // name means one thing here.
    for (const model of [
      'opencode-go/deepseek-v4-flash',
      'opencode-go/deepseek-v4-pro',
      'opencode-go/grok-4.6',
      'opencode-go/qwen3.8-max',
      'opencode-go/minimax-m3',
      'opencode-go/glm-5.3',
      'opencode-go/glm-5.3-flash',
      'opencode-go/kimi-k3',
      'opencode-go/muse-spark-1.3-contributor',
      'opencode-go/gpt-5.6-luna',
    ]) {
      assert.ok(
        CATALOG.pi.some((e) => e.model === model),
        `pi is missing ${model}`,
      )
      assert.ok(
        CATALOG.opencode.some((e) => e.model === model),
        `opencode is missing ${model}`,
      )
    }
    // The OpenRouter twins were NOT retired: Go is the cheap road, OpenRouter
    // the fallback. A Go row that quietly replaced one would be a route change
    // wearing a name the roster already trusts.
    const models = Object.values(CATALOG).flatMap((entries) => entries.map((e) => e.model))
    assert.ok(models.includes('openrouter/x-ai/grok-4.6'))
    assert.ok(models.includes('openrouter/moonshotai/kimi-k3'))
  })

  it('carries OpenCode Zen only where the account can actually reach it', () => {
    // Zen lists 102 models on models.dev and offers 7 through the CLI, because
    // this account has no Zen credential. Only reachable ids belong here, and
    // both of these were probed. pi has no Zen provider at all, so these rows
    // are opencode-only by necessity, not by preference.
    const zen = CATALOG.opencode.filter((e) => e.model.startsWith('opencode/'))
    assert.deepEqual(zen.map((e) => e.model).sort(), [
      'opencode/muse-spark-1.3-contributor-free',
      'opencode/nemotron-3-ultra-free',
    ])
    assert.equal(
      CATALOG.pi.filter((e) => e.model.startsWith('opencode/')).length,
      0,
      'pi has no Zen provider — a Zen row there would never run',
    )
  })

  it('reaches GPT 6 Astra on all three engines that answer for it', () => {
    // codex through the ChatGPT login (astraeus/asteria), pi through its own
    // copy of that login, opencode through OpenRouter — three roads, three
    // model strings, so no twin rule couples them. `ultra` stays codex-only:
    // neither of the new roads publishes it.
    assert.ok(CATALOG.codex.some((e) => e.model === 'gpt-6-astra'))
    assert.ok(CATALOG.pi.some((e) => e.model === 'openai-codex/gpt-6-astra'))
    assert.ok(CATALOG.opencode.some((e) => e.model === 'openrouter/openai/gpt-6-astra'))
    for (const harness of ['pi', 'opencode']) {
      const efforts = CATALOG[harness]
        .filter((e) => e.model.includes('gpt-6-astra'))
        .map((e) => e.effort)
        .sort()
      assert.deepEqual(
        efforts,
        ['low', 'max', 'medium', 'xhigh'],
        `${harness}: four Astra tiers, and no ultra`,
      )
    }
  })

  it('is the payload presets and nothing else — one list, not two', async () => {
    // A second hand-written list is how `nike` came to mean GPT-5.6-luna in the
    // app and Gemini 3.7 Flash in the harness. The catalog is now derived, so
    // the two can no longer disagree.
    const { AGENT_PRESETS } = await import('../hosts/lib/presets.js')
    const byName = new Map(AGENT_PRESETS.map((preset) => [preset.preset, preset]))

    for (const [harness, entries] of Object.entries(CATALOG)) {
      for (const entry of entries) {
        const preset = byName.get(entry.name)
        assert.ok(preset !== undefined, `${entry.name} exists as a preset`)
        assert.equal(entry.model, preset.model, `${entry.name}: model matches the harness`)
        assert.equal(entry.effort, preset.effort ?? preset.thinking, `${entry.name}: effort`)
        assert.equal(entry.preset, entry.name, `${entry.name}: records its provenance`)
        assert.ok(HARNESSES.includes(harness))
      }
    }

    // Every preset the manager can actually create is offered; the image
    // preset is not, because the roster has no harness that launches it.
    const offered = Object.values(CATALOG).flat().length
    assert.equal(offered, AGENT_PRESETS.length, 'every preset is offered, image included')
  })

  it('finds an entry by name, whatever tool it belongs to', () => {
    const entry = catalogEntry('zeus')
    assert.equal(entry.harness, 'claude')
    assert.equal(entry.model, 'claude-opus-5')
    assert.equal(catalogEntry('nobody'), undefined)
  })

  it('records the effort levels each CLI accepts, as its own help states them', () => {
    assert.deepEqual(EFFORTS.claude, ['low', 'medium', 'high', 'xhigh', 'max'])
    assert.ok(EFFORTS.codex.includes('ultra'))
    assert.ok(EFFORTS.pi.includes('off'))
  })

  it('names no ultra preset — ultra stays a level the CLI takes, not a row the catalog ships', () => {
    // Sol stepped down from ultra to max by the user's decision (2026-09-06):
    // a deliberate seat below the proven ceiling, like the DeepSeek rows. The
    // effort-ceilings audit must not "fix" it back.
    const ultras = Object.values(CATALOG)
      .flat()
      .filter((entry) => entry.effort === 'ultra')
    assert.deepEqual(ultras, [])
    const hyperion = catalogEntry('hyperion')
    assert.equal(hyperion.effort, 'max')
    assert.equal(hyperion.description, 'Codex GPT 5.6 Sol MAX')
  })
})

it('Kimi is K3-only and names its supported effort instead of hiding it', () => {
  assert.deepEqual(
    CATALOG.kimi.map((entry) => entry.name),
    ['ilmarinen'],
  )
  const entry = catalogEntry('ilmarinen')
  assert.equal(entry.model, 'moonshot-ai/kimi-k3')
  assert.equal(entry.effort, 'max')
  assert.deepEqual(EFFORTS.kimi, ['low', 'high', 'max'])
  assert.deepEqual(entry.profile.categories, ['coding', 'reviewer'])
  for (const effort of ['low', 'xhigh', 'medium'])
    assert.deepEqual(agentProfile({ ...entry, effort }).categories, ['coding'])
  assert.deepEqual(agentProfile({ ...entry, effort: 'high' }).categories, ['coding', 'reviewer'])
  for (const name of ['seppo', 'ahti']) assert.equal(catalogEntry(name), undefined)
  assert.ok(
    Object.values(CATALOG)
      .flat()
      .every((entry) => !/kimi-k2\.7/.test(entry.model)),
  )
})

describe('catalog presentation follows actual model and effort', () => {
  it('gives every curated entry an explicit model, route and practical description', () => {
    for (const entries of Object.values(CATALOG)) {
      for (const entry of entries) {
        assert.ok(entry.profile?.modelKey, entry.name)
        assert.ok(entry.profile.modelLabel, entry.name)
        assert.ok(entry.profile.routeLabel, entry.name)
        assert.ok(entry.profile.goodFor.length > 15, entry.name)
        assert.ok(
          entry.profile.categories.includes(entry.name === 'pygmalion' ? 'images' : 'coding'),
        )
      }
    }
  })

  it('unifies reviewed provider aliases while keeping model snapshots distinct', () => {
    assert.equal(catalogEntry('astraeus').profile?.modelKey, 'gpt-6-astra')
    assert.equal(catalogEntry('phosphoros').profile?.modelKey, 'gpt-6-astra')
    assert.equal(catalogEntry('aurvandil').profile?.modelKey, 'gpt-6-astra')
    assert.equal(catalogEntry('logi').profile?.modelKey, catalogEntry('gefjon').profile?.modelKey)
    assert.notEqual(
      catalogEntry('freya').profile?.modelKey,
      catalogEntry('dvalin').profile?.modelKey,
    )
  })

  it('recommends roles only for supported actual model and effort combinations', async () => {
    const { agentProfile } = await import('../src/catalog.js')
    assert.equal(typeof agentProfile, 'function')
    const astra = { harness: 'codex', model: 'gpt-6-astra', effort: 'medium' }
    assert.deepEqual(agentProfile(astra).categories, ['coding', 'reviewer'])
    for (const effort of ['low', 'off', 'minimal', 'unknown', undefined]) {
      assert.deepEqual(agentProfile({ ...astra, effort }).categories, ['coding'])
    }
    assert.deepEqual(
      agentProfile({ ...astra, harness: 'pi', model: 'openai-codex/gpt-6-astra', effort: 'ultra' })
        .categories,
      ['coding'],
    )
    assert.deepEqual(
      agentProfile({ harness: 'claude', model: 'claude-opus-5', effort: 'medium' }).categories,
      ['coding', 'reviewer'],
    )
    assert.deepEqual(agentProfile({ ...astra, model: 'invented', preset: 'astraeus' }).categories, [
      'coding',
    ])
    assert.deepEqual(agentProfile({ ...astra, harness: 'unknown' }).categories, [])
    assert.deepEqual(agentProfile({ harness: 'image', model: 'legacy-image' }).categories, [
      'images',
    ])
  })
})

it('ships all compatible low/medium choices with stable identities and Pi OpenRouter routing', () => {
  const matrix = [
    ['codex', 'gpt-5.6-sol', 'hemera', 'phaethon'],
    ['pi', 'openai-codex/gpt-5.6-sol', 'leto', 'asterope'],
    ['opencode', 'openrouter/openai/gpt-5.6-sol', 'arvakr', 'alsvidr'],
    ['codex', 'gpt-6-astra', 'electra', 'maia'],
    ['pi', 'openai-codex/gpt-6-astra', 'alcyone', 'merope'],
    ['opencode', 'openrouter/openai/gpt-6-astra', 'dagr', 'skirnir'],
    ['claude', 'claude-fable-5-1', 'terpsichore', 'thalia'],
    ['pi', 'openrouter/anthropic/claude-fable-5.1', 'musaeus', 'erato'],
    ['opencode', 'openrouter/anthropic/claude-fable-5.1', 'suttung', 'kvasir'],
  ]
  for (const [harness, model, low, medium] of matrix) {
    for (const [name, effort] of [
      [low, 'low'],
      [medium, 'medium'],
    ]) {
      const entry = catalogEntry(name)
      assert.ok(entry, name)
      assert.equal(entry.harness, harness)
      assert.equal(entry.model, model)
      assert.equal(entry.effort, effort)
    }
  }
  assert.equal(Object.values(CATALOG).flat().length, 98)
  for (const name of ['orpheus', 'linus', 'erato', 'kronos', 'atlas']) {
    assert.match(catalogEntry(name).model, /^openrouter\/anthropic\//)
    assert.equal(catalogEntry(name).profile.routeLabel, 'OpenRouter · API')
  }
  assert.equal(
    CATALOG.claude.some((e) => e.model.includes('astra')),
    false,
  )
  assert.equal(
    CATALOG.codex.some((e) => e.model.includes('fable')),
    false,
  )
})

it('lead and PM need xhigh or higher; reviewer recommendations begin at medium', async () => {
  const { agentProfile } = await import('../src/catalog.js')
  const pairs = [
    ['codex', 'gpt-6-astra'],
    ['codex', 'gpt-5.6-sol'],
    ['claude', 'claude-fable-5-1'],
    ['claude', 'claude-opus-5'],
    ['pi', 'openrouter/anthropic/claude-opus-5'],
    ['opencode', 'openrouter/anthropic/claude-fable-5.1'],
  ]
  for (const [harness, model] of pairs) {
    for (const effort of ['low', 'medium', 'high', 'xhigh', 'max']) {
      const categories = agentProfile({ harness, model, effort }).categories
      const expected = ['coding']
      if (['xhigh', 'max'].includes(effort)) expected.push('lead', 'pm')
      if (effort !== 'low') expected.push('reviewer')
      assert.deepEqual(categories, expected, model + ' ' + effort)
    }
  }
  assert.deepEqual(
    agentProfile({ harness: 'codex', model: 'gpt-6-astra', effort: 'ultra' }).categories,
    ['coding', 'lead', 'pm', 'reviewer'],
  )
  assert.deepEqual(
    agentProfile({ harness: 'codex', model: 'gpt-5.6-luna', effort: 'medium' }).categories,
    ['coding', 'reviewer'],
  )
})

it('Gemini 3.1 Pro Preview is retired on every harness while Gemini 3.8 Flash remains', () => {
  assert.equal(catalogEntry('helios'), undefined)
  assert.equal(catalogEntry('heimdall'), undefined)
  assert.ok(
    !Object.values(CATALOG)
      .flat()
      .some((p) => p.model.includes('gemini-3.1-pro')),
  )
  assert.equal(catalogEntry('nike').model, 'openrouter/google/gemini-3.8-flash')
  assert.equal(catalogEntry('sif').model, 'openrouter/google/gemini-3.8-flash')
})

it('Muse Contributor variants share model identity while retaining route terms and execution IDs', () => {
  for (const name of ['eos', 'logi', 'urania', 'odrerir', 'gefjon']) {
    const p = catalogEntry(name)
    assert.equal(p.profile.modelKey, 'muse-spark-1.3')
    assert.equal(p.profile.modelLabel, 'Muse Spark 1.3')
    if (['urania', 'odrerir', 'gefjon'].includes(name)) {
      assert.match(p.model, /contributor/)
      assert.match(p.profile.routeLabel, /Contributor/)
      assert.equal(p.profile.routeNote, 'Prompts and replies may train Meta models.')
    } else assert.equal(p.profile.routeNote, undefined)
  }
  assert.equal(catalogEntry('gefjon').profile.routeLabel, 'OpenCode Zen · Contributor · Free')
})
