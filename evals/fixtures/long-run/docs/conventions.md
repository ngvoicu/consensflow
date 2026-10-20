# How ledgerline is written

## Money

- An amount is whole cents, an integer, everywhere between reading it and
  showing it. No module does arithmetic on a float amount.
- A fraction of a cent is made whole in one place, `money.round`, and a half
  rounds to the even neighbour: 0.5 is 0, 1.5 is 2, 2.5 is 2, -2.5 is -2. Over a
  year of conversions, rounding half up drifts the totals upward; half to even
  does not drift.
- A share is in hundredths of a percent, an integer: 9050 is 90.50%.
- A rate is in ten-thousandths, an integer: 10900 is 1.09.
- An amount is shown with its thousands grouped by commas, two decimals, and its
  currency after it: `1,234.50 EUR`.

## Dates

- A date is text, `YYYY-MM-DD`, and must be a date of the calendar: 2025-02-30
  is none.
- A month is `YYYY-MM`.

## Code

- ES modules, Node 20 or later, no dependencies.
- A module says what it is for in a comment at its top, and each exported
  function says what it returns in a line above it.
- Errors in the data are said, with the line of the file they are on, and the
  entry is set aside: one bad line never stops a build.
- A function that reads the disk is in `index.mjs` or in a script, never in the
  library.

## Tests

- Each module has a test file of the same name in `test/`.
- A fix comes with a test that fails before it and passes after it.
- Each case waits a moment first (`await pace()`): the suite is slow on purpose,
  and a case that does not wait makes the numbers of a run mean less.

## Budget alerts

(To be written in phase 2: the levels, and how a share on the line rounds.)
