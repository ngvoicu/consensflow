import { spawn } from 'node:child_process'
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto'
import { fstatSync, readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { CATALOG, EFFORTS } from './catalog.js'
import { detectHarnesses } from './harnesses.js'
import { installSkill, skillsStatus, skillsSummary, uninstallSkills } from './install.js'
import {
  applyMode,
  currentMode,
  MODES,
  modeLabel,
  modeReport,
  resetEverything,
  resetPreview,
  syncCmuxSkills,
  turnOff,
} from './mode.js'
import {
  addAgent,
  agentDrift,
  configRoot,
  editAgent,
  HARNESSES,
  listAgents,
  migrateStateRoot,
  removeAgent,
  syncAgents,
} from './roster.js'
import { agentCommand, generateSkill } from './skill.js'
import {
  healOnOpen,
  refreshInstalledSkill,
  retireSkillFromNativeHosts,
  skillGaps,
  skillTargets,
  staleSkills,
} from './sync.js'
import {
  installTerminalCommand,
  removeTerminalCommand,
  terminalCommandStatus,
  terminalRuntime,
} from './terminal.js'

/**
 * The minimal roster editor: one ephemeral loopback HTTP server, one inline
 * page, a random bearer token. No daemon, no lock file — Ctrl-C ends it.
 * Every mutation persists to the roster and regenerates the installed skill,
 * exactly as the CLI verbs do.
 */

function tokenMatches(presented, token) {
  return timingSafeEqual(
    createHash('sha256').update(presented).digest(),
    createHash('sha256').update(token).digest(),
  )
}

const VERSION = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', 'package.json'), 'utf8'),
).version

/**
 * The three integrations, each described in the terms someone choosing
 * between them needs: what it gives you, whether it is the path this
 * machine runs, and whether anything of it is already installed.
 */
const INTEGRATIONS = {
  claude: {
    title: 'Claude Code',
    summary: 'Only Claude Code gets the skill. Nothing else on this machine can consult.',
  },
  pi: {
    title: 'pi',
    summary: 'Only pi gets the skill. Nothing else on this machine can consult.',
  },
  cmux: {
    title: 'cmux (pi, cc, codex, opencode, kimi)',
    summary:
      'Every coding harness gets the skill, and a consult opens the agent’s own window in its cmux pane.',
  },
}

/**
 * One row per mode. Every mode installs the same generated skill and differs
 * only in who gets it, so there is one thing to count and one way to say it.
 */
function integrations(env) {
  const mode = currentMode(env)
  const carried = skillsStatus(env).filter((file) => file.source === 'consensflow').length

  const detected = detectHarnesses(env)

  return MODES.map((id) => {
    // Who this mode would reach on THIS machine. A card offering `pi` on a
    // machine with no pi is worth saying out loud, not leaving to be discovered
    // after the switch.
    const reach = id === 'cmux' ? detected : detected.filter((harness) => harness.id === id)
    const active = mode === id

    return {
      id,
      title: INTEGRATIONS[id].title,
      summary: INTEGRATIONS[id].summary,
      active,
      files: active ? carried : 0,
      present: active && carried > 0,
      reach: reach.map((harness) => harness.id),
      detail: active
        ? carried > 0
          ? `${carried} harness${carried === 1 ? '' : 'es'} carry the skill`
          : 'no agents yet — the first one installs it'
        : reach.length === 0
          ? id === 'cmux'
            ? 'no coding harness found on PATH'
            : `${id} is not on PATH`
          : 'available',
    }
  })
}

/** Everything `cf doctor` and `cf skills status` would tell you, as data. */
function systemState(env) {
  const files = skillsStatus(env)
  const mode = currentMode(env)
  return {
    version: VERSION,
    mode: {
      current: mode,
      available: MODES,
      report: modeReport(mode ?? 'cmux', env),
      labels: Object.fromEntries(MODES.map((name) => [name, modeLabel(name)])),
    },
    integrations: integrations(env),
    terminal: terminalCommandStatus(env),
    harnesses: detectHarnesses(env).map((harness) => ({
      id: harness.id,
      native: harness.native === true,
      skillsDir: harness.skillsDir,
    })),
    agents: listAgents(env).length,
    // The two facts `cf doctor` prints that the panel used to omit. The runtime
    // is the load-bearing one: move whatever provided it and the wiring stops.
    home: configRoot(env),
    runtime: terminalRuntime(env),
    // What a reset would destroy, so the page's dialog and `cf reset`'s
    // refusal quote the same two numbers.
    reset: resetPreview(env),
    // In scope but carrying no skill — a silent refusal looks exactly like health.
    gaps: skillGaps(env),
    // Files, not skills: a skill is a directory. Report both, and whose —
    // plus how many carry an older ConsensFlow's text, which an app upgrade
    // creates and nothing else reports: the files are ours and unedited, so
    // every other count calls them healthy.
    skills: { owned: files.length, ...skillsSummary(env), stale: staleSkills(env).length },
    // What opening the app put right before this page was served. A write
    // nobody reported is the quiet this whole feature exists to end.
    opened,
  }
}

/**
 * Set once, by the editor the app opens — never by `startUiServer` on its own,
 * which is what tests and other callers use.
 */
let opened = null

