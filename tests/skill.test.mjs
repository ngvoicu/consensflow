import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { agentCommand, generateSkill } from '../src/skill.js'

const ROSTER = [
  {
    name: 'zeus',
    harness: 'claude',
    model: 'claude-opus-5',
    effort: 'max',
    description: 'Deepest reviewer.',
  },
  {
    name: 'hyperion',
    harness: 'codex',
    model: 'gpt-5.6-sol',
    effort: 'max',
  },
  {
    name: 'endymion',
    harness: 'pi',
    model: 'openrouter/moonshotai/kimi-k3',
    effort: 'xhigh',
  },
  {
    name: 'mani',
    harness: 'opencode',
    model: 'openrouter/moonshotai/kimi-k3',
  },
  {
    name: 'loki',
    harness: 'codex',
    model: 'gpt-5.6-luna',
    effort: 'xhigh',
  },
]

it('coordinators maintain scoped tasks without confusing progress with activity or receipts', () => {
  for (const role of ['lead', 'pm']) {
    const md = generateSkill([], role)
    for (const command of ['cf task list', 'cf task get', 'cf task add', 'cf task update'])
      assert.ok(md.includes(command), command)
    assert.match(md, /--revision/)
    assert.match(md, /--review-of/)
    assert.match(md, /--question/)
    assert.match(md, /idle.*acceptance/i)
    assert.match(md, /does not\s+create a receipt/i)
    assert.match(md, /own group/)
    assert.match(md, /answers.*task|get.*answers/)
  }
  assert.doesNotMatch(generateSkill([], 'advisor'), /cf task (?:add|update)/)
})

describe('the private lead skill', () => {
  const md = generateSkill(ROSTER)
  it('has a scoped role description and lists the configured workers', () => {
    assert.match(md, /name: consensflow-lead/)
    for (const agent of ROSTER) {
      assert.ok(md.includes(`| ${agent.name} |`))
      assert.equal(agentCommand(agent), `cf run @${agent.name} "<task>"`)
    }
  })
  it('works with an empty roster and uses explicit discovery', () => {
    assert.match(generateSkill([]), /cf agent list/)
    assert.doesNotMatch(
      md,
      /cf skills (?:install|update)|unsupported version|Resume replies|cf catchup/,
    )
  })
  it('distinguishes new work from continuing a conversation', () => {
    assert.match(md, /--new/)
    assert.match(md, /--session <conversation>/)
    assert.match(md, /cf say <conversation>/)
    assert.match(md, /--prompt-file/)
  })
  it('uses delivered answers and finishes all parts without a new read request', () => {
    assert.match(md, /No retrieval call or new user permission is needed/)
    assert.match(md, /Read every part of that one result/)
    assert.match(md, /immutable delivery ID/)
    assert.match(md, /No delivered answer and no explicit request to read/)
  })
  it('continues independent work and does not infer failure from counters', () => {
    assert.match(md, /continue independent authorized work/)
    assert.match(md, /do not end the overall task|Do not\s+poll or end the overall task/)
    assert.match(md, /`0 runs`/)
    assert.match(md, /do not prove failure/)
    assert.match(md, /execution is uncertain/)
  })
  it('keeps role and policy authority with the app and user', () => {
    assert.match(md, /You do not need to inspect/)
    assert.match(md, /Leave draft text and delivery settings to the user/)
    assert.match(md, /do not manufacture\ncredentials/)
    assert.doesNotMatch(md, /consensflow-pm/)
  })
})

