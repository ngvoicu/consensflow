import { createHash, timingSafeEqual } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { WORK_TIERS } from '../../hosts/lib/presets.js'
import { EFFORTS } from '../catalog.js'
import { HarnessAdmin } from '../harness-admin.js'
import { harnessPage } from '../harness-page.js'
import { addAgent, editAgent, HARNESSES, listAgents, removeAgent } from '../roster.js'

/**
 * The human's agents screens, served by the new core: the agents (`/`: the
 * catalog and the saved agents as one list, each saved agent with its tier)
 * and the harness diagnostics (`/harnesses`), each an inline page, with the
 * `/api/agents` routes they call. The app checks the UI token it was handed
 * and puts it on every request; the agents' own tokens open none of this.
 */

function tokenMatches(presented, token) {
  return timingSafeEqual(
    createHash('sha256').update(presented).digest(),
    createHash('sha256').update(token).digest(),
  )
}

const VERSION = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'package.json'), 'utf8'),
).version

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

/** The screens and their API, mounted by `startApi` under the UI token. */
export function agentsUi(
  env,
  { token, harnessLatest, harnessRun, onRosterChange = () => {} } = {},
) {
  if (typeof token !== 'string' || token.length === 0)
    throw new Error('the agents screens need a UI token')
  const harnessAdmin = new HarnessAdmin(env, {
    latest: harnessLatest,
    ...(harnessRun === undefined ? {} : { run: harnessRun }),
  })
  const html = (page) => ({ status: 200, html: page })
  const json = (status, body) => ({ status, body })

  return {
    token,
    /** Answers one of these screens' requests, or null when the path is not theirs. */
    async handle(request, url) {
      const path = url.pathname
      const page = ['/', '/harnesses'].includes(path)
      const named = /^\/api\/agents\/([a-z][a-z0-9-]*)$/.exec(path)
      const api =
        path === '/api/agents' ||
        path === '/api/harnesses/check' ||
        path === '/api/harnesses/update' ||
        named !== null
      if (!page && !api) return null
      const header = request.headers.authorization ?? ''
      const bearer = header.startsWith('Bearer ') ? header.slice('Bearer '.length) : ''
      const presented = bearer || (url.searchParams.get('token') ?? '')
      if (presented.length === 0 || !tokenMatches(presented, token)) {
        return json(401, { error: 'unauthorized' })
      }
      try {
        if (request.method === 'GET' && path === '/') return html(PAGE(token))
        if (request.method === 'GET' && path === '/harnesses') return html(harnessPage(token))
        if (request.method === 'GET' && path === '/api/agents') {
          return json(200, { agents: listAgents(env), harnesss: HARNESSES, efforts: EFFORTS })
        }
        const body = request.method === 'GET' ? {} : JSON.parse((await readBody(request)) || '{}')
        if (request.method === 'POST' && path === '/api/agents') {
          const agent = addAgent(body, env)
          onRosterChange()
          return json(201, { agent })
        }
        if (request.method === 'POST' && path === '/api/harnesses/update') {
          if (!['claude', 'codex', 'opencode', 'pi', 'kimi', 'devin'].includes(body.id)) {
            return json(400, { error: 'Unknown harness' })
          }
          return json(200, { result: await harnessAdmin.update(body.id) })
        }
        if (request.method === 'POST' && path === '/api/harnesses/check') {
          if (
            body.id !== undefined &&
            !['claude', 'codex', 'opencode', 'pi', 'kimi', 'devin'].includes(body.id)
          ) {
            return json(400, { error: 'Unknown harness' })
          }
          return json(200, {
            harnesses: await harnessAdmin.check(body.id ?? null, {
              refresh: body.refresh === true,
            }),
          })
        }
        if (named !== null && request.method === 'PATCH') {
          const agent = editAgent(named[1], body, env)
          onRosterChange()
          return json(200, { agent })
        }
        if (named !== null && request.method === 'DELETE') {
          removeAgent(named[1], env)
          onRosterChange()
          return { status: 204 }
        }
        return json(404, { error: 'not found' })
      } catch (cause) {
        return json(400, { error: cause instanceof Error ? cause.message : String(cause) })
      }
    },
  }
}