/**
 * The install the page can trigger: the generated skill everywhere it
 * belongs, and cmux's own skills wherever the mode says they go. Named
 * operations only — there
 * is deliberately no endpoint that runs a command someone typed.
 */
function installFromUi(body, env) {
  const agents = listAgents(env)
  const report = []
  if (agents.length > 0) {
    report.push(
      ...installSkill(
        {
          relPath: 'consensflow/SKILL.md',
          content: generateSkill(agents, { mode: currentMode(env) }),
          source: 'consensflow',
        },
        env,
        { force: body.force === true, targets: skillTargets(env, { all: body.all === true }) },
      ),
    )
    if (body.all !== true) report.push(...retireSkillFromNativeHosts(env))
  }
  // The only cmux-skills work left is taking back what an older version
  // installed — local disk only, so it cannot fail on the network.
  report.push(...syncCmuxSkills(env, { force: body.force === true }).report)
  return { report, cmuxCommit: null, system: systemState(env) }
}

/** The line this agent becomes in the skill — shown verbatim in the UI. */
function withCommand(agent) {
  const command = agentCommand(agent)
  return command === undefined ? agent : { ...agent, command }
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    let body = ''
    request.on('data', (chunk) => {
      body += chunk
      if (body.length > 64 * 1024) reject(new Error('body too large'))
    })
    request.on('end', () => resolve(body))
    request.on('error', reject)
  })
}

/**
 * True when this descriptor is the parent's end of a pipe. Node's `'pipe'`
 * stdio is a socketpair on macOS and a FIFO elsewhere, so both count; a
 * terminal or /dev/null is neither.
 */
function isPipe(fd) {
  try {
    const stats = fstatSync(fd)
    return stats.isFIFO() || stats.isSocket()
  } catch {
    return false
  }
}

