# How ledgerline got here

## 0.1: a spreadsheet, read

The books were a spreadsheet the owner kept by hand, one tab a month. The first
version read its CSV export and printed each month's spending to the terminal.
Amounts were floats; the totals of a year were off by a few cents, and the
accountant noticed. That was the first decision: amounts are whole cents.

Decided:

- Amounts are integers of cents (the accountant's request after the first year).
- The books stay CSV: the owner edits them in the spreadsheet, and exports.
- No dependencies: the project is maintained a few hours a quarter, and a
  dependency that moves is work nobody has time for.

## 0.2: accounts and budgets

The tabs became one journal with an account column, and the chart of accounts a
file of its own. Budgets came when the owner asked why office spending was high
in a month: an account now has a monthly budget, and a page says how much of it
was spent. The share was a float percentage at first, and showed 89.99999999% on
one page; shares became hundredths of a percent, integers.

Decided:

- Single-entry books: the accountant keeps the double entry, and wants only
  what the owner reads from this.
- Codes of four digits, the usual plan: 1xxx assets, 4xxx income, 6xxx spending.
- A share is an integer of hundredths of a percent.

## 0.3: the site

The terminal output became a static site, a page per month and per account,
built into `dist/` and served on the office's network by a small server. The
real build renders a chart for each page with a headless browser, which takes a
few seconds a page; the build in this repository waits as long instead
(`scripts/pace.mjs`), so that it takes as long and behaves the same way under
whoever runs it.

The bank started exporting in CSV with quoted memos. The parser was not changed
for it; that is bug 2 of `docs/bugs.md`.

Decided:

- The site is static: no database, no login, served read-only.
- The dev server never exits: it is started for a session of work and stopped
  by whoever started it.
- Rates are integers of ten-thousandths, so that a conversion has one fraction,
  made whole in one place.

## 0.4: currencies

Some software is billed in dollars, so an entry has a currency, and the reports
show every amount converted into one. Which one is the owner's call; the
configuration says the bank's for now (`src/config.mjs`). The conversion rounds
with `money.round`, which rounds half up; the conventions say half to even, and
that is bug 1.

A refund was entered for the first time in March (a chair mat sent back), and
March's office spending went up instead of down: bug 3.

## What the owner asked for next

`docs/requirements.md`: the three bugs fixed with tests, the default currency
set, budget alerts with a decided rounding, a JSON export, the site complete
with an alerts page, served and checked page by page, and a review of the code.

## Notes from the last review (0.3)

- `report.total` is used for every number the site shows; a change to it moves
  every page. Check the month pages and the account pages after touching it.
- `budget.usage` converts the budget into the report's currency, so a budget in
  euros is compared with spending in dollars at the day's rate: the accountant
  agreed, for now.
- `search` treats `word:value` as a field only for `tag`, `account` and
  `month`; any other word with a colon is looked for in the memo as it is.
- The export writes the entries the books could read: what was skipped is said
  on the error stream and is not in the file. The accountant wants a list of the
  skipped lines too, one day.
- The dev server reads the books once, when it starts: a change to the data
  needs a restart of the server, and a change to the site a new build.
- The pace of the tests and of the build is the same variable, so a quick run of
  one is a quick run of both.
