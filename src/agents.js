import { accessSync, constants, existsSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, join } from 'node:path'

/**
 * Where each coding agent keeps its skills, and whether it is installed here.
 * Detection is "the CLI resolves on PATH" — the same test the generated
 * commands live or die by. Directories honour each agent's own override
 * variable, which is also what keeps tests off the real machine.
 */

/**
 * A host that ships its own ConsensFlow already has a richer path than the
 * generated skill — consensflow-cc packets the live Claude Code conversation,
 * consensflow-pi does the same inside pi. Installing our skill there too
 * would put two entries with the same name and the same trigger in front of
 * one agent, competing for its skills budget. So we detect them and stand
 * aside (`--all` overrides).
 */
const NATIVE = {
  claude: (env) => join(home(env), '.claude', 'plugins', 'cache', 'consensflow-cc'),
  pi: (env) => join(home(env), '.pi', 'agent', 'git', 'github.com', 'ngvoicu', 'consensflow-pi'),
}

const AGENTS = [
  {
    id: 'claude',
    command: 'claude',
    skillsDir: (env) => join(env.CLAUDE_CONFIG_DIR ?? join(home(env), '.claude'), 'skills'),
  },
  {
    id: 'codex',
    command: 'codex',
    skillsDir: (env) => join(env.CODEX_HOME ?? join(home(env), '.codex'), 'skills'),
  },
  {
    id: 'opencode',
    command: 'opencode',
    skillsDir: (env) =>
      join(env.XDG_CONFIG_HOME ?? join(home(env), '.config'), 'opencode', 'skills'),
  },
  {
    id: 'pi',
    command: 'pi',
    skillsDir: (env) => join(home(env), '.pi', 'agent', 'skills'),
  },
]

function home(env) {
  return env.HOME ?? homedir()
}

function resolvesOnPath(command, env) {
  for (const dir of (env.PATH ?? '').split(delimiter)) {
    if (dir.length === 0) continue
    const candidate = join(dir, command)
    try {
      if (!statSync(candidate).isFile()) continue
      accessSync(candidate, constants.X_OK)
      return true
    } catch {
      // Not here; keep looking.
    }
  }
  return false
}

export function detectAgents(env) {
  return AGENTS.filter((agent) => resolvesOnPath(agent.command, env)).map((agent) => ({
    id: agent.id,
    command: agent.command,
    skillsDir: agent.skillsDir(env),
    native: NATIVE[agent.id] !== undefined && existsSync(NATIVE[agent.id](env)),
  }))
}
