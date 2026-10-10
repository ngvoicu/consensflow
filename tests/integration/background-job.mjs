import { writeFileSync } from 'node:fs'

/**
 * The job an agent started and left running when its turn ended: a stand-in for
 * the recording run under a lock tool (`lock.py`) of the report of 2026-10-09,
 * which writes RELEASED on a normal end and on INT, TERM or HUP, and nothing
 * when it is killed outright. `background-job.mjs <seconds> <file>` runs that
 * many seconds and writes `<file>.finished`; a signal that asks it to end (INT,
 * TERM, HUP, and Break where Windows has it) makes it write `<file>.released` and
 * go. `<file>.pid` is its process, for whoever asks whether it is still there.
 * With `--deaf` it ignores the signals and is only ever killed.
 */

const [seconds, file, deaf] = process.argv.slice(2)
writeFileSync(`${file}.pid`, String(process.pid))

const release = () => {
  writeFileSync(`${file}.released`, 'RELEASED\n')
  process.exit(0)
}
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGBREAK']) {
  process.on(signal, deaf === '--deaf' ? () => {} : release)
}

setTimeout(() => {
  writeFileSync(`${file}.finished`, 'FINISHED\n')
  process.exit(0)
}, Number(seconds) * 1000)
