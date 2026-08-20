import { spawn } from 'node:child_process'
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto'
import { fstatSync, readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { detectAgents } from './agents.js'
import { CATALOG, EFFORTS } from './catalog.js'
import { installCmuxSkills } from './cmux-skills.js'
import { hostStatus } from './hosts.js'
import { installSkill, skillsStatus, uninstallSkills } from './install.js'
import { applyMode, currentMode, MODES, modeLabel, modeReport, turnOff } from './mode.js'
import {
  addParticipant,
  editParticipant,
  listParticipants,
  RUNTIMES,
  removeParticipant,
} from './roster.js'
import { generateSkill, participantCommand } from './skill.js'
import { refreshInstalledSkill, retireSkillFromNativeHosts, skillTargets } from './sync.js'
import { installTerminalCommand, removeTerminalCommand, terminalCommandStatus } from './terminal.js'

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
    summary: 'Claude Code consults, and the participant gets your live conversation as context.',
  },
  pi: {
    title: 'pi',
    summary: 'pi consults, and the participant gets your live pi session as context.',
  },
  cmux: {
    title: 'cmux (pi, cc, codex, opencode)',
    summary:
      'Every coding agent can consult, through the generated skill. No conversation is shared.',
  },
}

function integrations(env) {
  const mode = currentMode(env)
  const hosts = Object.fromEntries(hostStatus(env).map((host) => [host.id, host]))
  const owned = skillsStatus(env)

  return MODES.map((id) => {
    const host = hosts[id]
    const generated = owned.filter((file) => file.source === 'consensflow')
    return {
      id,
      title: INTEGRATIONS[id].title,
      summary: INTEGRATIONS[id].summary,
      active: mode === id,
      files:
        id === 'cmux' ? owned.filter((f) => f.source === 'consensflow').length : (host?.files ?? 0),
      present:
        id === 'cmux'
          ? generated.length > 0
          : (host?.installed ?? false) || (host?.present ?? false),
      detail:
        id === 'cmux'
          ? generated.length > 0
            ? `${generated.length} agents carry the skill`
            : 'not installed'
          : host?.installed === true
            ? `installed by ConsensFlow${host.version ? ` (v${host.version})` : ''}`
            : host?.present === true
              ? `already present via ${host.via}`
              : 'not installed',
    }
  })
}

/** Everything `cf doctor` and `cf skills status` would tell you, as data. */
function systemState(env) {
  const files = skillsStatus(env)
  const cmux = files.find((file) => file.source.startsWith('cmux@'))
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
    agents: detectAgents(env).map((agent) => ({
      id: agent.id,
      native: agent.native === true,
      skillsDir: agent.skillsDir,
    })),
    participants: listParticipants(env).length,
    skills: {
      owned: files.length,
      drifted: files.filter((file) => file.state === 'drifted').length,
      missing: files.filter((file) => file.state === 'missing').length,
      cmuxCommit: cmux === undefined ? null : cmux.source.slice('cmux@'.length),
    },
  }
}

/**
 * The install the page can trigger: the generated skill everywhere it
 * belongs, optionally cmux's own skills too. Named operations only — there
 * is deliberately no endpoint that runs a command someone typed.
 */
function installFromUi(body, env) {
  const participants = listParticipants(env)
  const report = []
  if (participants.length > 0) {
    report.push(
      ...installSkill(
        {
          relPath: 'consensflow/SKILL.md',
          content: generateSkill(participants),
          source: 'consensflow',
        },
        env,
        { force: body.force === true, targets: skillTargets(env, { all: body.all === true }) },
      ),
    )
    if (body.all !== true) report.push(...retireSkillFromNativeHosts(env))
  }
  // cmux's own skills are part of an install, not a checkbox.
  let cmuxCommit = null
  try {
    const cmux = installCmuxSkills(env, { force: body.force === true })
    cmuxCommit = cmux.commit
    report.push(...cmux.report)
  } catch {
    // Offline: the consensflow skill still installed, and skills update
    // will fetch cmux's later.
  }
  return { report, cmuxCommit, system: systemState(env) }
}

