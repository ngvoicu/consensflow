// Copies the installers to the site, which serves them as
// https://consensflow.ngvoicu.dev/install.sh and /install.ps1
// (npm run site:installers -- <site folder>, by default ../sites/consensflow-site
// beside this repository's folder):
//
//   node install/to-site.mjs [--check] [SITE]
//
// The site's install section shows the update key the installers pin, for a
// person to compare with the scripts: the copy is refused while the page shows
// another, or none.
//
// With --check nothing is copied: it says whether the site holds this
// repository's two installers byte for byte, and refuses (1) when one is
// missing or differs, which is what a release finds before it is published
// (the site's branch once held an installer older than the repository's: T-840).
// The gate runs it, on the main checkout, once the site holds the installers.

import { copyFileSync, existsSync, readFileSync } from 'node:fs'
import path from 'node:path'

import { INSTALL_PS1, INSTALL_SH, ROOT, scriptKeyLine, updaterKeyLine } from './kit.mjs'

const words = process.argv.slice(2)
const check = words.includes('--check')
const site = path.resolve(
  words.find((word) => word !== '--check') ?? path.join(ROOT, '..', 'sites', 'consensflow-site'),
)
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
if (check) {
  const stale = [INSTALL_SH, INSTALL_PS1].flatMap((script) => {
    const name = path.basename(script)
    const held = path.join(site, name)
    if (!existsSync(held)) return [`${name} is missing from the site`]
    return readFileSync(held).equals(readFileSync(script))
      ? []
      : [`${name} differs from this repository's`]
  })
  if (stale.length > 0) {
    console.error(
      `to-site: ${stale.join('; ')}: run npm run site:installers -- ${site}, and commit it on the site's branch`,
    )
    process.exit(1)
  }
  console.log(`to-site: the site holds this repository's installers (${site})`)
} else {
  for (const script of [INSTALL_SH, INSTALL_PS1]) {
    copyFileSync(script, path.join(site, path.basename(script)))
    console.log(`to-site: ${path.basename(script)} -> ${path.join(site, path.basename(script))}`)
  }
}
