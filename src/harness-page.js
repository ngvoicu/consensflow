/** Standalone harness diagnostics, served by the authenticated app editor. */
export const harnessPage = (token) => `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ConsensFlow Harnesses</title>
<style>
  :root { color-scheme: dark light; --ink: #0C1E23; --panel: #12262C; --line: #1E3A42; --foam: #E9F1EF; --muted: #8FA9AF; --accent: #63C7B2; }
  @media (prefers-color-scheme: light) {
    :root { --ink: #E9F1EF; --panel: #FFFFFF; --line: #C9DAD8; --foam: #0C1E23; --muted: #52717A; --accent: #16766A; }
  }
  * { box-sizing: border-box; }
  body { margin: 0; padding: 40px 24px 64px; background: var(--ink); color: var(--foam); font: 15px/1.5 Archivo, "Helvetica Neue", system-ui, sans-serif; -webkit-font-smoothing: antialiased; }
  main { max-width: 760px; margin: 0 auto; }
  .mark { font: 12px ui-monospace, monospace; letter-spacing: .14em; text-transform: uppercase; color: var(--accent); }
  h1 { font-size: 26px; font-weight: 600; margin: 6px 0 4px; }
  .lede, #check-note { color: var(--muted); font-size: 13.5px; }
  #check-note { min-height: 20px; }
  .host { padding: 18px 0; border-top: 1px solid var(--line); overflow-wrap: anywhere; }
  .host strong { display: block; font-size: 17px; color: var(--accent); margin-bottom: 6px; }
  .host div { font-size: 13px; }
  .host small { display: block; color: var(--muted); margin-top: 8px; }
  button { font: inherit; font-size: 13px; color: var(--foam); background: var(--panel); border: 1px solid var(--line); border-radius: 4px; padding: 6px 12px; cursor: pointer; }
  button:hover { border-color: var(--accent); }
  button:disabled { opacity: .6; cursor: wait; }
  .host button { margin: 10px 12px 0 0; }
  :focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
</style>
</head>
<body>
<main>
  <p class="mark">ConsensFlow</p>
  <h1>Harnesses</h1>
  <p class="lede">Installed coding tools and available updates.</p>
  <button id="check-all" type="button">Check all harnesses</button>
  <p id="check-note" role="status"></p>
  <section id="harnesses" aria-label="Harness diagnostics"></section>
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
let HARNESS_ROWS = [];
async function checkHarnesses(id = null, button = null) {
  const note = document.querySelector('#check-note');
  note.textContent = 'Checking…';
  if (button) button.disabled = true;
  try {
    const response = await fetch('/api/harnesses/check', { method: 'POST', headers, body: JSON.stringify({ ...(id ? { id } : {}), refresh: button !== null }) });
    if (!response.ok) throw new Error('Harness check failed');
    const { harnesses } = await response.json();
    HARNESS_ROWS = id ? HARNESS_ROWS.map(row => row.id === id ? harnesses[0] : row) : harnesses;
    renderHarnesses();
    note.textContent = '';
  } catch (error) {
    note.textContent = error.message + ' — try checking again.';
  } finally { if (button) button.disabled = false; }
}

function renderHarness(row) {
  const line = el('div', 'host');
  line.append(el('strong', null, row.id + (row.lead ? '' : ' (worker only)')));
  line.append(el('div', null, row.installed ? 'Installed: ' + row.path : 'Not installed'));
  if (row.installed) {
    line.append(el('div', null, 'Version: ' + (row.version.value || row.version.reason || row.version.state)));
    const update = row.update;
    const text = update.state === 'available' ? 'New release: ' + update.value : update.state === 'current' ? 'Up to date' : update.reason || 'Update availability not verified';
    line.append(el('div', null, text));
    if (update.note) line.append(el('small', null, update.note));
    if (row.extension?.state === 'error' || row.extension?.state === 'not-installed') {
      const label = row.id === 'pi' ? 'Pi' : 'OpenCode';
      const status = el('div', null, label + ' setup failed: ' + (row.extension.reason || 'Setup is missing.'));
      status.style.color = '#f47769';
      line.append(status);
      const retry = el('button', null, 'Retry ' + label + ' setup');
      retry.onclick = () => checkHarnesses(row.id, retry);
      line.append(retry);
    }
  }
  const check = el('button', null, 'Check again');
  check.onclick = () => checkHarnesses(row.id, check); line.append(check);
  line.append(el('small', null, 'Checked: ' + new Date(row.checkedAt).toLocaleString()));
  return line;
}


function renderHarnesses() {
  document.querySelector('#harnesses').replaceChildren(...HARNESS_ROWS.map(renderHarness));
}
const checkAll = document.querySelector('#check-all');
checkAll.onclick = () => checkHarnesses(null, checkAll);
checkHarnesses();
</script>
</body>
</html>
`