/** The line this participant becomes in the skill — shown verbatim in the UI. */
function withCommand(participant) {
  const command = participantCommand(participant)
  return command === undefined ? participant : { ...participant, command }
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
      if (request.method === 'GET' && url.pathname === '/api/participants') {
        return send(200, {
          participants: listParticipants(env).map(withCommand),
          runtimes: RUNTIMES,
          catalog: Object.fromEntries(
            Object.entries(CATALOG).map(([runtime, entries]) => [
              runtime,
              entries.map((entry) => withCommand({ ...entry, runtime })),
            ]),
          ),
          efforts: EFFORTS,
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/participants') {
        const added = addParticipant(JSON.parse(await readBody(request)), env)
        refreshInstalledSkill(env)
        return send(201, { participant: added })
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

      const named = /^\/api\/participants\/([a-z][a-z0-9-]*)$/.exec(url.pathname)
      if (named !== null && request.method === 'PATCH') {
        const edited = editParticipant(named[1], JSON.parse(await readBody(request)), env)
        refreshInstalledSkill(env)
        return send(200, { participant: edited })
      }
      if (named !== null && request.method === 'DELETE') {
        removeParticipant(named[1], env)
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
  .eyebrow--tool { margin: 22px 0 4px; }
  .eyebrow--tool::after { display: none; }

  /* A participant IS a command: the callsign names it, the line below is
     exactly what lands in the skill and exactly what an agent will run. */
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
  button.primary { background: var(--seafoam); border-color: var(--seafoam); color: #06171C; font-weight: 600; }
  button.primary:hover { filter: brightness(1.08); color: #06171C; }
  :focus-visible { outline: 2px solid var(--seafoam); outline-offset: 2px; }

  .offer { display: flex; align-items: baseline; gap: 12px; padding: 8px 0; border-top: 1px solid var(--line); }
  .offer:first-of-type { border-top: none; }
  .offer__name { font-family: var(--mono); font-size: 13px; color: var(--foam); min-width: 96px; }
  .offer__what { color: var(--muted); font-size: 13px; flex: 1; }

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
  <h1>Your participants</h1>
  <p class="lede" id="lede">Each one is a real coding-agent CLI. Every change here rewrites the skill installed in your agents, so they can consult it by name.</p>

  <section id="roster"></section>

  <p class="eyebrow eyebrow--section">Ready-made</p>
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
  </div>
  <p id="skills-note" class="note"></p>

  <p class="eyebrow eyebrow--section">Define your own</p>
  <form id="add">
    <input name="name" placeholder="callsign, lowercase" required>
    <select name="runtime"></select>
    <input class="full" name="model" placeholder="model — anything this runtime accepts" required>
    <input name="effort" list="effort-options" placeholder="effort (optional)">
    <datalist id="effort-options"></datalist>
    <button class="primary">Add participant</button>
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
  document.querySelector('#lede').textContent = data.participants.length === 0
    ? 'Each one is a real coding-agent CLI. Add the first and the skill installs itself into every agent you have.'
    : 'Each one is a real coding-agent CLI. Every change here rewrites the skill installed in your agents, so they can consult it by name.';

  if (data.participants.length === 0) {
    host.appendChild(el('p', 'empty', 'No participants yet. Take a ready-made one below, or define your own.'));
    return;
  }
  for (const p of data.participants) {
    const card = el('div', 'member');
    const head = el('div', 'member__head');
    head.append(el('span', 'callsign', p.name));
    head.append(el('span', 'tag', p.runtime + (p.effort ? ' · ' + p.effort : '')));
    head.append(el('span', 'spacer'));
    const edit = el('button', null, 'Edit');
    edit.onclick = () => openEditor(card, p);
    head.append(edit);
    const remove = el('button', 'danger', 'Remove');
    remove.onclick = async () => {
      await fetch('/api/participants/' + p.name, { method: 'DELETE', headers });
      load();
    };
    head.append(remove);
    card.append(head);
    if (p.description) card.append(el('p', 'member__desc', p.description));
    if (p.command) card.append(renderCommand(p));
    else card.append(el('p', 'member__desc', p.runtime + ' participants are not run by this tool — it leaves them alone.'));
    host.append(card);
  }
}

/** Editing a participant is changing its model, effort or description. */
function openEditor(card, participant) {
  if (card.querySelector('form')) return;
  const form = el('form', 'form');
  const fields = [
    ['model', participant.model, 'model'],
    ['effort', participant.effort ?? '', 'effort (blank for none)'],
    ['description', participant.description ?? '', 'description'],
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
    const res = await fetch('/api/participants/' + participant.name, {
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
  const taken = new Set(data.participants.map((p) => p.name));
  const host = document.querySelector('#catalog');
  host.innerHTML = '';
  for (const [runtime, entries] of Object.entries(data.catalog)) {
    const available = entries.filter((e) => !taken.has(e.name));
    if (available.length === 0) continue;
    const group = el('section');
    group.append(el('p', 'eyebrow eyebrow--tool', runtime));
    for (const entry of available) {
      const row = el('div', 'offer');
      row.append(el('span', 'offer__name', entry.name));
      row.append(el('span', 'offer__what', entry.description));
      const add = el('button', null, 'Add');
      add.onclick = async () => {
        await fetch('/api/participants', {
          method: 'POST', headers,
          body: JSON.stringify({
            name: entry.name, runtime, model: entry.model,
            ...(entry.effort ? { effort: entry.effort } : {}),
            description: entry.description,
          }),
        });
        load();
      };
      row.append(add);
      group.append(row);
    }
    host.append(group);
  }
}

function renderForm(data) {
  const runtimeSelect = document.querySelector('select[name=runtime]');
  if (runtimeSelect.options.length === 0) {
    for (const r of data.runtimes) runtimeSelect.add(new Option(r, r));
    runtimeSelect.onchange = () => showEfforts(data.efforts, runtimeSelect.value);
  }
  showEfforts(data.efforts, runtimeSelect.value);
}

function showEfforts(efforts, runtime) {
  const list = document.querySelector('#effort-options');
  list.innerHTML = '';
  for (const e of efforts[runtime] ?? []) list.appendChild(new Option(e, e));
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

/** What each agent has, given the path this machine runs. */
function agentState(agent, mode) {
  if (mode === null) return 'nothing yet';
  if (mode === 'cmux') return 'consults, via the generated skill';
  if (agent.id === mode) return 'consults, with your conversation as context';
  return 'nothing in ' + mode + ' mode';
}

function renderTerminal(system) {
  const btn = document.querySelector('#terminal');
  const installed = system.terminal.installed;
  btn.textContent = installed ? 'Remove terminal command' : 'Install terminal command (optional)';
  btn.title = installed
    ? system.terminal.path + (system.terminal.onPath ? '' : ' (not on your PATH)')
    : 'Optional: puts the consensflow command on your PATH for scripting. Nothing here needs it.';
  btn.onclick = () =>
    post('/api/terminal-command', { remove: installed }, installed ? 'Removing…' : 'Installing…');
}

function renderSystem(system) {
  document.querySelector('#version').textContent = 'v' + system.version;
  const host = document.querySelector('#system');
  host.innerHTML = '';
  const list = el('dl', 'facts');

  const hosts = el('div');
  for (const agent of system.agents) {
    const line = el('div', 'host');
    line.append(agent.id + ' ');
    line.append(el('span', null, '— ' + agentState(agent, system.mode.current)));
    hosts.append(line);
  }
  if (system.agents.length === 0) hosts.append(el('span', null, 'none on PATH'));

  const active = system.integrations.find((i) => i.active);
  const parts = [];
  if (active && active.files > 0) {
    parts.push(active.id === 'cmux'
      ? active.files + ' agents carry the skill'
      : active.title + ': ' + active.files + ' files wired + payload');
  }
  if (system.skills.owned > 0) parts.push(system.skills.owned + ' skill files');
  if (system.skills.cmuxCommit) parts.push('cmux@' + system.skills.cmuxCommit);
  if (system.skills.drifted) parts.push(system.skills.drifted + ' edited by you');
  if (system.skills.missing) parts.push(system.skills.missing + ' missing');
  const skills = parts.length > 0 ? parts.join(' · ') : 'nothing yet';

  for (const [label, value] of [['Agents', hosts], ['Installed', skills]]) {
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
  el2.textContent = data.report ? data.report.join(' · ') : (summary.length > 0 ? summary : 'nothing to do');
  load();
}

// Choosing an integration installs it; this only refreshes what is there.
document.querySelector('#update').onclick = () =>
  post('/api/skills/install', {}, 'Updating…');
document.querySelector('#off').onclick = () => {
  if (!confirm('Turn ConsensFlow off? Every file it installed is removed and no agent will consult.')) return;
  post('/api/off', { confirm: true }, 'Turning off…');
};

async function load() {
  const [data, system] = await Promise.all([
    (await fetch('/api/participants', { headers })).json(),
    (await fetch('/api/system', { headers })).json(),
  ]);
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
  const res = await fetch('/api/participants', { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  document.querySelector('#error').textContent = res.ok ? '' : data.error;
  if (res.ok) { event.target.reset(); load(); }
};
load();
</script>
</body>
</html>
`
