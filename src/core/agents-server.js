import { createHash, timingSafeEqual } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { ArtificialAnalysis, METRICS, withBenchmarks } from '../../hosts/lib/benchmarks.js'
import { CATEGORY_LABELS, WORK_TIERS } from '../../hosts/lib/presets.js'
import { agentProfile, CATALOG, EFFORTS } from '../catalog.js'
import { HarnessAdmin } from '../harness-admin.js'
import { harnessPage } from '../harness-page.js'
import {
  addAgent,
  agentDrift,
  configRoot,
  editAgent,
  HARNESSES,
  listAgents,
  refreshAgentProfiles,
  removeAgent,
  syncAgents,
} from '../roster.js'

/**
 * The human's agents screens, served by the new core: the roster editor
 * (`/`, with each agent's tier and tags), the agent library (`/library`) and
 * the harness diagnostics (`/harnesses`), each an inline page, with the
 * `/api/agents` routes they call. The app checks the UI token it was handed
 * and puts it on every request; the agents' own tokens open none of this.
 */

export function tokenMatches(presented, token) {
  return timingSafeEqual(
    createHash('sha256').update(presented).digest(),
    createHash('sha256').update(token).digest(),
  )
}

const VERSION = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'package.json'), 'utf8'),
).version

/** The agent with its profile and benchmark scores, as the screens show it. */
export function withProfile(agent, benchmarks) {
  return { ...agent, profile: withBenchmarks(agent, agentProfile(agent), benchmarks) }
}

