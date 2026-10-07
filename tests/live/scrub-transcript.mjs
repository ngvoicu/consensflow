/**
 * Makes a fixture of a Claude transcript a live check kept (`npm run live:stops
 * -- --keep <folder>`): every record stays, in its order, and what is personal
 * or opaque in it is replaced. The reader of the records and the tests built
 * on them read what is left, which is each record's type, ids, time and
 * conversation.
 *
 *   node tests/live/scrub-transcript.mjs <transcript.jsonl> <fixture.jsonl>
 *
 * What goes:
 *   - the session id, which becomes `$SESSION` (a test puts its own there);
 *   - the folder the window ran in (`/work/app`), the git branch (`main`);
 *   - an account's, organization's and bridge's ids;
 *   - what an attachment carried (a hook's command and output, the skills,
 *     the environment, the instructions, the e-mail address of the user): only
 *     its type, and a hook's name, event, exit code and duration, are kept;
 *   - the request ids the API gave, and the signature of a thinking block.
 * The records a message is made of (the user's words, the assistant's, its
 * model and usage) stay as Claude wrote them.
 */
import { readFileSync, writeFileSync } from 'node:fs'

const [from, to] = process.argv.slice(2)
if (!from || !to) throw new Error('usage: scrub-transcript.mjs <transcript.jsonl> <fixture.jsonl>')

/** The text of the user's folder and branch, and what names an account. */
const FOLDER = '/work/app'
const OPAQUE = ['bridgeSessionId', 'ownerAccountUuid', 'ownerOrganizationUuid']

/** What an attachment keeps: its type, and of a hook's the facts of its run. */
function attachment(found) {
  const kept = { type: found.type }
  if (found.type === 'hook_success' || found.type === 'hook_cancelled') {
    for (const name of ['hookName', 'hookEvent', 'exitCode', 'durationMs', 'timedOut']) {
      if (name in found) kept[name] = found[name]
    }
    kept.command = '[scrubbed]'
  }
  return kept
}

function scrub(record, session) {
  const out = { ...record }
  for (const name of ['sessionId', 'session_id']) if (name in out) out[name] = '$SESSION'
  if ('cwd' in out) out.cwd = FOLDER
  if ('gitBranch' in out) out.gitBranch = 'main'
  for (const name of OPAQUE) if (name in out) out[name] = '[scrubbed]'
  if ('requestId' in out) out.requestId = 'req_scrubbed'
  if (out.attachment) {
    out.attachment = attachment(out.attachment)
    delete out.rendered
    delete out.renderedRole
    delete out.renderedBesideToolResult
  }
  if (typeof out.lastPrompt === 'string') out.lastPrompt = '[scrubbed]'
  if (typeof out.aiTitle === 'string') out.aiTitle = '[scrubbed]'
  if (Array.isArray(out.message?.content)) {
    out.message = {
      ...out.message,
      content: out.message.content.map((block) =>
        'signature' in block ? { ...block, signature: '[scrubbed]' } : block,
      ),
    }
  }
  return JSON.stringify(out).replaceAll(session, '$SESSION')
}

const lines = readFileSync(from, 'utf8').split('\n').filter(Boolean)
const session = lines
  .map((line) => JSON.parse(line).sessionId)
  .find((found) => typeof found === 'string')
if (!session) throw new Error('no record names its session')
writeFileSync(to, `${lines.map((line) => scrub(JSON.parse(line), session)).join('\n')}\n`)
process.stdout.write(`${lines.length} records of ${session} kept in ${to}\n`)
