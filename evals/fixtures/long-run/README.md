# ledgerline

A small bookkeeping library: a chart of accounts, a journal of entries kept in
CSV, balances by account and by month, monthly budgets, and a report site built
from all of it. No dependencies: Node 20 or later is all it needs.

```sh
npm test          # the whole test suite: a few minutes, on purpose (see below)
npm run build     # the report site, into dist/: a few minutes too
npm run dev       # serves dist/ and a small API on http://127.0.0.1:4173 (never exits)
npm run export -- --csv    # the journal as CSV, into exports/
```

The suite and the build are slow on purpose: each test case and each page waits
a moment (`scripts/pace.mjs`), as a real project's integration tests and a real
site's image work do. `LONGRUN_PACE_MS=0` makes them fast, for a quick look; the
numbers a phase reports are those of a full run, with the pace as it is.

## Layout

- `src/` is the library: `money.mjs` (amounts in whole cents), `currency.mjs`,
  `format.mjs`, `dates.mjs`, `csv.mjs`, `accounts.mjs`, `journal.mjs`,
  `validate.mjs`, `ledger.mjs`, `budget.mjs`, `report.mjs`, `tags.mjs`,
  `search.mjs`, `export.mjs`, `html.mjs`, `config.mjs`, and `index.mjs`, which
  loads the data.
- `data/` is the books of 2025: `accounts.csv`, `journal.csv`, `budgets.csv`.
- `scripts/` builds the site, serves it, exports, and paces the slow work.
- `test/` is the suite, one file to a module.
- `docs/` says how it is built (`architecture.md`), how it is written
  (`conventions.md`), what is known to be wrong (`bugs.md`), what comes next
  (`requirements.md`) and how it got here (`history.md`).
