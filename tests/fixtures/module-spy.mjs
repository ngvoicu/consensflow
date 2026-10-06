import { appendFileSync } from 'node:fs'
import { registerHooks } from 'node:module'

/**
 * A preload for a test that asks whether Node's own CLI ran
 * (`NODE_OPTIONS=--import=<this file>`): every load of `src/cli.js`, which is
 * Node's CLI, and of `src/use-node.js`, which is the decision `bin/cf.mjs` asks,
 * is a line in the file `CF_TEST_SPY` names. `bin/cf.mjs` that forwarded a
 * command loads the second and never the first, and one that ran it loads both;
 * a native program loads neither, and says nothing.
 */
if (process.env.CF_TEST_SPY) {
  registerHooks({
    load(url, context, nextLoad) {
      const loaded = /\/src\/(cli|use-node)\.js$/.exec(url)
      if (loaded) appendFileSync(process.env.CF_TEST_SPY, `${loaded[1]}\n`)
      return nextLoad(url, context)
    },
  })
}
