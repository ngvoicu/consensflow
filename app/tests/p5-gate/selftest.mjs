// The gate's own test: it must fail where it is meant to (T-2284's findings 10 and 13 to 15), each
// with its own cause, and pass where nothing is wrong. Each control runs the gate (gate.mjs) on the
// one case it concerns, with a fault put into what the host or the page hands over
// (CF_GATE_FAULT), and states the exit code and the words of the cause it must give. A gate that
// hangs fails here too: every run has a limit.
//   node selftest.mjs <webkit|chromium>
import { spawn } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const browser = process.argv[2] ?? 'chromium'
const gate = resolve(dirname(fileURLToPath(import.meta.url)), 'gate.mjs')
const WAIT = 20
const controls = [
  { name: 'finding 10, clean', only: 'finding 10', exit: 0 },
  {
    name: 'finding 10, a history that starts at its boundary',
    only: 'finding 10',
    fault: 'empty-frame',
    exit: 1,
    cause: /history starts at \d+, past/,
  },
  {
    name: 'finding 10, a history and boundary one byte short of what the page before was handed',
    only: 'finding 10',
    fault: 'short-boundary',
    exit: 1,
    cause: /retained bytes|boundary/,
  },
  { name: 'finding 14, clean', only: 'finding 14', exit: 0 },
  {
    name: 'finding 14, a private cursor report for an ordinary enquiry',
    only: 'finding 14',
    fault: 'private-cpr',
    exit: 1,
    cause: /answers: expected|cursor report/,
  },
  {
    name: 'finding 14, a cursor report for another place than the program set',
    only: 'finding 14',
    fault: 'wrong-cpr-place',
    exit: 1,
    cause: /answers/,
  },
  { name: 'lane, clean', only: 'a lane that leaves', exit: 0 },
  {
    name: 'finding 13, every acknowledgement of the retired lane lost',
    only: 'a lane that leaves',
    fault: 'no-retired-ack',
    exit: 1,
    cause: /stopped at \d+ of \d+ bytes/,
  },
  {
    name: 'finding 15, a docked reply for another generation',
    only: 'a lane that leaves',
    fault: 'wrong-reply-generation',
    exit: 1,
    cause: /refused|received/,
  },
  {
    name: "finding 15, the retired lane's acknowledgements for another generation",
    only: 'a lane that leaves',
    fault: 'wrong-last-ack',
    exit: 1,
    cause: /acknowledgement of .* refused/,
  },
]
let failed = 0
for (const control of controls) {
  const started = Date.now()
  const output = await new Promise((done) => {
    const child = spawn('node', [gate, browser, '0', '1', '0'], {
      env: {
        ...process.env,
        CF_GATE_ONLY: control.only,
        CF_GATE_FAULT: control.fault ?? '',
        CF_GATE_WAIT_S: String(WAIT),
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let text = ''
    child.stdout.on('data', (d) => (text += d))
    child.stderr.on('data', (d) => (text += d))
    const limit = setTimeout(
      () => {
        text += '\nSELFTEST: the gate was still running after its limit\n'
        child.kill('SIGKILL')
      },
      (4 * WAIT + 60) * 1000,
    )
    child.on('exit', (code, signal) => {
      clearTimeout(limit)
      done({ code: signal ? 'killed' : code, text })
    })
  })
  const seconds = Math.round((Date.now() - started) / 1000)
  const problems = []
  if (output.code !== control.exit) problems.push(`exit ${output.code}, wanted ${control.exit}`)
  if (control.cause && !control.cause.test(output.text))
    problems.push(`no cause like ${control.cause}`)
  if (problems.length) {
    failed++
    console.log(
      `FAIL ${control.name} (${seconds} s): ${problems.join('; ')}\n${output.text.split('\n').slice(-6).join('\n')}`,
    )
  } else console.log(`ok   ${control.name} (${seconds} s)`)
}
console.log(`${controls.length - failed} of ${controls.length} controls held, ${browser}`)
process.exitCode = failed ? 1 : 0
