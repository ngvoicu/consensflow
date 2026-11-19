import { appendFileSync, existsSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { PowerShellStartLock } from './powershell-start-lock.mjs'

const [work, name] = process.argv.slice(2)
const lock = new PowerShellStartLock()
writeFileSync(path.join(work, name), '')
const until = Date.now() + 5000
while (!existsSync(path.join(work, 'go'))) {
  if (Date.now() >= until) throw new Error('the test did not open its gate')
  await new Promise((resolve) => setTimeout(resolve, 10))
}
await lock.start(() => {
  appendFileSync(path.join(work, 'events'), `start ${name}\n`)
  const ready = new Promise((resolve) =>
    setTimeout(() => {
      appendFileSync(path.join(work, 'events'), `ready ${name}\n`)
      resolve()
    }, 400),
  )
  return { ready, value: undefined }
})