export function readBody(request) {
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
export function agentsUi(env, { token, harnessLatest } = {}) {
  if (typeof token !== 'string' || token.length === 0)
    throw new Error('the agents screens need a UI token')
  const harnessAdmin = new HarnessAdmin(env, { latest: harnessLatest })
  const artificialAnalysis = new ArtificialAnalysis(configRoot(env))
  const html = (page) => ({ status: 200, html: page })
  const json = (status, body) => ({ status, body })

  return {
    token,
    /** Answers one of these screens' requests, or null when the path is not theirs. */
    async handle(request, url) {
      const path = url.pathname
      const page = ['/', '/library', '/harnesses'].includes(path)
      const named = /^\/api\/agents\/([a-z][a-z0-9-]*)$/.exec(path)
      const api =
        path === '/api/agents' ||
        path === '/api/agents/sync' ||
        path === '/api/harnesses/check' ||
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
        if (request.method === 'GET' && path === '/library') return html(PAGE(token, true))
        if (request.method === 'GET' && path === '/harnesses') return html(harnessPage(token))
        if (request.method === 'GET' && path === '/api/agents') {
          const benchmarks = await artificialAnalysis.refresh()
          refreshAgentProfiles(env, benchmarks)
          return json(200, {
            agents: listAgents(env).map((agent) => withProfile(agent, benchmarks)),
            drift: agentDrift(env),
            harnesss: HARNESSES,
            catalog: Object.fromEntries(
              Object.entries(CATALOG).map(([harness, entries]) => [
                harness,
                entries.map((entry) => withProfile({ ...entry, harness }, benchmarks)),
              ]),
            ),
            efforts: EFFORTS,
            benchmarks: {
              status: benchmarks.status,
              tier: benchmarks.tier,
              fetchedAt: benchmarks.fetchedAt,
              indexVersion: benchmarks.indexVersion,
              metrics: METRICS,
            },
          })
        }
        const body = request.method === 'GET' ? {} : JSON.parse((await readBody(request)) || '{}')
        if (request.method === 'POST' && path === '/api/agents/sync') {
          const applied = syncAgents(env, {
            ...(typeof body.name === 'string' ? { name: body.name } : {}),
          })
          return json(200, { applied, agents: listAgents(env).map(withProfile) })
        }
        if (request.method === 'POST' && path === '/api/agents') {
          return json(201, { agent: addAgent(body, env) })
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
          return json(200, { agent: editAgent(named[1], body, env) })
        }
        if (named !== null && request.method === 'DELETE') {
          removeAgent(named[1], env)
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
    <label>Work tier<select name="tier" aria-label="Work tier"><option value="all">All tiers</option>${Object.entries(
      WORK_TIERS,
    )
      .map(([id, tier]) => `<option value="${id}">${tier.label}</option>`)
      .join('')}</select></label>
    <label>Category<select name="category" aria-label="Category"><option value="all">All categories</option><optgroup label="Task capabilities">${Object.entries(
      CATEGORY_LABELS,
    )
      .filter(([id]) => !['lead', 'pm'].includes(id))
      .map(([id, label]) => `<option value="${id}">${label}</option>`)
      .join(
        '',
      )}</optgroup><optgroup label="Coordinator recommendations"><option value="lead">Lead candidate</option><option value="pm">PM candidate</option></optgroup></select></label>
    <label>Group by<select name="group" aria-label="Group by"><option value="none">None</option><option value="harness">Harness</option><option value="model-reasoning" selected>Model and reasoning</option><option value="tier">Work tier</option></select></label>
    <label>Sort by<select name="sort" aria-label="Sort by"><option value="default">Model and reasoning</option></select></label>
    <button type="button">Clear filters</button>
  </div>
  <p class="tier-guide">Work tier sets the assignment scope. Capability tags show suitable tasks; lead and PM tags are recommendations.</p>
  <p class="benchmark-source"></p>
  <details class="benchmark-guide"><summary>About benchmark scores</summary><div></div></details>`

export const PAGE = (token, library = false) => `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ConsensFlow — ${library ? 'Agent library' : 'Your agents'}</title>
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
    --pill-coding: #63C7B2; --pill-lead: #ABC9F1; --pill-pm: #DDC6EF; --pill-images: #EAC58B;
    --ui: Archivo, "Helvetica Neue", system-ui, sans-serif;
    --mono: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  @media (prefers-color-scheme: light) {
    :root {
      --ink: #E9F1EF; --panel: #FFFFFF; --line: #C9DAD8; --foam: #0C1E23;
      --muted: #52717A; --accent-text: #16766A; --buoy: #C2402F;
      --pill-coding: #176B5F; --pill-lead: #285A9C; --pill-pm: #734A91; --pill-images: #835D15;
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
  .member__head .spacer { flex: 1; }
  .member__desc, .member__tags { color: var(--muted); font-size: 13px; margin: 0; }
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

  .offer { display: flex; align-items: baseline; gap: 12px; padding: 8px 0; border-top: 1px solid var(--line); }
  .offer:first-of-type { border-top: none; }
  .offer__name { font-family: var(--mono); font-size: 13px; color: var(--foam); min-width: 96px; }
  .offer__what { color: var(--muted); font-size: 13px; flex: 1; }
  .offer__model { font-family: var(--mono); font-size: 11px; color: var(--muted); opacity: .8; }
  .member__drift { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin: 6px 0 0; font-size: 13px; color: var(--muted); }
  .tag--moved { background: var(--buoy); color: var(--ink); }
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
  @media (max-width: 620px) { form { grid-template-columns: 1fr; } .offer { flex-wrap: wrap; } }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; } }

  h1 { display: flex; flex-wrap: wrap; align-items: baseline; gap: 12px; }
  h3 { font-weight: 500; }
  .section-count { font: 12px var(--mono); color: var(--muted); margin-left: auto; }
  .filters { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; margin: 24px 0 32px; }
  .filters label { display: grid; gap: 5px; color: var(--muted); font-size: 12px; }
  .filters .filter-search { flex: 1 1 100%; }
  .filters input, .filters select { width: 100%; min-width: 0; }
  .filters button { padding: 7px 10px; }
  .offer { align-items: start; }
  .offer__what { min-width: 0; overflow-wrap: anywhere; }
  .offer__what p { margin: 4px 0 0; }
  .category-pills { display: flex; flex-wrap: wrap; gap: 6px; list-style: none; padding: 0; margin: 6px 0; }
  .category-pill { border: 1px solid currentColor; border-radius: 999px; padding: 2px 8px; font-size: 11px; line-height: 1.4; white-space: nowrap; color: var(--pill-coding); background: var(--panel); }
  .tier-pill { display: inline-block; width: fit-content; border: 1px solid var(--muted); border-radius: 999px; padding: 4px 10px; margin: 6px 0; font-size: 12px; color: var(--foam); }
  .tier-pill[data-tier=critical] { border-color: var(--pill-images); color: var(--pill-images); }
  .tier-note { margin: 2px 0 8px; font-size: 12px; color: var(--muted); }
  .tier-guide { color: var(--muted); font-size: 12px; }
  .category-pill[data-kind=role] { border-style: dashed; }
  .category-pill[data-category=lead] { color: var(--pill-lead); }
  .category-pill[data-category=pm] { color: var(--pill-pm); }
  .category-pill[data-category=reviewer] { color: var(--foam); }
  .category-pill[data-category=images] { color: var(--pill-images); }
  .benchmark-source, .benchmark-guide, .benchmark-details, .benchmark-missing, .benchmark-context { font-size: 12px; color: var(--muted); }
  .benchmark-source { margin: -16px 0 4px; }
  .benchmark-guide { margin: 0 0 22px; }
  .benchmark-pills { display: flex; flex-wrap: wrap; gap: 6px; list-style: none; padding: 0; margin: 8px 0; }
  .benchmark-pill { color: var(--foam); background: var(--panel); border: 1px solid var(--line); border-radius: 999px; padding: 3px 8px; font: 11px/1.4 var(--mono); max-width: 100%; }
  .benchmark-pill[data-selected=true] { border-color: var(--accent-text); }
  .benchmark-details summary, .benchmark-guide summary { cursor: pointer; width: fit-content; color: var(--accent-text); }
  .benchmark-details p, .benchmark-guide p { margin: 8px 0; overflow-wrap: anywhere; }
  .benchmark-details a, .benchmark-source a { color: var(--accent-text); }
  .benchmark-guide dt, .benchmark-details dt { margin-top: 8px; color: var(--foam); font-weight: 600; }
  .benchmark-guide dd, .benchmark-details dd { margin: 2px 0 10px; }
  .model-group { border: 1px solid var(--line); border-radius: 8px; padding: 16px; margin: 16px 0; }
  .model-summary { padding-bottom: 14px; overflow-wrap: anywhere; }
  .model-summary h3 { color: var(--foam); font-size: 18px; font-weight: 600; margin: 0 0 10px; }
  .model-summary .agent-focus { margin: 8px 0; }
  .model-group .offer, .model-group .member { padding: 12px 0; border-top: 1px solid var(--line); }
  .model-group .offer:last-child, .model-group .member:last-child { padding-bottom: 0; border-bottom: none; }
  .agent-focus { color: var(--foam); font-size: 13px; }
  .agent-route, .agent-route-note { color: var(--muted); font-size: 11px; }
  .member .agent-focus, .member .agent-route, .member .agent-route-note { margin: 0; }
  .offer__model { color: var(--foam); opacity: 1; display: block; overflow-wrap: anywhere; }
  .offer__actions { display: flex; flex-wrap: wrap; gap: 6px; justify-content: flex-end; max-width: 220px; }
  .offer__actions button { overflow-wrap: anywhere; max-width: 100%; }
  .member__desc, .member__tags, .tag { overflow-wrap: anywhere; }
  button:disabled { opacity: .6; cursor: default; }
  @media (max-width: 620px) {
    .filters label { flex: 1 1 45%; min-width: 0; }
    .offer__name { min-width: 80px; }
    .offer__what { flex: 1 1 180px; }
    .offer__actions { margin-left: auto; max-width: 100%; }
  }
</style>
</head>
<body>
<main>
  <p class="mark"><span>consensflow</span> <span>v${VERSION}</span></p>
  ${
    library
      ? `
  <section id="catalog-section" aria-label="Agent library">
  <h1>Agent library <span id="catalog-count" class="section-count"></span></h1>
  <p class="lede lede--tight">Add an agent with its model and reasoning effort already configured.</p>
  <p id="roster-note" class="note" role="status"></p>
  ${BROWSING_CONTROLS}
  <div id="catalog"></div>
  </section>

`
      : `
  <section id="roster-section" aria-label="Your agents">
  <h1>Your agents <span id="roster-count" class="section-count"></span></h1>
  <p class="lede" id="lede">Configure the workers your lead can consult by name.</p>
  <p id="roster-note" class="note" role="status"></p>
  ${BROWSING_CONTROLS}
  <div id="roster"></div>
  </section>

  <p class="eyebrow eyebrow--section">Define your own</p>
  <form id="add">
    <input name="name" placeholder="callsign, lowercase" required>
    <select name="harness"></select>
    <input class="full" name="model" placeholder="model — anything this harness accepts" required>
    <input name="effort" list="effort-options" placeholder="effort (optional)">
    <datalist id="effort-options"></datalist>
    <input class="full" name="tags" placeholder="tags: what it is good for, comma-separated (optional)">
    <button class="primary">Add agent</button>
    <p id="error" class="alert full"></p>
  </form>`
  }

</main>
<script>
const TOKEN = ${JSON.stringify(token)};
const LIBRARY = ${library};
const headers = { authorization: 'Bearer ' + TOKEN, 'content-type': 'application/json' };
const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

/** "coding, rust" as the list the roster keeps; an empty field clears the tags. */
const tagList = text => {
  const tags = text.split(',').map(tag => tag.trim()).filter(Boolean);
  return tags.length === 0 ? null : tags;
};


const HARNESS_LABELS = { claude: 'Claude Code', codex: 'Codex', opencode: 'OpenCode', pi: 'Pi', kimi: 'Kimi', devin: 'Devin', image: 'Images' };
const CATEGORY_LABELS = ${JSON.stringify(CATEGORY_LABELS)};
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
  /^deepseek-v4-pro(?:-|$)/, /^deepseek-v4-flash(?:-|$)/, /^deepseek-/,
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
function compareScores(a, b, metric) {
  if (!metric) return 0;
  const av = a.profile.benchmarks?.scores[metric.id], bv = b.profile.benchmarks?.scores[metric.id];
  const ah = Number.isFinite(av), bh = Number.isFinite(bv);
  if (ah !== bh) return ah ? -1 : 1;
  return ah ? (metric.direction === 'asc' ? av - bv : bv - av) : 0;
}
const selectedMetric = () => LAST?.benchmarks?.metrics.find(m => m.id === document.querySelector('[name=sort]').value);

function browsingGroups(entries, sectionId) {
  const section = document.querySelector(sectionId);
  const needle = section.querySelector('input[type=search]').value.trim().toLowerCase();
  const category = section.querySelector('[name=category]').value;
  const tier = section.querySelector('[name=tier]').value;
  const by = section.querySelector('[name=group]').value;
  const metric = selectedMetric();
  const filtered = entries.filter(p => (tier === 'all' || p.profile.workTier === tier) && (category === 'all' || p.profile.categories.includes(category)) &&
    [p.name, p.model, p.description, p.detail, p.harness, HARNESS_LABELS[p.harness], effortLabel(effortValue(p)),
      WORK_TIERS[p.profile.workTier].label, p.profile.modelLabel, p.profile.routeLabel, p.profile.routeNote, p.profile.goodFor, ...p.profile.categories.map(c => CATEGORY_LABELS[c])]
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
    group.rows.sort((a, b) => compareScores(a, b, metric) || compareAgents(a, b));
    group.shared = group.modelGroup ? ['workTier', 'categories', 'goodFor', 'benchmarks'].filter(field =>
      group.rows.every(p => JSON.stringify(p.profile[field]) === JSON.stringify(group.rows[0].profile[field]))) : [];
  }
  return [...groups.values()].sort((a, b) =>
    (by === 'tier' ? rank(Object.keys(WORK_TIERS), a.key) - rank(Object.keys(WORK_TIERS), b.key) : by === 'harness' ? rank(Object.keys(HARNESS_LABELS), a.key) - rank(Object.keys(HARNESS_LABELS), b.key) :
      by === 'model-reasoning' ? compareScores(a.rows[0], b.rows[0], metric) || compareModels(a, b) || compareEffort(a.effort, b.effort) : 0) || compareText(a.title, b.title) || compareText(a.key, b.key));
}
function groupSection(group) {
  const section = el('section', group.modelGroup ? 'agent-group model-group' : 'agent-group');
  if (group.modelGroup) {
    const summary = el('header', 'model-summary');
    summary.append(el('h3', null, group.title + ' · ' + group.rows.length));
    appendProfile(summary, group.rows[0], group.shared);
    section.append(summary);
  } else if (group.title) section.append(el('h3', 'eyebrow eyebrow--tool', group.title + ' · ' + group.rows.length));
  return section;
}
function appendProfile(host, p, fields = ['workTier', 'categories', 'goodFor', 'routeLabel', 'benchmarks']) {
  if (fields.includes('workTier')) {
    const tier = WORK_TIERS[p.profile.workTier];
    const pill = el('span', 'tier-pill', 'T' + (Object.keys(WORK_TIERS).indexOf(p.profile.workTier) + 1) + ' · ' + tier.label);
    pill.dataset.tier = p.profile.workTier;
    pill.title = tier.description;
    host.append(pill);
    if (p.profile.workTier === 'critical') host.append(el('p', 'tier-note', 'Important work only · No coding'));
  }
  if (fields.includes('categories') && p.profile.categories.length) {
    const categories = el('ul', 'category-pills');
    categories.setAttribute('aria-label', 'Categories');
    categories.setAttribute('role', 'list');
    for (const category of p.profile.categories) {
      const pill = el('li', 'category-pill', CATEGORY_LABELS[category]);
      pill.dataset.category = category;
      pill.dataset.kind = ['lead', 'pm'].includes(category) ? 'role' : 'capability';
      pill.title = ['lead', 'pm'].includes(category) ? 'Coordinator recommendation; does not launch a role.' : CATEGORY_LABELS[category];
      categories.append(pill);
    }
    host.append(categories);
  }
  if (fields.includes('goodFor')) host.append(el('p', 'agent-focus', 'Good for: ' + p.profile.goodFor));
  if (fields.includes('routeLabel')) {
    host.append(el('p', 'agent-route', p.profile.routeLabel));
    if (p.profile.routeNote) host.append(el('p', 'agent-route-note', p.profile.routeNote));
  }
  if (fields.includes('benchmarks')) appendBenchmarks(host, p);
}

const metricValue = (metric, value) => value.toFixed(1) + (metric.unit === '%' ? '%' : metric.unit === 'Elo' ? ' Elo' : '');
function benchmarkPills(metrics, scores) {
  const list = el('ul', 'benchmark-pills');
  list.setAttribute('aria-label', 'Artificial Analysis scores');
  for (const metric of metrics) {
    const pill = el('li', 'benchmark-pill', metric.label + ' ' + metricValue(metric, scores[metric.id]));
    pill.dataset.metric = metric.id;
    pill.dataset.selected = String(selectedMetric()?.id === metric.id);
    list.append(pill);
  }
  return list;
}
function appendBenchmarks(host, p) {
  const snapshot = p.profile.benchmarks;
  if (!snapshot) {
    if (LAST?.benchmarks?.fetchedAt) host.append(el('p', 'benchmark-missing', 'No AA score for this model and reasoning setting.'));
    return;
  }
  const metrics = LAST.benchmarks.metrics.filter(m => Number.isFinite(snapshot.scores[m.id]));
  const primary = m => ['intelligence', 'coding', 'agentic', selectedMetric()?.id].includes(m.id);
  if (snapshot.reasoningMatch === 'unspecified') host.append(el('p', 'benchmark-context', 'AA reasoning level not specified'));
  host.append(benchmarkPills(metrics.filter(primary), snapshot.scores));
  const details = el('details', 'benchmark-details');
  details.append(el('summary', null, 'Benchmark details · ' + metrics.length + ' scores'));
  details.append(el('p', null, 'Tested: ' + snapshot.testedModel));
  details.append(el('p', null, 'AA index v' + snapshot.indexVersion + ' · Retrieved ' + new Date(snapshot.fetchedAt).toLocaleDateString()));
  details.append(el('p', null, 'AA tests its own configuration. Results can differ with this agent’s harness and provider.'));
  const link = el('a', null, 'AA model result');
  link.href = snapshot.url; link.target = '_blank'; link.rel = 'noopener noreferrer';
  details.append(link);
  details.append(benchmarkPills(metrics.filter(m => !primary(m)), snapshot.scores));
  const definitions = el('dl');
  for (const metric of metrics) {
    definitions.append(el('dt', null, metric.label + ': ' + metricValue(metric, snapshot.scores[metric.id])));
    definitions.append(el('dd', null, metric.description));
  }
  details.append(definitions);
  host.append(details);
}
function renderBenchmarkControls(data) {
  const info = data.benchmarks;
  const rows = [...data.agents, ...Object.values(data.catalog).flat()];
  const select = document.querySelector('[name=sort]');
  const selected = select.value;
  select.replaceChildren(new Option('Model and reasoning', 'default'));
  for (const metric of info.metrics) {
    if (rows.some(row => Number.isFinite(row.profile.benchmarks?.scores[metric.id])))
      select.add(new Option(metric.label + ' · ' + (metric.direction === 'asc' ? 'lowest first' : 'highest first'), metric.id));
  }
  if ([...select.options].some(option => option.value === selected)) select.value = selected;
  const source = document.querySelector('.benchmark-source');
  source.replaceChildren();
  const link = el('a', null, 'Artificial Analysis');
  link.href = 'https://artificialanalysis.ai/'; link.target = '_blank'; link.rel = 'noopener noreferrer';
  source.append(link);
  source.append(document.createTextNode(info.fetchedAt ? ' · Index v' + info.indexVersion + ' · Updated ' + new Date(info.fetchedAt).toLocaleDateString() : ' · Scores unavailable'));
  if (info.status === 'stale') source.append(document.createTextNode(' · Refresh unavailable; showing saved scores'));
  const guide = document.querySelector('.benchmark-guide > div');
  guide.replaceChildren();
  guide.append(el('p', null, info.tier === 'free' ? 'Free access includes Intelligence, Coding and Agentic indexes. Individual benchmark scores, including hallucinations, require higher AA access.' : info.status === 'unconfigured' ? 'AA scores are not configured on this installation.' : info.status === 'unavailable' ? 'Could not retrieve AA scores. Agent browsing remains available; scores will retry later.' : 'Available scores are shown for each tested model and reasoning setting. Missing scores are not zero.'));
  guide.append(el('p', null, 'Sort by orders scored entries first, then entries without a score. Group by stays independent. Default ordering keeps the model families and descending reasoning effort. Scores refresh daily.'));
  const definitions = el('dl');
  for (const metric of info.metrics) {
    definitions.append(el('dt', null, metric.label + ' · ' + metric.unit + ' · ' + (metric.direction === 'asc' ? 'lower is better' : 'higher is better')));
    definitions.append(el('dd', null, metric.description));
  }
  guide.append(definitions);
}

function renderRoster(data) {
  const host = document.querySelector('#roster');
  const editors = new Map([...host.querySelectorAll('.member[data-agent-name]')]
    .map(card => [card.dataset.agentName, card.querySelector('form')]).filter(([, form]) => form));
  host.innerHTML = '';
  const groups = browsingGroups(data.agents, '#roster-section');
  document.querySelector('#lede').textContent = data.agents.length === 0
    ? 'Add agents for your lead and PM to consult by name.'
    : 'Choose the scope and capabilities available to your lead and PM.';

  if (data.agents.length === 0) {
    host.appendChild(el('p', 'empty', 'No agents yet. Add one from Agent library, or define your own below.'));
    return;
  }
  if (groups.length === 0) { host.append(el('p', 'empty', 'No agents match these filters.')); return; }
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
  for (const group of groups) {
    const section = groupSection(group);
    host.append(section);
    for (const p of group.rows) {
    const card = el('div', 'member');
    card.dataset.agentName = p.name;
    const head = el('div', 'member__head');
    head.append(el('span', 'callsign', p.name));
    head.append(el('span', 'tag', group.modelGroup ? (HARNESS_LABELS[p.harness] || p.harness) : p.profile.modelLabel + ' · ' + (HARNESS_LABELS[p.harness] || p.harness) + ' · ' + effortLabel(effortValue(p))));
    head.append(el('span', 'spacer'));
    const edit = el('button', null, 'Edit');
    edit.onclick = () => openEditor(card, p);
    head.append(edit);
    head.append(removeButton(p, 'Remove'));
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
    appendProfile(card, p, ['workTier', 'categories', 'goodFor', 'routeLabel', 'benchmarks'].filter(field => !group.shared.includes(field)));
    if (p.tags?.length) card.append(el('p', 'member__tags', 'Tags: ' + p.tags.join(', ')));
    if (p.description) card.append(el('p', 'member__desc', p.description));
    else card.append(el('p', 'member__desc', p.harness + ' agents are not run by this tool — it leaves them alone.'));
    if (editors.has(p.name)) card.append(editors.get(p.name));
    section.append(card);
    }
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
    ['tags', (agent.tags ?? []).join(', '), 'tags: what it is good for, comma-separated'],
  ];
  for (const [name, value, placeholder] of fields.filter(([name]) => agent.harness !== 'image' || name === 'description')) {
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
    if (entries.tags !== undefined) entries.tags = tagList(entries.tags);
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

const pendingAdds = new Set();
function catalogMatches(entry, agents) {
  return agents.filter(p => p.preset === entry.preset || (!p.preset && p.name === entry.name &&
    p.harness === entry.harness && p.model === entry.model && (p.effort || '') === (entry.effort || '')));
}
function catalogState(entry, agents) {
  if (pendingAdds.has(entry.preset)) return 'Adding…';
  if (catalogMatches(entry, agents).length > 0) return 'Already added';
  return agents.some(p => p.name === entry.name) ? 'Name in use' : 'Add';
}
function renderCatalog(data) {
  const host = document.querySelector('#catalog');
  host.innerHTML = '';
  const entries = Object.entries(data.catalog).flatMap(([harness, entries]) => entries.map(p => ({ ...p, harness })));
  const groups = browsingGroups(entries, '#catalog-section');
  for (const group of groups) {
    const section = groupSection(group);
    for (const entry of group.rows) {
      const row = el('div', 'offer');
      row.append(el('span', 'offer__name', entry.name));
      const what = el('div', 'offer__what');
      what.append(el('span', 'offer__model', group.modelGroup ? (HARNESS_LABELS[entry.harness] || entry.harness) : entry.profile.modelLabel + ' · ' + (HARNESS_LABELS[entry.harness] || entry.harness) + ' · ' + effortLabel(effortValue(entry))));
      appendProfile(what, entry, ['workTier', 'categories', 'goodFor', 'routeLabel', 'benchmarks'].filter(field => !group.shared.includes(field)));
      row.append(what);
      const state = catalogState(entry, data.agents);
      const add = el('button', null, state);
      add.disabled = state !== 'Add';
      add.onclick = async () => {
        if (pendingAdds.has(entry.preset)) return;
        pendingAdds.add(entry.preset);
        add.disabled = true;
        add.textContent = 'Adding…';
        const status = document.querySelector('#roster-note');
        status.textContent = '';
        try {
          const response = await fetch('/api/agents', {
            method: 'POST', headers,
            body: JSON.stringify({ name: entry.name, harness: entry.harness, model: entry.model,
              ...(entry.effort ? { effort: entry.effort } : {}), description: entry.description, preset: entry.preset }),
          });
          const result = await response.json();
          if (!response.ok) throw new Error(result.error || 'Could not add agent');
        } catch (error) {
          status.textContent = error.message || 'Could not add agent';
        } finally {
          pendingAdds.delete(entry.preset);
          try { await load(); } catch {
            renderLists();
            if (!status.textContent) status.textContent = 'Could not refresh agents. Reopen Agents to try again.';
          }
        }
      };
      const actions = el('div', 'offer__actions');
      actions.append(add);
      const matches = catalogMatches(entry, data.agents);
      for (const agent of matches) {
        actions.append(removeButton(agent, matches.length === 1 && agent.name === entry.name ? 'Remove' : 'Remove ' + agent.name));
      }
      row.append(actions);
      section.append(row);
    }
    host.append(section);
  }
  if (groups.length === 0) host.append(el('p', 'empty', 'No ready-made agents match these filters.'));
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

async function post(path, body, note) {
  const status = document.querySelector('#roster-note');
  status.textContent = note;
  const res = await fetch(path, { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  if (!res.ok) { status.textContent = data.error; return; }
  status.textContent = data.applied.length === 0
    ? 'already up to date'
    : data.applied.map((a) => a.name + ': ' + a.changes
        .map((c) => c.field + ' → ' + (c.to ?? '-')).join(', ')).join(' · ');
  load();
}

function renderLists() { if (LAST !== null) (LIBRARY ? renderCatalog : renderRoster)(LAST); }
for (const [id, render] of [['#roster-section', renderRoster], ['#catalog-section', renderCatalog]]) {
  const filters = document.querySelector(id + ' .filters');
  if (!filters) continue;
  const refresh = () => { if (LAST !== null) render(LAST); };
  filters.querySelector('input').addEventListener('input', refresh);
  for (const select of filters.querySelectorAll('select')) select.addEventListener('change', refresh);
  filters.querySelector('button').onclick = () => {
    filters.querySelector('input').value = '';
    filters.querySelector('[name=category]').value = 'all';
    filters.querySelector('[name=tier]').value = 'all';
    filters.querySelector('[name=group]').value = 'model-reasoning';
    filters.querySelector('[name=sort]').value = 'default';
    refresh();
  };
}

// The last roster payload, so filtering the catalog re-renders without a fetch.
let LAST = null;

async function load() {
  const response = await fetch('/api/agents', { headers });
  if (!response.ok) throw new Error('Could not refresh agents. Reopen this screen to try again.');
  LAST = await response.json();
  renderBenchmarkControls(LAST);
  renderLists();
  if (!LIBRARY) renderForm(LAST);
}

if (!LIBRARY) document.querySelector('#add').onsubmit = async (event) => {
  event.preventDefault();
  const form = new FormData(event.target);
  const body = Object.fromEntries([...form.entries()].filter(([, v]) => v !== ''));
  if (body.tags !== undefined) body.tags = tagList(body.tags);
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
