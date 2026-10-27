// Copies the installers to the site, which serves them as
// https://consensflow.ngvoicu.dev/install.sh and /install.ps1
// (npm run site:installers -- <site folder>, by default ../sites/consensflow-site
// beside this repository's folder):
//
//   node install/to-site.mjs [SITE]
//
// The site's install section shows the update key the installers pin, for a
// person to compare with the scripts: the copy is refused while the page shows
// another, or none.

import { copyFileSync, existsSync, readFileSync } from 'node:fs'
import path from 'node:path'

import { INSTALL_PS1, INSTALL_SH, ROOT, scriptKeyLine, updaterKeyLine } from './kit.mjs'

const site = path.resolve(process.argv[2] ?? path.join(ROOT, '..', 'sites', 'consensflow-site'))
const page = path.join(site, 'index.html')
if (!existsSync(page)) {
  console.error(`to-site: ${site} holds no index.html: name the site's folder`)
  process.exit(2)
}
const key = scriptKeyLine(readFileSync(INSTALL_SH, 'utf8'))
if (scriptKeyLine(readFileSync(INSTALL_PS1, 'utf8')) !== key || updaterKeyLine() !== key) {
  console.error("to-site: install.sh, install.ps1 and the app's updater do not hold one key")
  process.exit(1)
}
if (!readFileSync(page, 'utf8').includes(key)) {
  console.error(`to-site: ${page} does not show the key the installers pin, ${key}`)
  process.exit(1)
}
for (const script of [INSTALL_SH, INSTALL_PS1]) {
  copyFileSync(script, path.join(site, path.basename(script)))
  console.log(`to-site: ${path.basename(script)} -> ${path.join(site, path.basename(script))}`)
}
