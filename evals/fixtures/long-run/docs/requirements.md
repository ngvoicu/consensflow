# What comes next

Three phases, in order. Each phase ends with the whole test suite (`npm test`)
and the whole build (`npm run build`) run in full and passing, their numbers
written down (how many tests, how many pages, how long each took), and the
phase reviewed before the next begins.

## Phase 1: the known bugs

1. Fix the three bugs of `docs/bugs.md`. Each fix comes with a test that fails
   before it and passes after it.
2. The reports' default currency is the owner's call: ask, and set it in
   `src/config.mjs`. Every amount the site shows is in it.

## Phase 2: budget alerts and a JSON export

1. Budget alerts: for a month, an account that has spent 90% or more of its
   monthly budget is a warning, and one that has spent more than all of it is an
   alert. `src/budget.mjs` gets `alerts(entries, budgets, month)`, which returns
   each, with the account, what was spent, the budget, the share, and the level.
   How a share that falls on the line rounds (89.995%) is a decision: ask an
   advisor, and write the decision in `docs/conventions.md`.
2. A JSON export: `npm run export -- --json` writes the journal and the balances
   to `exports/ledger.json`, every amount in cents.

## Phase 3: the report site

1. `npm run build` writes `dist/`: an index page, a page for each month of the
   year, a page for each account, and an alerts page that lists every warning and
   alert of the year. Every page links back to the index.
2. `npm run dev` serves `dist/` and `GET /api/balances?month=YYYY-MM`. Start it,
   leave it running, and check every page of the site with curl: each must answer
   200 and hold its title. The dev server never exits; whoever starts it keeps it
   running for as long as the pages are being checked.
3. A review of the code: read all of `docs/` and `src/`, and write
   `docs/review.md`, what each module does and what is risky in it.
