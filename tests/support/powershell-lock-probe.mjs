import { writeFileSync } from 'node:fs'
import { PowerShellStartLock } from './powershell-start-lock.mjs'

const [ready, entered] = process.argv.slice(2)
writeFileSync(ready, '')
await new PowerShellStartLock().start(() => {
  writeFileSync(entered, '')
  return { ready: Promise.resolve(), value: undefined }
})