export async function startUiServer(env) {
  // The app is an entry point too: a machine from before the merge should not
  // have to run the CLI once to be tidied up.
  migrateStateRoot(env)

  const token = randomBytes(24).toString('hex')

  const server = createServer(async (request, reply) => {
    const url = new URL(request.url, 'http://127.0.0.1')
    const send = (status, body, type = 'application/json') => {
      reply.writeHead(status, { 'content-type': type })
      reply.end(typeof body === 'string' ? body : JSON.stringify(body))
    }

    const presented =
      (request.headers.authorization ?? '').replace(/^Bearer /, '') ||
      (url.searchParams.get('token') ?? '')
    if (presented.length === 0 || !tokenMatches(presented, token)) {
      return send(401, { error: 'unauthorized' })
    }

    try {
      if (request.method === 'GET' && url.pathname === '/') {
        return send(200, PAGE(token), 'text/html; charset=utf-8')
      }
      if (request.method === 'GET' && url.pathname === '/api/agents') {
        return send(200, {
          agents: listAgents(env).map(withCommand),
          drift: agentDrift(env),
          harnesss: HARNESSES,
          catalog: Object.fromEntries(
            Object.entries(CATALOG).map(([harness, entries]) => [
              harness,
              entries.map((entry) => withCommand({ ...entry, harness })),
            ]),
          ),
          efforts: EFFORTS,
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/agents/sync') {
        // A named operation, like every other one here: it re-resolves
        // catalog-backed agents and nothing else.
        const body = JSON.parse((await readBody(request)) || '{}')
        const applied = syncAgents(env, {
          ...(typeof body.name === 'string' ? { name: body.name } : {}),
        })
        if (applied.length > 0) refreshInstalledSkill(env)
        return send(200, { applied, agents: listAgents(env).map(withCommand) })
      }
      if (request.method === 'POST' && url.pathname === '/api/agents') {
        const added = addAgent(JSON.parse(await readBody(request)), env)
        refreshInstalledSkill(env)
        return send(201, { agent: added })
      }
      if (request.method === 'GET' && url.pathname === '/api/system') {
        return send(200, systemState(env))
      }
      if (request.method === 'POST' && url.pathname === '/api/mode') {
        const body = JSON.parse((await readBody(request)) || '{}')
        if (!MODES.includes(body.mode)) {
          return send(400, { error: `unknown mode; expected ${MODES.join(', ')}` })
        }
        const outcome = applyMode(body.mode, env, {})
        return send(200, { ...outcome, system: systemState(env) })
      }
      if (request.method === 'POST' && url.pathname === '/api/skills/install') {
        const body = JSON.parse((await readBody(request)) || '{}')
        return send(200, installFromUi(body, env))
      }
      if (request.method === 'POST' && url.pathname === '/api/terminal-command') {
        const body = JSON.parse((await readBody(request)) || '{}')
        const outcome =
          body.remove === true ? removeTerminalCommand(env) : installTerminalCommand(env)
        return send(200, { ...outcome, system: systemState(env) })
      }
      if (request.method === 'POST' && url.pathname === '/api/reset') {
        const body = JSON.parse((await readBody(request)) || '{}')
        if (body.confirm !== true) {
          return send(400, { error: 'confirm before resetting ConsensFlow' })
        }
        const outcome = resetEverything(env)
        const { agents, runs } = outcome.removed
        return send(200, {
          ...outcome,
          report: [
            `Reset — ${agents} agent${agents === 1 ? '' : 's'}, ${runs} run${runs === 1 ? '' : 's'} and every installed file are gone`,
          ],
          system: systemState(env),
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/off') {
        const body = JSON.parse((await readBody(request)) || '{}')
        if (body.confirm !== true) {
          return send(400, { error: 'confirm before turning ConsensFlow off' })
        }
        const outcome = turnOff(env, { force: body.force === true })
        return send(200, {
          ...outcome,
          report: ['ConsensFlow is off — nothing is installed'],
          system: systemState(env),
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/skills/uninstall') {
        const body = JSON.parse((await readBody(request)) || '{}')
        // A click that removes 300 files says so first; the flag is the say-so.
        if (body.confirm !== true) {
          return send(400, { error: 'confirm the removal before it runs' })
        }
        return send(200, { report: uninstallSkills(env, { force: body.force === true }) })
      }

      const named = /^\/api\/agents\/([a-z][a-z0-9-]*)$/.exec(url.pathname)
      if (named !== null && request.method === 'PATCH') {
        const edited = editAgent(named[1], JSON.parse(await readBody(request)), env)
        refreshInstalledSkill(env)
        return send(200, { agent: edited })
      }
      if (named !== null && request.method === 'DELETE') {
        removeAgent(named[1], env)
        refreshInstalledSkill(env)
        reply.writeHead(204)
        return reply.end()
      }
      return send(404, { error: 'not found' })
    } catch (cause) {
      return send(400, { error: cause instanceof Error ? cause.message : String(cause) })
    }
  })

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address()

  return {
    url: `http://127.0.0.1:${port}`,
    token,
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

/**
 * `cf ui`: start, say where it is, run until Ctrl-C.
 *
 * `json` prints one machine-readable handle line first, so a host program
 * (the desktop app) can start the editor and point a window at it instead of
 * scraping prose. `open: false` leaves the browser alone for the same reason.
 */
export async function serveUi(env, { onOut, json = false, open = true }) {
  // Opening the app IS the act: it does what its own buttons do, before the
  // page is served, so the first render already tells the truth.
  opened = healOnOpen(env)
  const server = await startUiServer(env)
  const url = `${server.url}/?token=${server.token}`

  if (json) {
    onOut(JSON.stringify({ url: `${server.url}/`, token: server.token }))
  } else {
    onOut(`roster editor: ${url}`)
    onOut('Ctrl-C to stop — nothing keeps running after it.')
  }
  if (open) spawn('open', [url], { stdio: 'ignore', detached: true }).unref()

  // A parent that holds a pipe to our stdin is telling us it wants to own
  // this editor's lifetime: when that pipe closes the parent is gone, and an
  // editor nobody can see must not keep serving. A stdin that is a terminal
  // or /dev/null says nothing of the sort, so it is left alone.
  if (isPipe(0)) {
    process.stdin.on('end', () => process.exit(0))
    process.stdin.on('close', () => process.exit(0))
    process.stdin.resume()
  }
  await new Promise(() => {})
}

const PAGE = (token) => `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ConsensFlow roster</title>
<style>
  /* Dark-first: this window lives beside a terminal. Brand marine palette;
     Archivo and IBM Plex Mono when installed locally, never fetched — a local
     tool must not wait on a font CDN. */
  :root {
    --ink: #0C1E23;
    --panel: #12262C;
    --line: #1E3A42;
    --foam: #E9F1EF;
    --muted: #8FA9AF;
    --seafoam: #63C7B2;
    /* Seafoam on foam is unreadable; light mode gets a deeper teal for text
       while keeping seafoam for fills and borders. */
    --accent-text: #63C7B2;
    --buoy: #FF6B5A;
    --ui: Archivo, "Helvetica Neue", system-ui, sans-serif;
    --mono: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  @media (prefers-color-scheme: light) {
    :root {
      --ink: #E9F1EF; --panel: #FFFFFF; --line: #C9DAD8; --foam: #0C1E23;
      --muted: #52717A; --accent-text: #16766A; --buoy: #C2402F;
    }
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; padding: 40px 24px 64px; background: var(--ink); color: var(--foam);
    font-family: var(--ui); font-size: 15px; line-height: 1.5;
    -webkit-font-smoothing: antialiased;
  }
  main { max-width: 760px; margin: 0 auto; }

  .mark { font-family: var(--mono); font-size: 12px; letter-spacing: .14em; text-transform: uppercase; color: var(--accent-text); }
  h1 { font-size: 26px; font-weight: 600; letter-spacing: -.015em; margin: 6px 0 4px; }
  .lede { color: var(--muted); font-size: 13.5px; margin: 0 0 32px; max-width: 52ch; }

  .eyebrow {
    font-family: var(--mono); font-size: 11px; letter-spacing: .16em; text-transform: uppercase;
    color: var(--muted); display: flex; align-items: center; gap: 12px; margin: 34px 0 12px;
  }
  .eyebrow::after { content: ""; flex: 1; height: 1px; background: var(--line); }
  /* The section head announces; the tool heads inside it only sort. */
  .eyebrow--section { color: var(--foam); font-size: 12px; margin-top: 46px; }
  .cmds { margin: 14px 0 0; }
  .cmds dt { margin-top: 14px; }
  .cmds dt code { color: var(--seafoam); font-size: 13px; }
  .cmds dd { margin: 4px 0 0; color: var(--muted); font-size: 13px; line-height: 1.5; }
  .cmds dd code { color: var(--seafoam); }
  .cmds dd strong { color: var(--foam); }
  .eyebrow--tool { margin: 22px 0 4px; }
  .eyebrow--tool::after { display: none; }

  /* An agent IS a command: the callsign names it, the line below is
     exactly what lands in the skill and exactly what an harness will run. */
  .member { border-top: 1px solid var(--line); padding: 14px 0; display: grid; gap: 8px; }
  /* Grid children default to min-width:auto, so a long command line would
     stretch the row and push the controls off the page instead of scrolling. */
  .member > * { min-width: 0; }
  .member:last-of-type { border-bottom: 1px solid var(--line); }
  .member__head { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .callsign { font-size: 17px; font-weight: 600; color: var(--accent-text); letter-spacing: -.01em; }
  .tag { font-family: var(--mono); font-size: 11px; color: var(--muted); }
  .member__head .spacer { flex: 1; }
  .member__desc { color: var(--muted); font-size: 13px; margin: 0; }
  .cmd-wrap { position: relative; }
  /* A long command scrolls rather than wrapping (it stays one readable line);
     the fade is the only hint that there is more to the right. */
  .cmd-wrap::after {
    content: ""; position: absolute; inset: 1px 1px 1px auto; width: 44px; border-radius: 0 4px 4px 0;
    background: linear-gradient(90deg, transparent, var(--panel)); pointer-events: none;
  }
  .cmd {
    font-family: var(--mono); font-size: 11.5px; line-height: 1.6; color: var(--muted);
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
    padding: 9px 11px; margin: 0; overflow-x: auto; white-space: pre; scrollbar-width: thin;
  }
  .cmd b { color: var(--foam); font-weight: 500; }

  button {
    font: inherit; font-size: 13px; color: var(--foam); background: transparent;
    border: 1px solid var(--line); border-radius: 4px; padding: 4px 12px; cursor: pointer;
    transition: border-color .12s ease, color .12s ease, background .12s ease;
  }
  button:hover { border-color: var(--seafoam); color: var(--accent-text); }
  button.danger:hover { border-color: var(--buoy); color: var(--buoy); }
  button[data-armed="true"] { border-color: var(--buoy); color: var(--buoy); font-weight: 600; }
  button.primary { background: var(--seafoam); border-color: var(--seafoam); color: #06171C; font-weight: 600; }
  button.primary:hover { filter: brightness(1.08); color: #06171C; }
  :focus-visible { outline: 2px solid var(--seafoam); outline-offset: 2px; }

  .offer { display: flex; align-items: baseline; gap: 12px; padding: 8px 0; border-top: 1px solid var(--line); }
  .offer:first-of-type { border-top: none; }
  .offer__name { font-family: var(--mono); font-size: 13px; color: var(--foam); min-width: 96px; }
  .offer__what { color: var(--muted); font-size: 13px; flex: 1; }
  .offer__model { font-family: var(--mono); font-size: 11px; color: var(--muted); opacity: .8; }
  .member__drift { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin: 6px 0 0; font-size: 13px; color: var(--muted); }
  .tag--moved { background: var(--buoy); color: var(--ink); }
  .lede--tight { margin: 0 0 8px; }
  #catalog-filter {
    width: 100%; box-sizing: border-box; margin-bottom: 10px; padding: 8px 10px;
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
    color: var(--ink); font: inherit; font-size: 13px;
  }
  #catalog-filter:focus-visible { outline: 2px solid var(--seafoam); outline-offset: 1px; }

  form { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-top: 14px; }
  input, select {
    font: inherit; font-size: 13px; padding: 7px 10px; color: var(--foam);
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
  }
  input::placeholder { color: var(--muted); }
  .full { grid-column: 1 / -1; }
  .alert { color: var(--buoy); font-size: 13px; margin: 0; }
  .integration {
    border: 1px solid var(--line); border-radius: 6px; padding: 12px 14px; margin-bottom: 10px;
    display: grid; gap: 6px; background: var(--panel);
  }
  .integration[data-active="true"] { border-color: var(--seafoam); }
  .integration__head { display: flex; align-items: baseline; gap: 10px; }
  .integration__title { font-weight: 600; font-size: 15px; }
  .integration__state {
    font-family: var(--mono); font-size: 11px; letter-spacing: .06em; text-transform: uppercase;
    color: var(--muted);
  }
  .integration[data-active="true"] .integration__state { color: var(--accent-text); }
  .integration__head .spacer { flex: 1; }
  .integration p { margin: 0; font-size: 13px; color: var(--muted); }
  .facts { display: grid; gap: 4px; margin: 0 0 14px; }
  .fact { display: flex; gap: 12px; font-size: 13px; }
  .fact dt { color: var(--muted); min-width: 104px; font-family: var(--mono); font-size: 11.5px; letter-spacing: .04em; text-transform: uppercase; padding-top: 2px; }
  .fact dd { margin: 0; }
  .host { font-family: var(--mono); font-size: 12.5px; }
  .host + .host { margin-top: 2px; }
  .host span { color: var(--muted); }
  .actions { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
  .check { font-size: 13px; color: var(--muted); display: flex; align-items: center; gap: 6px; }
  .note { font-size: 13px; color: var(--muted); min-height: 20px; margin: 10px 0 0; }
  .empty { color: var(--muted); font-size: 13.5px; border: 1px dashed var(--line); border-radius: 4px; padding: 18px; }
  @media (max-width: 620px) { form { grid-template-columns: 1fr; } .offer { flex-wrap: wrap; } }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; } }
</style>
</head>
<body>
<main>
  <p class="mark"><span>consensflow</span> <span id="version"></span></p>
  <h1>Your agents</h1>
  <p class="lede" id="lede">Each one is a real coding-harness CLI. Every change here rewrites the skill installed in your harnesses, so they can consult it by name.</p>

  <section id="roster"></section>

  <p class="eyebrow eyebrow--section">Ready-made</p>
  <p class="lede lede--tight">Pick a name and it is configured for you. <span id="catalog-count"></span></p>
  <input id="catalog-filter" type="search" placeholder="Filter by name, model or engine…" autocomplete="off">
  <div id="catalog"></div>

  <p class="eyebrow eyebrow--section">How this machine consults</p>
  <p class="lede" id="mode-lede"></p>
  <div id="integrations"></div>

  <p class="eyebrow eyebrow--section">Installed</p>
  <div id="system"></div>
  <div class="actions">
    <button id="update">Update skills</button>
    <button id="terminal"></button>
    <button id="off" class="danger">Turn ConsensFlow off</button>
    <button id="reset" class="danger" title="Removes your agents, every run artifact (packets, transcripts, generated images), and every file ConsensFlow installed — including skill files you edited. The ConsensFlow.app bundle stays. This cannot be undone.">Reset everything</button>
  </div>
  <p id="skills-note" class="note"></p>

  <p class="eyebrow eyebrow--section">Talking to an agent</p>
  <p class="lede">Your coding agent runs these for you — you just say "ask hyperion…".
  They are here so you can drive it yourself when you want to.</p>
  <dl class="cmds">
    <dt><code>cf run @name "&lt;task&gt;"</code></dt>
    <dd>One consult. In cmux mode, in a terminal, this IS the agent's own
        window: the pane becomes claude's, pi's or opencode's interface on that
        conversation, seeded with your task, and you just talk. codex streams
        its first answer, then the pane becomes its window too. Each coding
        session gets its own conversation — nobody inherits yours.</dd>
    <dt><code>cf run @name "&lt;task&gt;" --new</code></dt>
    <dd>Start a fresh conversation instead, and print its name.</dd>
    <dt><code>cf sessions</code></dt>
    <dd>The conversations alive in this folder — name, agent, how many turns.</dd>
    <dt><code>cf catchup &lt;name&gt;</code></dt>
    <dd>Everything said in one, read from the harness's own session — whoever
        said it, window or not. <code>--wait</code> sits out the next answer and
        prints only what is new; it is how your coding agent follows along.</dd>
    <dt><code>cf last &lt;name&gt;</code></dt>
    <dd>The last answer a streamed run left, and where its transcript is.</dd>
    <dt><code>cf attach &lt;name&gt;</code></dt>
    <dd>Reopen a conversation's window later, in any terminal — whole history
        in it. Your coding agent still follows with <code>cf catchup</code>.</dd>
  </dl>

  <p class="eyebrow eyebrow--section">Define your own</p>
  <form id="add">
    <input name="name" placeholder="callsign, lowercase" required>
    <select name="harness"></select>
    <input class="full" name="model" placeholder="model — anything this harness accepts" required>
    <input name="effort" list="effort-options" placeholder="effort (optional)">
    <datalist id="effort-options"></datalist>
    <button class="primary">Add agent</button>
    <p id="error" class="alert full"></p>
  </form>
</main>
<script>
const TOKEN = ${JSON.stringify(token)};
const headers = { authorization: 'Bearer ' + TOKEN, 'content-type': 'application/json' };
const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

/** The model and effort are what the reader is scanning for: mark them. */
function renderCommand(entry) {
  const pre = el('pre', 'cmd');
  let rest = entry.command;
  for (const value of [entry.model, entry.effort].filter(Boolean)) {
    const at = rest.indexOf(value);
    if (at === -1) continue;
    pre.append(rest.slice(0, at));
    pre.append(el('b', null, value));
    rest = rest.slice(at + value.length);
  }
  pre.append(rest);
  const wrap = el('div', 'cmd-wrap');
  wrap.append(pre);
  return wrap;
}

function renderRoster(data) {
  const host = document.querySelector('#roster');
  host.innerHTML = '';
  document.querySelector('#lede').textContent = data.agents.length === 0
    ? 'Each one is a real coding-harness CLI. Add the first and the skill installs itself into every harness you have.'
    : 'Each one is a real coding-harness CLI. Every change here rewrites the skill installed in your harnesses, so they can consult it by name.';

  if (data.agents.length === 0) {
    host.appendChild(el('p', 'empty', 'No agents yet. Take a ready-made one below, or define your own.'));
    return;
  }
  if ((data.drift ?? []).length > 1) {
    const all = el('div', 'member');
    const line = el('p', 'member__drift');
    line.append(el('span', 'tag tag--moved', data.drift.length + ' agents moved'));
    line.append(el('span', null, ' the catalog has newer models for them '));
    const update = el('button', null, 'Update all');
    update.onclick = () => post('/api/agents/sync', {}, 'Updating…');
    line.append(update);
    all.append(line);
    host.append(all);
  }
  for (const p of data.agents) {
    const card = el('div', 'member');
    const head = el('div', 'member__head');
    head.append(el('span', 'callsign', p.name));
    head.append(el('span', 'tag', p.harness + (p.effort ? ' · ' + p.effort : '')));
    head.append(el('span', 'spacer'));
    const edit = el('button', null, 'Edit');
    edit.onclick = () => openEditor(card, p);
    head.append(edit);
    const remove = el('button', 'danger', 'Remove');
    remove.onclick = async () => {
      await fetch('/api/agents/' + p.name, { method: 'DELETE', headers });
      load();
    };
    head.append(remove);
    card.append(head);
    const moved = (data.drift ?? []).find((d) => d.name === p.name);
    if (moved) {
      const note = el('p', 'member__drift');
      note.append(el('span', 'tag tag--moved', 'catalog moved'));
      note.append(el('span', null, ' ' + moved.changes
        .map((c) => c.field + ': ' + (c.from ?? '-') + ' → ' + (c.to ?? '-')).join(', ') + ' '));
      const update = el('button', null, 'Update');
      update.onclick = () => post('/api/agents/sync', { name: p.name }, 'Updating ' + p.name + '…');
      note.append(update);
      card.append(note);
    }
    if (p.description) card.append(el('p', 'member__desc', p.description));
    if (p.command) card.append(renderCommand(p));
    else card.append(el('p', 'member__desc', p.harness + ' agents are not run by this tool — it leaves them alone.'));
    host.append(card);
  }
}

/** Editing an agent is changing its model, effort or description. */
function openEditor(card, agent) {
  if (card.querySelector('form')) return;
  const form = el('form', 'form');
  const fields = [
    ['model', agent.model, 'model'],
    ['effort', agent.effort ?? '', 'effort (blank for none)'],
    ['description', agent.description ?? '', 'description'],
  ];
  for (const [name, value, placeholder] of fields) {
    const input = document.createElement('input');
    input.name = name;
    input.value = value;
    input.placeholder = placeholder;
    input.className = 'full';
    form.append(input);
  }
  const save = el('button', 'primary', 'Save');
  save.type = 'submit';
  const cancel = el('button', null, 'Cancel');
  cancel.type = 'button';
  cancel.onclick = () => form.remove();
  form.append(save, cancel);
  form.onsubmit = async (event) => {
    event.preventDefault();
    const entries = Object.fromEntries(new FormData(form).entries());
    const res = await fetch('/api/agents/' + agent.name, {
      method: 'PATCH',
      headers,
      body: JSON.stringify(entries),
    });
    if (res.ok) load();
    else document.querySelector('#skills-note').textContent = (await res.json()).error;
  };
  card.append(form);
}

function renderCatalog(data) {
  const taken = new Set(data.agents.map((p) => p.name));
  const host = document.querySelector('#catalog');
  const needle = (document.querySelector('#catalog-filter').value || '').trim().toLowerCase();
  const matches = (entry, harness) => needle.length === 0 ||
    [entry.name, entry.model, entry.description, entry.detail, harness]
      .filter(Boolean).join(' ').toLowerCase().includes(needle);
  host.innerHTML = '';
  let shown = 0, free = 0;
  for (const [harness, entries] of Object.entries(data.catalog)) {
    free += entries.filter((e) => !taken.has(e.name)).length;
    const available = entries.filter((e) => !taken.has(e.name) && matches(e, harness));
    if (available.length === 0) continue;
    shown += available.length;
    const group = el('section');
    group.append(el('p', 'eyebrow eyebrow--tool', harness + ' · ' + available.length));
    for (const entry of available) {
      const row = el('div', 'offer');
      row.append(el('span', 'offer__name', entry.name));
      const what = el('span', 'offer__what', entry.description);
      if (entry.detail) what.title = entry.detail;
      row.append(what);
      row.append(el('span', 'offer__model', entry.model + (entry.effort ? ' · ' + entry.effort : '')));
      const add = el('button', null, 'Add');
      add.onclick = async () => {
        await fetch('/api/agents', {
          method: 'POST', headers,
          body: JSON.stringify({
            name: entry.name, harness, model: entry.model,
            ...(entry.effort ? { effort: entry.effort } : {}),
            description: entry.description,
            preset: entry.preset,
          }),
        });
        load();
      };
      row.append(add);
      group.append(row);
    }
    host.append(group);
  }
  const count = document.querySelector('#catalog-count');
  count.textContent = needle.length > 0
    ? shown + ' of ' + free + ' shown'
    : free + ' available across every engine';
  if (shown === 0) host.append(el('p', 'note', needle.length > 0
    ? 'No ready-made agent matches that.'
    : 'Every ready-made agent is already on your roster.'));
}

function renderForm(data) {
  const harnessSelect = document.querySelector('select[name=harness]');
  if (harnessSelect.options.length === 0) {
    for (const r of data.harnesss) harnessSelect.add(new Option(r, r));
    harnessSelect.onchange = () => showEfforts(data.efforts, harnessSelect.value);
  }
  showEfforts(data.efforts, harnessSelect.value);
}

function showEfforts(efforts, harness) {
  const list = document.querySelector('#effort-options');
  list.innerHTML = '';
  for (const e of efforts[harness] ?? []) list.appendChild(new Option(e, e));
}

function renderMode(system) {
  document.querySelector('#mode-lede').textContent =
    system.mode.current === null
      ? 'Nothing is installed yet. Pick the one path this machine runs — switching later removes the previous one.'
      : system.mode.report.join(' · ');

  const host = document.querySelector('#integrations');
  host.innerHTML = '';
  for (const integration of system.integrations) {
    const card = el('div', 'integration');
    card.dataset.active = String(integration.active);

    const head = el('div', 'integration__head');
    head.append(el('span', 'integration__title', integration.title));
    head.append(el('span', 'integration__state', integration.active ? 'active' : integration.detail));
    head.append(el('span', 'spacer'));
    if (!integration.active) {
      const use = el('button', null, 'Use this');
      use.onclick = () =>
        post('/api/mode', { mode: integration.id }, 'Switching to ' + integration.title + '…');
      head.append(use);
    }
    card.append(head);
    card.append(el('p', null, integration.summary));
    if (integration.active && integration.detail !== 'not installed') {
      card.append(el('p', null, integration.detail));
    }
    host.append(card);
  }
}

/** What each harness has, given the path this machine runs. */
function harnessState(harness, mode) {
  if (mode === null) return 'nothing yet';
  if (mode === 'cmux') return 'consults, via the generated skill';
  if (harness.id === mode) return 'consults, via the generated skill';
  return 'nothing in ' + mode + ' mode';
}

function renderTerminal(system) {
  const btn = document.querySelector('#terminal');
  const installed = system.terminal.installed;
  // The command is not a convenience. The skill this app installs teaches
  // cf run @name, so a consult IS this command — it used to call itself
  // optional and offer removal as the only other move, beside a skill that
  // depends on it. What it offers now depends on where it points: nowhere,
  // at another ConsensFlow, or here.
  const elsewhere = installed && system.runtime !== null && system.runtime.mine === false;
  btn.textContent = !installed
    ? 'Install terminal command'
    : elsewhere ? 'Point the command at this app' : 'Remove terminal command';
  btn.title = !installed
    ? 'The skill teaches cf run @name, so every consult goes through this command.'
    : elsewhere
      ? system.terminal.path + ' runs ' + system.runtime.runtime
      : system.terminal.path + (system.terminal.onPath ? '' : ' (not on your PATH)');
  btn.onclick = () =>
    post('/api/terminal-command', { remove: installed && !elsewhere },
      installed && !elsewhere ? 'Removing…' : 'Installing…');
}

function renderSystem(system) {
  document.querySelector('#version').textContent = 'v' + system.version;
  const host = document.querySelector('#system');
  host.innerHTML = '';
  const list = el('dl', 'facts');

  const hosts = el('div');
  for (const harness of system.harnesses) {
    const line = el('div', 'host');
    line.append(harness.id + ' ');
    line.append(el('span', null, '— ' + harnessState(harness, system.mode.current)));
    hosts.append(line);
  }
  if (system.harnesses.length === 0) hosts.append(el('span', null, 'none on PATH'));

  const active = system.integrations.find((i) => i.active);
  const parts = [];
  if (active && active.files > 0) {
    parts.push(active.title + ': ' + active.detail);
  }
  const sk = system.skills;
  if (sk.files > 0) {
    parts.push(sk.perHarness + ' skills in each of ' + sk.harnesses + ' harnesses');
    parts.push(sk.files + ' files: ' + sk.ours + ' ours'
      + (sk.cmux > 0 ? ', ' + sk.cmux + ' from cmux@' + sk.cmuxCommit : ''));
  }
  if (sk.drifted) parts.push(sk.drifted + ' edited by you');
  if (sk.missing) parts.push(sk.missing + ' missing');
  const skills = parts.length > 0 ? parts.join(' · ') : 'nothing yet';

  const runtime = system.runtime === null
    ? null
    : !system.runtime.exists
      ? system.runtime.runtime + ' — MISSING, reinstall from the app'
      : system.runtime.mine === false
        ? system.runtime.runtime + ' — another ConsensFlow: the cf command runs that one, not this app'
        : system.runtime.runtime;

  const rows = [['Harnesses', hosts], ['Installed', skills]];
  if (runtime) rows.push(['Runtime', runtime]);
  if (system.gaps.length > 0) rows.push(['Missing', system.gaps.join(', ') + ' — in scope but carrying no skill']);
  // An upgrade brings a new skill; the installed files stay as they are until
  // something rewrites them. Say which button does that.
  if (sk.stale > 0) rows.push(['Out of date', sk.stale + ' file' + (sk.stale === 1 ? '' : 's') + " carry an older ConsensFlow's text — press “Update skills” below"]);
  // Opening the app repairs what it can. Say so: a silent write is worse than
  // no write, and the one thing it deliberately does NOT do belongs here too.
  const on = system.opened;
  if (on && on.mode !== null) {
    const did = [];
    if (on.command === 'installed') did.push('put the cf command on your PATH');
    if (on.command === 'repaired') did.push('pointed the cf command back at this app');
    if (on.skills > 0) did.push('brought ' + on.skills + ' skill' + (on.skills === 1 ? '' : 's') + ' up to date');
    if (on.command === 'elsewhere') did.push('left the cf command where it is — it runs another ConsensFlow');
    if (did.length > 0) rows.push(['On open', did.join(' · ')]);
  }
  rows.push(['Home', system.home]);

  for (const [label, value] of rows) {
    const dt = el('dt', null, label);
    const dd = el('dd');
    if (typeof value === 'string') dd.textContent = value;
    else dd.append(value);
    const row = el('div', 'fact');
    row.append(dt, dd);
    list.append(row);
  }
  host.append(list);
}

async function post(path, body, note) {
  const el2 = document.querySelector('#skills-note');
  el2.textContent = note;
  const res = await fetch(path, { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  if (!res.ok) { el2.textContent = data.error; return; }
  const counts = {};
  for (const row of data.changes ?? data.report ?? []) {
    if (row && row.action) counts[row.action] = (counts[row.action] ?? 0) + 1;
  }
  const summary = Object.entries(counts).map(([action, n]) => n + ' ' + action).join(' · ');
  // The report is a list of sentences from some endpoints and a list of change
  // ROWS from others. Joining rows printed [object Object] on every skills
  // update; count those instead, and answer whatever shape did come back.
  const sentences = Array.isArray(data.report) && data.report.every((r) => typeof r === 'string')
    ? data.report
    : null;
  if (Array.isArray(data.applied)) {
    el2.textContent = data.applied.length === 0
      ? 'already up to date'
      : data.applied.map((a) => a.name + ': ' + a.changes
          .map((c) => c.field + ' → ' + (c.to ?? '-')).join(', ')).join(' · ');
  } else if (sentences !== null && sentences.length > 0) {
    el2.textContent = sentences.join(' · ');
  } else if (summary.length > 0) {
    // All it did was confirm every file is already right — say that, do not
    // make the reader parse a count to discover nothing happened.
    const only = Object.keys(counts);
    el2.textContent = only.length === 1 && only[0] === 'unchanged'
      ? 'already up to date — ' + counts.unchanged + ' files checked'
      : summary;
  } else if (Array.isArray(data.removed)) {
    const n = data.removed.length;
    el2.textContent = n === 0 ? 'nothing to remove' : 'removed ' + n + ' file' + (n === 1 ? '' : 's');
  } else {
    el2.textContent = 'nothing to do';
  }
  load();
}

// Choosing an integration installs it; this only refreshes what is there.
document.querySelector('#catalog-filter').addEventListener('input', () => {
  if (LAST !== null) renderCatalog(LAST);
});

document.querySelector('#update').onclick = () =>
  post('/api/skills/install', {}, 'Updating…');
/**
 * A confirmation the host cannot swallow.
 *
 * window.confirm is a no-op in the app: it is a WKWebView and nothing on the
 * Rust side implements the JS dialog panels, so the call returns false without
 * showing anything and the button silently does nothing. Two deliberate clicks
 * work in every webview and in a plain browser — the first arms the button and
 * says what is about to happen, the second does it, and walking away disarms.
 */
function arming(button, resting, armedLabel, run) {
  let timer = null;
  const disarm = () => {
    clearTimeout(timer);
    timer = null;
    button.textContent = resting;
    button.dataset.armed = 'false';
  };
  disarm();
  button.onclick = () => {
    if (timer !== null) { disarm(); run(); return; }
    button.textContent = armedLabel();
    button.dataset.armed = 'true';
    timer = setTimeout(disarm, 6000);
  };
}

arming(
  document.querySelector('#off'),
  'Turn ConsensFlow off',
  () => 'Click again to turn off — agents are kept',
  () => post('/api/off', { confirm: true }, 'Turning off…'),
);

arming(
  document.querySelector('#reset'),
  'Reset everything',
  () => {
    const c = LAST_SYSTEM === null ? { agents: 0, runs: 0 } : LAST_SYSTEM.reset;
    const say = (n, noun) => n + ' ' + noun + (n === 1 ? '' : 's');
    return 'Click again to destroy ' + say(c.agents, 'agent') + ' and ' + say(c.runs, 'run');
  },
  () => post('/api/reset', { confirm: true }, 'Resetting…'),
);

// The last roster payload, so filtering the catalog re-renders without a fetch.
let LAST = null;
// The last system payload, for the counts the reset dialog has to name.
let LAST_SYSTEM = null;

async function load() {
  const [data, system] = await Promise.all([
    (await fetch('/api/agents', { headers })).json(),
    (await fetch('/api/system', { headers })).json(),
  ]);
  LAST = data;
  LAST_SYSTEM = system;
  renderRoster(data);
  renderCatalog(data);
  renderForm(data);
  renderSystem(system);
  renderMode(system);
  renderTerminal(system);
}

document.querySelector('#add').onsubmit = async (event) => {
  event.preventDefault();
  const form = new FormData(event.target);
  const body = Object.fromEntries([...form.entries()].filter(([, v]) => v !== ''));
  const res = await fetch('/api/agents', { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  document.querySelector('#error').textContent = res.ok ? '' : data.error;
  if (res.ok) { event.target.reset(); load(); }
};
load();
</script>
</body>
</html>
`