const BROWSING_CONTROLS = `
  <div class="filters">
    <label class="filter-search">Search agents<input type="search" placeholder="Name, model, harness or task…" autocomplete="off"></label>
    <label>Show<select name="show" aria-label="Show"><option value="all">All agents</option><option value="mine">My own agents</option></select></label>
    <label>Work tier<select name="tier" aria-label="Work tier"><option value="all">All tiers</option>${Object.entries(
      WORK_TIERS,
    )
      .map(([id, tier]) => `<option value="${id}">${tier.label}</option>`)
      .join('')}</select></label>
    <label>Group by<select name="group" aria-label="Group by"><option value="none">None</option><option value="harness">Harness</option><option value="model-reasoning" selected>Model and reasoning</option><option value="tier">Work tier</option></select></label>
    <button type="button">Clear filters</button>
  </div>
  <p class="tier-guide">The work tier is what a task finds an agent by.</p>`

const PAGE = (token) => `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ConsensFlow — Agents</title>
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
    --pill-worker: #63C7B2; --pill-lead: #ABC9F1; --pill-advisor: #EAC58B;
    --ui: Archivo, "Helvetica Neue", system-ui, sans-serif;
    --mono: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  @media (prefers-color-scheme: light) {
    :root {
      --ink: #E9F1EF; --panel: #FFFFFF; --line: #C9DAD8; --foam: #0C1E23;
      --muted: #52717A; --accent-text: #16766A; --buoy: #C2402F;
      --pill-worker: #176B5F; --pill-lead: #285A9C; --pill-advisor: #835D15;
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
  .tag--own { color: var(--accent-text); }
  .member__head .spacer { flex: 1; }
  /* A long command scrolls rather than wrapping (it stays one readable line);
     the fade is the only hint that there is more to the right. */
    content: ""; position: absolute; inset: 1px 1px 1px auto; width: 44px; border-radius: 0 4px 4px 0;
    background: linear-gradient(90deg, transparent, var(--panel)); pointer-events: none;
  }
    font-family: var(--mono); font-size: 11.5px; line-height: 1.6; color: var(--muted);
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
    padding: 9px 11px; margin: 0; overflow-x: auto; white-space: pre; scrollbar-width: thin;
  }

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

  .lede--tight { margin: 0 0 8px; }
  .filters input[type=search] {
    width: 100%; box-sizing: border-box; padding: 8px 10px;
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
    color: var(--foam); font: inherit; font-size: 13px;
  }
  .filters input[type=search]:focus-visible { outline: 2px solid var(--seafoam); outline-offset: 1px; }

  form { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-top: 14px; }
  input, select {
    font: inherit; font-size: 13px; padding: 7px 10px; color: var(--foam);
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
  }
  input::placeholder { color: var(--muted); }
  .full { grid-column: 1 / -1; }
  .alert { color: var(--buoy); font-size: 13px; margin: 0; }
  .note { font-size: 13px; color: var(--muted); min-height: 20px; margin: 10px 0 0; }
  .empty { color: var(--muted); font-size: 13.5px; border: 1px dashed var(--line); border-radius: 4px; padding: 18px; }
  @media (max-width: 620px) { form { grid-template-columns: 1fr; } }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; } }

  h1 { display: flex; flex-wrap: wrap; align-items: baseline; gap: 12px; }
  h3 { font-weight: 500; }
  .section-count { font: 12px var(--mono); color: var(--muted); margin-left: auto; }
  .filters { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; margin: 24px 0 32px; }
  .filters label { display: grid; gap: 5px; color: var(--muted); font-size: 12px; }
  .filters .filter-search { flex: 1 1 100%; }
  .filters input, .filters select { width: 100%; min-width: 0; }
  .filters button { padding: 7px 10px; }
  .tier-pill { display: inline-block; width: fit-content; border: 1px solid var(--muted); border-radius: 999px; padding: 4px 10px; margin: 6px 0; font-size: 12px; color: var(--foam); background: var(--panel); }
  .tier-pill[data-tier=critical] { border-color: var(--pill-advisor); color: var(--pill-advisor); }
  .tier-note { margin: 2px 0 8px; font-size: 12px; color: var(--muted); }
  .tier-guide { color: var(--muted); font-size: 12px; }
  .model-group { border: 1px solid var(--line); border-radius: 8px; padding: 16px; margin: 16px 0; }
  .model-summary { padding-bottom: 14px; overflow-wrap: anywhere; }
  .model-summary h3 { color: var(--foam); font-size: 18px; font-weight: 600; margin: 0 0 10px; }
  .model-group .member { padding: 12px 0; border-top: 1px solid var(--line); }
  .model-group .member:last-child { padding-bottom: 0; border-bottom: none; }
  .agent-route, .agent-route-note { color: var(--muted); font-size: 11px; }
  .member .agent-route, .member .agent-route-note { margin: 0; }
  .tag { overflow-wrap: anywhere; }
  button:disabled { opacity: .6; cursor: default; }
  @media (max-width: 620px) {
    .filters label { flex: 1 1 45%; min-width: 0; }
        }
</style>
</head>
<body>
<main>
  <p class="mark"><span>consensflow</span> <span>v${VERSION}</span></p>
  <section id="agents-section" aria-label="Agents">
  <h1>Agents <span id="agents-count" class="section-count"></span></h1>
  <p class="lede" id="lede">The agents a project's team is picked from: the catalog's, with your edits, and your own.</p>
  <p id="roster-note" class="note" role="status"></p>
  ${BROWSING_CONTROLS}
  <div id="agents"></div>
  </section>

  <p class="eyebrow eyebrow--section">Define your own</p>
  <form id="add">
    <input name="name" placeholder="callsign, lowercase" required>
    <select name="harness"></select>
    <input class="full" name="model" placeholder="model — anything this harness accepts" required>
    <input name="effort" list="effort-options" placeholder="effort (optional)">
    <datalist id="effort-options"></datalist>
    <label class="full">Work tier<select name="workTier"><option value="auto">Automatic (model and reasoning)</option>${Object.entries(
      WORK_TIERS,
    )
      .map(([id, tier]) => `<option value="${id}">${tier.label}</option>`)
      .join('')}</select></label>
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

const HARNESS_LABELS = { claude: 'Claude Code', codex: 'Codex', opencode: 'OpenCode', pi: 'Pi', kimi: 'Kimi', devin: 'Devin', image: 'Images' };
const WORK_TIERS = ${JSON.stringify(WORK_TIERS)};
const EFFORT_ORDER = ['ultra', 'max', 'xhigh', 'high', 'medium', 'low', 'minimal', 'off', 'default', 'kimi-setting', 'not-applicable'];
const effortValue = p => p.harness === 'image' ? 'not-applicable' : (p.effort || (p.harness === 'kimi' ? 'kimi-setting' : 'default'));
const effortLabel = value => value === 'not-applicable' ? 'Not applicable' : value === 'kimi-setting' ? 'Kimi setting' : EFFORT_ORDER.includes(value) ? value.charAt(0).toUpperCase() + value.slice(1) : value;
const rank = (values, value) => values.includes(value) ? values.indexOf(value) : values.length;
const compareText = (a, b) => String(a).localeCompare(String(b));
// Curated family/tier order. Provider paths affect routing and identity, not rank.
// Versions and aliases stay separate; their numeric label is only a tie-breaker.
const MODEL_ORDER = [
  /^claude-fable(?:-|$)/, /^claude-opus(?:-|$)/, /^claude-sonnet(?:-|$)/, /^claude-haiku(?:-|$)/, /^claude-/,
  /^gpt-[0-9.]+-astra(?:-|$)/, /^gpt-[0-9.]+-sol(?:-|$)/, /^gpt-[0-9.]+-terra(?:-|$)/, /^gpt-[0-9.]+-luna(?:-|$)/, /^gpt-/,
  /^gemini-[0-9.]+-pro(?:-|$)/, /^gemini-[0-9.]+-flash(?:-|$)/, /^gemini-/,
  /^deepseek-v4(?:\.\d+)?-pro(?:-|$)/, /^deepseek-v4(?:\.\d+)?-flash(?:-|$)/, /^deepseek-/,
  /^glm-[0-9.]+$/, /^glm-[0-9.]+-flash(?:-|$)/, /^glm-/,
  /^grok-/, /^kimi-/, /^laguna-/, /^minimax-/, /^muse-/, /^nemotron-/,
  /^qwen[0-9.]+-max(?:-|$)/, /^qwen[0-9.]+-27b(?:-|$)/, /^qwen[0-9]/,
  /^codex-image$/,
];
function modelRank(key) {
  const index = MODEL_ORDER.findIndex(pattern => pattern.test(key.split('/').at(-1)));
  return index < 0 ? MODEL_ORDER.length : index;
}
const compareModels = (a, b) => modelRank(a.modelKey) - modelRank(b.modelKey) ||
  b.modelLabel.localeCompare(a.modelLabel, undefined, { numeric: true }) || compareText(a.modelKey, b.modelKey);
const compareEffort = (a, b) => rank(EFFORT_ORDER, a) - rank(EFFORT_ORDER, b) || compareText(a, b);
const compareAgents = (a, b) => compareModels(a.profile, b.profile) ||
  compareEffort(effortValue(a), effortValue(b)) || compareText(a.name, b.name);

function browsingGroups(entries, sectionId) {
  const section = document.querySelector(sectionId);
  const needle = section.querySelector('input[type=search]').value.trim().toLowerCase();
  const tier = section.querySelector('[name=tier]').value;
  const by = section.querySelector('[name=group]').value;
  const filtered = entries.filter(p => (tier === 'all' || p.profile.workTier === tier) &&
    [p.name, p.model, p.description, p.detail, p.harness, HARNESS_LABELS[p.harness], effortLabel(effortValue(p)),
      WORK_TIERS[p.profile.workTier].label, p.profile.modelLabel, p.profile.routeLabel, p.profile.routeNote]
      .filter(Boolean).join(' ').toLowerCase().includes(needle));
  section.querySelector('.section-count').textContent = filtered.length + ' of ' + entries.length + ' shown';
  const groups = new Map();
  for (const p of filtered) {
    const effort = effortValue(p);
    const key = by === 'model-reasoning' ? JSON.stringify([p.profile.modelKey, effort]) : by === 'harness' ? p.harness : by === 'tier' ? p.profile.workTier : '';
    const title = by === 'model-reasoning' ? p.profile.modelLabel + ' · ' + effortLabel(effort) : by === 'harness' ? (HARNESS_LABELS[key] || key) : by === 'tier' ? WORK_TIERS[key].label : '';
    if (!groups.has(key)) groups.set(key, { key, title, modelGroup: by === 'model-reasoning', modelKey: p.profile.modelKey, modelLabel: p.profile.modelLabel, effort, rows: [] });
    groups.get(key).rows.push(p);
  }
  for (const group of groups.values()) {
    group.rows.sort(compareAgents);
    group.shared = group.modelGroup ? ['workTier'].filter(field =>
      group.rows.every(p => JSON.stringify(p.profile[field]) === JSON.stringify(group.rows[0].profile[field]))) : [];
    // The tier's note goes with the tier pill: said once on the card when the tier is.
    if (group.shared.includes('workTier')) group.shared.push('tierNote');
  }
  return [...groups.values()].sort((a, b) =>
    (by === 'tier' ? rank(Object.keys(WORK_TIERS), a.key) - rank(Object.keys(WORK_TIERS), b.key) : by === 'harness' ? rank(Object.keys(HARNESS_LABELS), a.key) - rank(Object.keys(HARNESS_LABELS), b.key) :
      by === 'model-reasoning' ? compareModels(a, b) || compareEffort(a.effort, b.effort) : 0) || compareText(a.title, b.title) || compareText(a.key, b.key));
}
function groupSection(group, fields = group.shared) {
  const section = el('section', group.modelGroup ? 'agent-group model-group' : 'agent-group');
  if (group.modelGroup) {
    const summary = el('header', 'model-summary');
    summary.append(el('h3', null, group.title + ' · ' + group.rows.length));
    appendProfile(summary, group.rows[0], fields.filter(field => group.shared.includes(field)));
    section.append(summary);
  } else if (group.title) section.append(el('h3', 'eyebrow eyebrow--tool', group.title + ' · ' + group.rows.length));
  return section;
}
function appendProfile(host, p, fields) {
  if (fields.includes('workTier')) {
    const tier = WORK_TIERS[p.profile.workTier];
    const pill = el('span', 'tier-pill', 'T' + (Object.keys(WORK_TIERS).indexOf(p.profile.workTier) + 1) + ' · ' + tier.label);
    pill.dataset.tier = p.profile.workTier;
    pill.title = tier.description;
    host.append(pill);
    if (fields.includes('tierNote') && p.profile.workTier === 'critical') host.append(el('p', 'tier-note', 'Important work only · No coding'));
  }
  if (fields.includes('routeLabel')) {
    host.append(el('p', 'agent-route', p.profile.routeLabel));
    if (p.profile.routeNote) host.append(el('p', 'agent-route-note', p.profile.routeNote));
  }
}

/**
 * An agent's row, the same for every agent: its name, what it runs, its tier
 * and route; Edit and Remove when it is the human's own, since a catalog
 * agent stays as the catalog has it.
 */
function agentCard(p, group, editors) {
  const card = el('div', 'member');
  card.dataset.agentName = p.name;
  if (p.custom) card.dataset.custom = 'true';
  const head = el('div', 'member__head');
  head.append(el('span', 'callsign', p.name));
  head.append(el('span', 'tag', group.modelGroup ? (HARNESS_LABELS[p.harness] || p.harness) : p.profile.modelLabel + ' · ' + (HARNESS_LABELS[p.harness] || p.harness) + ' · ' + effortLabel(effortValue(p))));
  if (p.custom) head.append(el('span', 'tag tag--own', 'your own'));
  head.append(el('span', 'spacer'));
  if (p.custom) {
    const edit = el('button', null, 'Edit');
    edit.onclick = () => openEditor(card, p);
    head.append(edit, removeButton(p, 'Remove'));
  }
  card.append(head);
  // The row shows what a task finds it by, its tier, and how it is billed;
  // the model card above says the rest once for the model.
  appendProfile(card, p, ['workTier', 'tierNote', 'routeLabel'].filter(field => !group.shared.includes(field)));
  if (editors.has(p.name)) card.append(editors.get(p.name));
  return card;
}

/**
 * One list: every catalog agent and every agent defined by hand, grouped as
 * the controls say. Show narrows it to the human's own.
 */
function renderAgents(data) {
  const host = document.querySelector('#agents');
  const editors = new Map([...host.querySelectorAll('.member[data-agent-name]')]
    .map(card => [card.dataset.agentName, card.querySelector('form')]).filter(([, form]) => form));
  host.innerHTML = '';
  const show = document.querySelector('#agents-section [name=show]').value;
  const entries = data.agents.filter(p => show === 'all' || p.custom);
  const mine = data.agents.filter(p => p.custom).length;
  document.querySelector('#lede').textContent = data.agents.length + ' agents, the catalog’s and your own; a project’s team is picked from them.' +
    (mine === 0 ? '' : ' ' + mine + ' ' + (mine === 1 ? 'is' : 'are') + ' yours, defined here.');
  const groups = browsingGroups(entries, '#agents-section');
  if (groups.length === 0) { host.append(el('p', 'empty', 'No agents match these filters.')); return; }
  for (const group of groups) {
    const section = groupSection(group, ['workTier', 'tierNote']);
    for (const agent of group.rows) section.append(agentCard(agent, group, editors));
    host.append(section);
  }
}

/** Editing an agent is changing its model, effort or tier; an image agent has only its tier. */
function openEditor(card, agent) {
  if (card.querySelector('form')) return;
  const form = el('form', 'form');
  const fields = [
    ['model', agent.model, 'model'],
    ['effort', agent.effort ?? '', 'effort (blank for none)'],
  ];
  for (const [name, value, placeholder] of agent.harness === 'image' ? [] : fields) {
    const input = document.createElement('input');
    input.name = name;
    input.value = value;
    input.placeholder = placeholder;
    input.className = 'full';
    form.append(input);
  }
  const tierLabel = el('label', 'full', 'Work tier');
  const tierSelect = document.createElement('select');
  tierSelect.name = 'workTier';
  tierSelect.add(new Option('Automatic (model and reasoning)', 'auto'));
  for (const [id, tier] of Object.entries(WORK_TIERS)) tierSelect.add(new Option(tier.label, id));
  tierSelect.value = agent.workTier ?? 'auto';
  tierLabel.append(tierSelect);
  form.append(tierLabel);
  const save = el('button', 'primary', 'Save');
  save.type = 'submit';
  const cancel = el('button', null, 'Cancel');
  cancel.type = 'button';
  cancel.onclick = () => form.remove();
  form.append(save, cancel);
  form.onsubmit = async (event) => {
    event.preventDefault();
    const entries = Object.fromEntries(new FormData(form).entries());
    if (entries.workTier === 'auto') entries.workTier = null;
    const res = await fetch('/api/agents/' + agent.name, {
      method: 'PATCH',
      headers,
      body: JSON.stringify(entries),
    });
    if (res.ok) { form.remove(); load(); }
    else document.querySelector('#roster-note').textContent = (await res.json()).error;
  };
  card.append(form);
}

const pendingRemovals = new Set();
function removeButton(agent, label) {
  const button = el('button', 'danger', pendingRemovals.has(agent.name) ? 'Removing…' : label);
  button.dataset.removeAgent = agent.name;
  button.disabled = pendingRemovals.has(agent.name);
  button.onclick = async () => {
    if (pendingRemovals.has(agent.name)) return;
    pendingRemovals.add(agent.name);
    for (const action of document.querySelectorAll('[data-remove-agent]')) {
      if (action.dataset.removeAgent !== agent.name) continue;
      action.disabled = true;
      action.textContent = 'Removing…';
    }
    const status = document.querySelector('#roster-note');
    status.textContent = '';
    try {
      const response = await fetch('/api/agents/' + encodeURIComponent(agent.name), { method: 'DELETE', headers });
      if (!response.ok) throw new Error((await response.json()).error || 'Could not remove agent');
    } catch (error) {
      status.textContent = error.message || 'Could not remove agent';
    } finally {
      pendingRemovals.delete(agent.name);
      try { await load(); } catch {
        renderLists();
        if (!status.textContent) status.textContent = 'Could not refresh agents. Reopen this screen to try again.';
      }
    }
  };
  return button;
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
  const model = document.querySelector('#add [name=model]');
  const effort = document.querySelector('#add [name=effort]');
  if (harness === 'image') {
    if (!model.hidden) model.dataset.previous = model.value;
    model.value = 'codex-image';
  } else if (model.hidden) {
    model.value = model.dataset.previous || '';
    delete model.dataset.previous;
  }
  model.hidden = harness === 'image';
  effort.hidden = harness === 'image';
  effort.disabled = harness === 'image';
  const list = document.querySelector('#effort-options');
  list.innerHTML = '';
  for (const e of efforts[harness] ?? []) list.appendChild(new Option(e, e));
}

function renderLists() { if (LAST !== null) renderAgents(LAST); }
{
  const filters = document.querySelector('#agents-section .filters');
  const refresh = renderLists;
  filters.querySelector('input').addEventListener('input', refresh);
  for (const select of filters.querySelectorAll('select')) select.addEventListener('change', refresh);
  filters.querySelector('button').onclick = () => {
    filters.querySelector('input').value = '';
    filters.querySelector('[name=tier]').value = 'all';
    filters.querySelector('[name=group]').value = 'model-reasoning';
    filters.querySelector('[name=show]').value = 'all';
    refresh();
  };
}

// The last roster payload, so filtering the catalog re-renders without a fetch.
let LAST = null;

async function load() {
  const response = await fetch('/api/agents', { headers });
  if (!response.ok) throw new Error('Could not refresh agents. Reopen this screen to try again.');
  LAST = await response.json();
  renderLists();
  renderForm(LAST);
}

document.querySelector('#add').onsubmit = async (event) => {
  event.preventDefault();
  const form = new FormData(event.target);
  const body = Object.fromEntries([...form.entries()].filter(([, v]) => v !== ''));
  if (body.workTier === 'auto') delete body.workTier;
  const res = await fetch('/api/agents', { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  document.querySelector('#error').textContent = res.ok ? '' : data.error;
  if (res.ok) { event.target.reset(); load(); }
};
function refreshAgents() {
  load().catch(error => { document.querySelector('#roster-note').textContent = error.message; });
}
window.addEventListener('message', event => {
  if (event.source === window.parent && event.data === 'consensflow:refresh-agents') refreshAgents();
});
refreshAgents();
</script>
</body>
</html>
`
