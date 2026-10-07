/**
 * The agents the live bench and the live receipt checks run on: one cheap
 * model per harness (brain: operations/test-models.md), each with a tier of
 * its own, so a task that names the tier lands on that harness.
 */

/** What each harness calls its question tool, for the worker's brief; none for Pi. */
export const QUESTION_TOOL = {
  claude: 'AskUserQuestion tool',
  opencode: 'question tool',
  codex: 'request_user_input tool',
  devin: 'ask_user_question tool',
}

// Each harness's worker keeps a tier of its own: the daemon picks a member
// by tier alone, and a step that needs one harness's question tool must land
// on that harness. Critical work names a purpose; the flag carries it.
export const AGENTS = {
  claude: {
    id: 'bench-claude',
    kind: 'claude-code',
    model: 'claude-sonnet-5',
    workTier: 'complex',
  },
  opencode: {
    id: 'bench-opencode',
    kind: 'opencode',
    model: 'opencode/muse-spark-1.3-contributor-free',
    workTier: 'light',
  },
  pi: {
    id: 'bench-pi',
    kind: 'pi',
    model: 'openrouter/meta/muse-spark-1.3',
    workTier: 'standard',
  },
  // SWE-1.6 Slow, the free plan's, is not on Devin Pro (2026-10-03); SWE-2 is free there.
  devin: {
    id: 'bench-devin',
    kind: 'devin',
    model: 'swe-2',
    effort: 'medium',
    workTier: 'critical',
  },
  codex: { id: 'bench-codex', kind: 'codex', model: 'gpt-5.6-luna', workTier: 'critical' },
}

// The bench measures delivery, not judgment: a chief left to guess its job
// explores for minutes after every result, and each delivery waits for that.
export const BRIEF =
  'This folder is an automated ConsensFlow bench. Do exactly what each message asks and ' +
  'nothing more. When a ConsensFlow message arrives, reply with one line that names it ' +
  'and run no tools unless the message tells you to run a command.\n'

/** The flag that names a member's tier on `cf task add`. */
export const tierFlag = (tier) =>
  tier === 'critical' ? '--tier critical --purpose hard-problem' : `--tier ${tier}`