describe('coordinator model selection', () => {
  const roster = [
    { name: 'author', harness: 'claude', model: 'claude-fable-5-1', effort: 'xhigh' },
    {
      name: 'same-model',
      harness: 'opencode',
      model: 'openrouter/anthropic/claude-fable-5.1',
      effort: 'xhigh',
    },
    {
      name: 'review',
      harness: 'codex',
      model: 'gpt-6-astra',
      effort: 'max',
      description: 'Check | contracts\ncarefully',
      profile: {
        benchmarks: {
          source: 'Artificial Analysis',
          modelKey: 'gpt-6-astra',
          effort: 'max',
          testedModel: 'GPT-6 Astra (max)',
          fetchedAt: '2026-09-10T10:00:00Z',
          indexVersion: 4,
          scores: {
            intelligence: 53.2,
            coding: 80.7,
            hallucinations: 0,
            context: 61,
            invalid: 999,
          },
        },
      },
    },
    { name: 'native', harness: 'devin', model: 'default' },
    { name: 'kimi-worker', harness: 'kimi', model: 'moonshot-ai/kimi-k3', effort: 'max' },
    { name: 'image-worker', harness: 'image', model: 'codex-image' },
    { name: 'unsupported', harness: 'unknown', model: 'unverified-model' },
  ]
  it('supplies execution identity, effort, capabilities, route and benchmark provenance', () => {
    const md = generateSkill(roster)
    assert.match(md, /\|[^\n]*Effort[^\n]*\|/)
    assert.match(md, /Complex debugging, architecture and detailed review/)
    assert.match(md, /OpenRouter · API/)
    assert.match(md, /claude-fable-5\.1/g)
    const author = md.split('\n').find((line) => line.startsWith('| author |'))
    const same = md.split('\n').find((line) => line.startsWith('| same-model |'))
    assert.ok(author.includes('claude-fable-5.1') && same.includes('claude-fable-5.1'))
    assert.match(md, /Intelligence 53\.2/)
    assert.match(md, /Coding 80\.7/)
    assert.match(md, /Hallucinations 0/)
    assert.match(md, /2026-09-10T10:00:00Z/)
    assert.match(md, /index v4/)
    assert.match(md, /Check \\\| contracts carefully/)
    assert.doesNotMatch(md, /999|unverified-model/)
    assert.match(md, /Devin configured model/)
  })
  it('gives PMs a saved advisor roster while excluding unsupported advisor routes', () => {
    const md = generateSkill(roster, 'pm')
    assert.match(md, /name: consensflow-pm/)
    assert.match(md, /Available advisors/)
    for (const name of ['author', 'same-model', 'review', 'native'])
      assert.ok(md.includes(`| ${name} |`))
    assert.doesNotMatch(md, /\| (kimi-worker|image-worker|unsupported) \|/)
    assert.match(md, /Only you write or revise specifications/)
    assert.doesNotMatch(md, /name: consensflow-lead/)
  })
  it('requires cross-model review of substantial owner, worker and advisor outputs', () => {
    for (const role of ['lead', 'pm']) {
      const md = generateSkill(roster, role)
      assert.match(md, /cf agent list --json/)
      assert.match(md, /cross-model review/i)
      assert.match(md, /your own work|yourself/)
      assert.match(md, role === 'pm' ? /advisor.*output/ : /worker.*output/)
      assert.match(md, /different model family/)
      assert.match(md, /same model.*(?:not|does not)\s+independent/i)
      assert.match(md, /unknown.*(?:identity|model)|(?:identity|model).*unknown/i)
      assert.match(md, /no suitable|no eligible/i)
      assert.match(md, /findings.*(?:resolve|reconcile)|(?:resolve|reconcile).*findings/i)
    }
  })
  it('omits scores for a different saved model or reasoning level and labels unspecified reasoning', () => {
    const scored = roster[2]
    const mismatched = (fields) =>
      generateSkill([
        { ...scored, profile: { benchmarks: { ...scored.profile.benchmarks, ...fields } } },
      ])
    assert.doesNotMatch(mismatched({ modelKey: 'claude-fable-5.1' }), /Intelligence 53\.2/)
    assert.doesNotMatch(mismatched({ effort: 'low' }), /Intelligence 53\.2/)
    assert.match(mismatched({ reasoningMatch: 'unspecified' }), /AA reasoning level not specified/)
    assert.match(generateSkill([]), /No saved workers/)
    assert.match(generateSkill([], 'pm'), /No saved advisors/)
  })
})

it('teaches tiers by policy names for both coordinators, before benchmark scores', () => {
  for (const role of ['lead', 'pm']) {
    const md = generateSkill(
      [{ name: 'specialist', harness: 'codex', model: 'gpt-6-astra', effort: 'max' }],
      role,
    )
    for (const label of ['Critical work', 'Complex work', 'Standard work', 'Light work'])
      assert.ok(md.includes(label), label)
    assert.match(md, /Work tier/)
    assert.match(md, /No coding/)
    assert.match(md, /important questions/)
    assert.match(md, /ordinary reviews.*lower tiers/i)
    assert.match(md, /--purpose/)
    assert.doesNotMatch(md, /astraeus|calliope|clio|asteria|zeus|thalia|maia|hyperion/i)
    assert.match(
      generateSkill(
        [
          {
            name: 'custom',
            harness: 'codex',
            model: 'gpt-6-astra',
            effort: 'max',
            workTier: 'standard',
          },
        ],
        role,
      ),
      /custom.*Standard work/,
    )
  }
})

it('both coordinators receive the same complete-result and retry policy', () => {
  const section = (role) => {
    const md = generateSkill([], role)
    const start = md.indexOf('## Handle results')
    assert.notEqual(start, -1, `${role} must have an explicit result policy`)
    const end = md.indexOf('\n## ', start + 1)
    return md.slice(start, end)
  }
  assert.equal(section('lead'), section('pm'))
})

it('all bundled roles use one generator and advisor context excludes coordinator powers and roster', () => {
  const md = generateSkill(ROSTER, 'advisor')
  assert.match(md, /name: consensflow-advisor/)
  assert.match(md, /Only the PM writes or revises specifications/)
  assert.doesNotMatch(md, /\| zeus \||cf run|cf lead send|Available workers|Available advisors/)
  assert.throws(() => generateSkill([], '../lead'), /No .*skill/)
})
