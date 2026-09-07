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

describe('one command builder serves the skill and the roster editor', () => {
  const md = generateSkill(ROSTER)

  it('gives the exact line the skill table will contain', () => {
    const command = agentCommand(ROSTER[0])
    assert.equal(command, `cf run @${ROSTER[0].name} "<task>"`)
    for (const agent of ROSTER) {
      assert.equal(agentCommand(agent), `cf run @${agent.name} "<task>"`)
    }
  })

  it('lists every agent name in the description, so mentions trigger it', () => {
    const description = md.slice(0, md.indexOf('\n---\n'))
    for (const p of ROSTER) assert.ok(description.includes(p.name), `${p.name} missing`)
  })

  it('tells the harness to consult on its own initiative, not only when asked', () => {
    // The roster is worth nothing if the lead only consults when told to.
    assert.match(md, /Reach for an advisor on your own/)
    assert.match(md, /do not need permission to consult/i)
    assert.match(md, /riskiest assumption/i)
    // And it says the one thing this shape cannot do: see the conversation.
    assert.match(md, /cannot see this conversation/i)
  })

  it('teaches the harness to self-heal when a name is missing from the table', () => {
    assert.match(md, /cf skills update/)
    assert.match(md, /missing from (the|this) table/i)
  })

  it('keeps the permission gate that two product generations kept', () => {
    // Asserted verbatim by engine suites outside this file (claude-e2e,
    // pi-core): keep the wording, not just the idea.
    assert.match(md, /Advice is free; acting is gated/)
    assert.match(md, /without the user's explicit approval/)
    assert.match(md, /never edit this file by hand/i)
  })

  it('refuses to generate from an empty roster rather than shipping a blank skill', () => {
    assert.throws(() => generateSkill([]), /empty roster/i)
  })
})

describe('the skill teaches one verb, with the flags that make a spawn', () => {
  const md = generateSkill([
    { name: 'zeus', harness: 'claude', model: 'claude-opus-5', effort: 'max' },
    { name: 'pygmalion', harness: 'image', model: 'gpt-image-2' },
  ])

  it('spawns every agent the same way, image agents included', () => {
    assert.match(md, /cf run @<name> "<task>"/)
    assert.match(md, /pygmalion/)
    assert.match(md, /gpt-image-2/)
    assert.doesNotMatch(md, /codex exec/)
    assert.doesNotMatch(md, /env -u ANTHROPIC_API_KEY/)
  })

  it('says the brief and the handoff are the lead to give', () => {
    assert.match(md, /--brief/)
    assert.match(md, /--handoff-file/)
    assert.match(md, /You are the one holding it/)
  })
})

describe('standalone: one shape, no modes', () => {
  const roster = [
    { name: 'athena', harness: 'pi', model: 'openrouter/qwen/qwen3.8-27b', effort: 'max' },
  ]

  it('produces one text irrespective of the old options.mode', () => {
    const base = generateSkill(roster)
    for (const mode of ['claude', 'pi', 'cmux', undefined]) {
      assert.equal(generateSkill(roster, { mode }), base, `mode ${mode} must not change the skill`)
    }
  })

  it('names no cmux command, pane opener, or host-mode variant', () => {
    const md = generateSkill(roster)
    assert.doesNotMatch(md, /cmux/i)
    assert.doesNotMatch(md, /new-pane/i)
    assert.doesNotMatch(md, /cf mint/i)
    assert.doesNotMatch(md, /one-shot/i)
    // Raw harness CLIs are never the lead's to run: the app owns the panes.
    assert.doesNotMatch(md, /claude -p/)
    assert.doesNotMatch(md, /opencode run/)
    assert.doesNotMatch(md, /pi --no-session/)
  })

  it('still names the roster', () => {
    assert.match(generateSkill(roster), /named AI agents — athena/)
  })
})

describe('standalone: the three acts — consult, follow up, read', () => {
  const md = generateSkill([
    { name: 'ares', harness: 'pi', model: 'openrouter/x-ai/grok-4.6', effort: 'high' },
  ])

  it('teaches the consult with the continuation rule and the app-minted name', () => {
    assert.match(md, /cf run @<name> "<task>"/)
    assert.match(md, /--new/)
    assert.match(md, /--session/)
    assert.match(md, /conversation: <name>/)
  })

  it('keeps the three acts and the continue default verbatim', () => {
    assert.match(md, /cf say/)
    assert.match(md, /cf results/)
    assert.match(md, /cf read <name>/)
    assert.match(md, /continue by default, unsure means continue/)
  })

  it('names no retired catchup verb', () => {
    assert.doesNotMatch(md, /catchup/)
  })
})

describe('standalone: delivery — whole answers, file reads, human policy', () => {
  const md = generateSkill([
    { name: 'ares', harness: 'pi', model: 'openrouter/x-ai/grok-4.6', effort: 'high' },
  ])

  it('reads a delivered answer WHOLE from the top', () => {
    assert.match(md, /WHOLE/)
    assert.match(md, /from the top/i)
  })

  it('runs every cf read part and reads the complete body before reporting', () => {
    assert.match(md, /cf read <id>/)
    assert.match(md, /cf read <id> --part 2/)
    assert.match(md, /every.*part|each.*part/i)
    assert.match(md, /before anything else|before reporting/i)
  })

  it('never teaches a conversation-scoped follow-up part', () => {
    assert.doesNotMatch(md, /cf read <name> --part/)
  })

  it('further parts use the immutable delivery id from the first part', () => {
    assert.match(md, /immutable delivery id/i)
    assert.match(md, /delivery id the first part printed/i)
  })

  it('says truncation is never the whole result', () => {
    assert.match(md, /truncat/i)
  })

  it('a result read covers that result, not the discussion around it', () => {
    assert.match(md, /omitted discussion|discussion.*around/i)
  })

  it('manual disables automatic delivery but keeps results reads available to the task', () => {
    assert.match(md, /no automatic delivery/i)
    assert.match(md, /whenever the authorized task needs them/i)
    assert.match(md, /cf results/)
    assert.match(md, /cf read/)
    assert.doesNotMatch(md, /human says when to read/)
  })

  it('never changes a policy the human set', () => {
    assert.match(md, /never change.*polic|polic.*never/i)
  })
})

describe('standalone: send and return, never wait', () => {
  const md = generateSkill([
    { name: 'ares', harness: 'pi', model: 'openrouter/x-ai/grok-4.6', effort: 'high' },
  ])

  it('reports what is running and takes the next message', () => {
    assert.match(md, /send and return/i)
    assert.match(md, /running/)
    assert.match(md, /next message/)
  })

  it('says auto delivers and manual leaves answers for the task', () => {
    assert.match(md, /auto/)
    assert.match(md, /manual/)
    assert.match(md, /no automatic delivery/i)
  })

  it('never names --wait and calls polling wrong', () => {
    assert.doesNotMatch(md, /--wait/)
    assert.match(md, /polling.*wrong/i)
  })
})

describe('standalone: the input line belongs to the human', () => {
  const md = generateSkill([
    { name: 'ares', harness: 'pi', model: 'openrouter/x-ai/grok-4.6', effort: 'high' },
  ])

  it('never assumes the native input line is empty', () => {
    assert.match(md, /never assume the input line is empty/i)
  })

  it('a manual cf read never touches terminal input', () => {
    assert.match(md, /never touches terminal input/i)
  })

  it('only a human-only Resume replies confirms the line is empty', () => {
    assert.match(md, /human-only Resume replies/i)
  })

  it('promises no automatic transcript-based clearing', () => {
    assert.match(md, /do not promise that it does/i)
    assert.doesNotMatch(md, /clears itself|automatically cleared/)
  })
})

describe('standalone: parallel workers are the product, not a violation', () => {
  const md = generateSkill([
    { name: 'ares', harness: 'pi', model: 'openrouter/x-ai/grok-4.6', effort: 'high' },
  ])

  it('does not gate independent consults behind one-answer-at-a-time', () => {
    assert.doesNotMatch(md, /One agent at a time/)
    assert.doesNotMatch(md, /Wait for one answer before asking another/)
  })

  it('does not park the lead until the user has weighed in', () => {
    assert.doesNotMatch(md, /until the user has weighed in/)
  })
})
