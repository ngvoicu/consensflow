import { spawn } from 'node:child_process'
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto'
import { createServer } from 'node:http'
import {
  addParticipant,
  editParticipant,
  listParticipants,
  PERMISSIONS,
  RUNTIMES,
  removeParticipant,
} from './roster.js'
import { refreshInstalledSkill } from './sync.js'

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
          participants: listParticipants(env),
          runtimes: RUNTIMES,
          permissions: PERMISSIONS,
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/participants') {
        const added = addParticipant(JSON.parse(await readBody(request)), env)
        refreshInstalledSkill(env)
        return send(201, { participant: added })
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
<title>ConsensFlow</title>
<style>
  :root { color-scheme: light dark; font-family: ui-sans-serif, system-ui; }
  body { margin: 2rem auto; max-width: 720px; padding: 0 1rem; }
  h1 { font-size: 1.2rem; }
  table { width: 100%; border-collapse: collapse; margin: 1rem 0; }
  td, th { text-align: left; padding: .4rem .5rem; border-bottom: 1px solid color-mix(in srgb, currentColor 20%, transparent); }
  form { display: grid; grid-template-columns: repeat(2, 1fr); gap: .5rem; margin-top: 1rem; }
  input, select, button { font: inherit; padding: .35rem .5rem; }
  button { cursor: pointer; }
  .full { grid-column: 1 / -1; }
  .error { color: #d33; }
  .muted { opacity: .65; font-size: .85em; }
</style>
</head>
<body>
<h1>ConsensFlow participants</h1>
<p class="muted">Every change regenerates the skill installed into your coding agents.</p>
<table id="roster"><thead><tr><th>Name</th><th>Runtime</th><th>Model</th><th>Effort</th><th>Permission</th><th></th></tr></thead><tbody></tbody></table>
<form id="add">
  <input name="name" placeholder="name (lowercase)" required>
  <select name="runtime"></select>
  <input name="model" placeholder="model (verbatim)" required class="full">
  <input name="effort" placeholder="effort (optional)">
  <select name="permission"></select>
  <button class="full">Add participant</button>
  <p id="error" class="error full"></p>
</form>
<script>
const TOKEN = ${JSON.stringify(token)};
const headers = { authorization: 'Bearer ' + TOKEN, 'content-type': 'application/json' };
async function load() {
  const res = await fetch('/api/participants', { headers });
  const data = await res.json();
  const tbody = document.querySelector('#roster tbody');
  tbody.innerHTML = '';
  for (const p of data.participants) {
    const tr = document.createElement('tr');
    for (const key of ['name', 'runtime', 'model', 'effort', 'permission']) {
      const td = document.createElement('td');
      td.textContent = p[key] ?? '—';
      tr.appendChild(td);
    }
    const td = document.createElement('td');
    const btn = document.createElement('button');
    btn.textContent = 'Remove';
    btn.onclick = async () => {
      await fetch('/api/participants/' + p.name, { method: 'DELETE', headers });
      load();
    };
    td.appendChild(btn);
    tr.appendChild(td);
    tbody.appendChild(tr);
  }
  const runtimeSelect = document.querySelector('select[name=runtime]');
  const permissionSelect = document.querySelector('select[name=permission]');
  if (runtimeSelect.options.length === 0) {
    for (const r of data.runtimes) runtimeSelect.add(new Option(r, r));
    for (const p of data.permissions) permissionSelect.add(new Option(p, p));
  }
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
