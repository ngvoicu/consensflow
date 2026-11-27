#!/bin/sh
# Installs one published Pi into a folder of its own, for `drive.mjs`: the coding agent and every
# package of its family at the same version (npm's carets would otherwise take the newest
# pi-agent-core and pi-ai, whose loop is not the version's), with no install scripts. npm checks
# each package against the registry's integrity. Nothing of the person's Pi is read or written.
#   sh tests/pi-loop/get-pi.sh 1.6.1 /some/fresh/folder
set -eu
version=$1
folder=$2
mkdir "$folder"
cd "$folder"
node -e '
const v = process.argv[1]
const overrides = {}
for (const name of ["chord", "pi-agent-core", "pi-ai", "pi-codemode", "pi-mcp", "pi-schema", "pi-telemetry", "pi-tui"])
  overrides["@earendil-works/" + name] = v
require("fs").writeFileSync("package.json", JSON.stringify({ name: "pi-loop", private: true, overrides }))
' "$version"
npm install --ignore-scripts --no-audit --no-fund "@earendil-works/pi-coding-agent@$version"
