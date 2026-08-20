import { spawn } from 'node:child_process'
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { detectAgents } from './agents.js'
import { CATALOG, EFFORTS } from './catalog.js'
import { installCmuxSkills } from './cmux-skills.js'
import { installSkill, skillsStatus, uninstallSkills } from './install.js'
import {
  addParticipant,
  editParticipant,
  listParticipants,
  RUNTIMES,
  removeParticipant,
} from './roster.js'
import { generateSkill, participantCommand } from './skill.js'
import { refreshInstalledSkill, retireSkillFromNativeHosts, skillTargets } from './sync.js'

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

/** Everything `cf doctor` and `cf skills status` would tell you, as data. */
function systemState(env) {
  const files = skillsStatus(env)
  const cmux = files.find((file) => file.source.startsWith('cmux@'))
  return {
    version: VERSION,
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
  let cmuxCommit = null
  if (body.withCmux === true) {
    const cmux = installCmuxSkills(env, { force: body.force === true })
    cmuxCommit = cmux.commit
    report.push(...cmux.report)
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
      if (request.method === 'POST' && url.pathname === '/api/skills/install') {
        const body = JSON.parse((await readBody(request)) || '{}')
        return send(200, installFromUi(body, env))
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

/** `cf ui`: start, print the URL, open the browser, run until Ctrl-C. */
export async function serveUi(env, { onOut }) {
  const server = await startUiServer(env)
  const url = `${server.url}/?token=${server.token}`
  onOut(`roster editor: ${url}`)
  onOut('Ctrl-C to stop — nothing keeps running after it.')
  spawn('open', [url], { stdio: 'ignore', detached: true }).unref()
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
  <p class="mark">consensflow</p>
  <h1>Your participants</h1>
  <p class="lede" id="lede">Each one is a real coding-agent CLI. Every change here rewrites the skill installed in your agents, so they can consult it by name.</p>

  <section id="roster"></section>

  <p class="eyebrow eyebrow--section">Ready-made</p>
  <div id="catalog"></div>

  <p class="eyebrow eyebrow--section">Skills</p>
  <div id="system"></div>
  <div class="actions">
    <button id="install" class="primary">Install / update skills</button>
    <label class="check"><input type="checkbox" id="with-cmux"> include cmux's own skills</label>
    <button id="uninstall" class="danger">Remove installed skills</button>
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

function renderSystem(system) {
  const host = document.querySelector('#system');
  host.innerHTML = '';
  const list = el('dl', 'facts');

  const hosts = el('div');
  for (const agent of system.agents) {
    const line = el('div', 'host');
    line.append(agent.id + ' ');
    line.append(el('span', null, agent.native ? '— has its own consensflow skill' : '— gets the generated skill'));
    hosts.append(line);
  }
  if (system.agents.length === 0) hosts.append(el('span', null, 'none on PATH'));

  const skills = system.skills.owned === 0
    ? 'none installed'
    : system.skills.owned + ' files'
      + (system.skills.cmuxCommit ? ' · cmux@' + system.skills.cmuxCommit : '')
      + (system.skills.drifted ? ' · ' + system.skills.drifted + ' edited by you' : '')
      + (system.skills.missing ? ' · ' + system.skills.missing + ' missing' : '');

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
  for (const row of data.report ?? []) counts[row.action] = (counts[row.action] ?? 0) + 1;
  const summary = Object.entries(counts).map(([action, n]) => n + ' ' + action).join(' · ');
  el2.textContent = summary.length > 0 ? summary : 'nothing to do';
  load();
}

document.querySelector('#install').onclick = () =>
  post('/api/skills/install', { withCmux: document.querySelector('#with-cmux').checked }, 'Installing…');
document.querySelector('#uninstall').onclick = () => {
  if (!confirm('Remove every skill file ConsensFlow installed?')) return;
  post('/api/skills/uninstall', { confirm: true }, 'Removing…');
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
