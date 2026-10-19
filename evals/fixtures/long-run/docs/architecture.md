# How ledgerline is built

ledgerline keeps the books of a small consultancy: the bank and the cash box,
two kinds of income (consulting, and the seats of the workshops it runs), and
six kinds of spending. It has no database and no dependencies. The books are
three CSV files in `data/`; the library in `src/` reads them, works out what the
reports need, and the scripts in `scripts/` build a static site from it, serve
it, and export what the accountant asks for.

## The data

- `data/accounts.csv` is the chart of accounts: a four-digit code, a name, and a
  kind (`asset`, `income` or `expense`). The codes follow the usual plan: 1xxx
  are what the business holds, 4xxx what it earns, 6xxx what it spends.
- `data/journal.csv` is every movement of the year, a line each: its date, the
  account it is on, a memo, the amount, the currency it was paid in, and tags
  (`#travel`, `#client:acme`). An amount is positive for what an expense account
  spent or an income account earned, and negative for a refund of either. The
  bank's export writes a memo in quotes when it holds a comma.
- `data/budgets.csv` is what each expense account may spend in a month, in
  euros.

The books are single-entry on purpose: the accountant keeps the double entry in
their own system, and asked only for what the business owner reads.

## The library

Each module does one thing, and the arrows go one way: `money` and `dates` know
nothing of the others, `csv` knows nothing of money, and `index` is the only
module that touches the disk.

- `money.mjs`: amounts are whole cents, as integers, from the moment they are
  read (`toCents`) to the moment they are shown (`format.mjs`). A float never
  holds an amount. `round` is the one place a fraction of a cent is made whole:
  a conversion, a share.
- `currency.mjs`: the rates against the euro, in ten-thousandths, and
  `convert`, which is a product and a quotient of whole numbers and a `round`.
- `format.mjs`: what a reader sees of an amount and of a share.
- `dates.mjs`: dates are text, `YYYY-MM-DD`, checked against the calendar; a
  month is `YYYY-MM`.
- `csv.mjs`: the files' format, both ways.
- `accounts.mjs`, `tags.mjs`, `validate.mjs`, `journal.mjs`: the rows made into
  accounts and entries, and the rows that make none set aside with why.
- `ledger.mjs`: balances, by account and by month, in a currency.
- `budget.mjs`: what each budgeted account spent of its budget.
- `report.mjs`: what a page of the site shows of a month and of an account.
- `search.mjs`: the entries a query finds.
- `export.mjs`: the journal written back as CSV.
- `html.mjs`: the pieces of a page.
- `config.mjs`: the currency the reports show, the year, and the folders.
- `index.mjs`: `load(folder)`, the books as the scripts use them.

## The scripts

- `scripts/longrun-build.mjs` (`npm run build`) writes `dist/`: a page per
  month, a page per account, and an index, each made in turn. It is slow on
  purpose (`scripts/pace.mjs`): the real site renders a chart for each page,
  which takes seconds, and the pace stands in for that work so that the build
  takes as long as the real one.
- `scripts/longrun-dev-server.mjs` (`npm run dev`) serves `dist/` and a small
  JSON API, and never exits: whoever starts it stops it.
- `scripts/export.mjs` (`npm run export`) writes what the accountant asks for
  into `exports/`.

## The tests

`test/` has a file for each module. Each case waits a moment first
(`scripts/pace.mjs`), as the real project's cases wait for its database, so that
the whole suite takes minutes; `LONGRUN_PACE_MS=0` runs it in seconds for a
quick look. The cases run one file at a time (`--test-concurrency=1`): the real
suite shares a database between its files.
