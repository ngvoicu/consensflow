# Known bugs

## 1. Amounts round half up, not half to even

`round()` in `src/money.mjs` rounds a half cent up, always. The conventions say
a half rounds to the even neighbour (banker's rounding), so that the rounding of
many conversions does not drift one way: 2.5 cents is 2, 3.5 cents is 4, -2.5 is
-2. A conversion of 12.50 EUR into USD (at 1.09) is 1362 cents, not 1363 as it is
now. Both callers of `round()` are affected (`currency.convert` and
`money.share`), and no test covers a half.

## 2. A quoted field with a comma in it is split in two

`parse()` in `src/csv.mjs` splits each line on every comma. A memo written in
quotes, as the bank's export writes one (`"Office, supplies"`), is split into
two fields, and every field after it moves one column: the entry's amount is
then its memo's second half, and the entry is dropped as invalid. Seven entries
of `data/journal.csv` are lost this way; the build says which it skipped.

## 3. A refund adds to a month's total

`total()` in `src/report.mjs` adds the size of each amount, so a refund (a
negative amount on an expense account) makes a month's spending larger instead of
smaller. March's office supplies (account 6100) come to 452.40 EUR; the report
adds them up to 512.40 EUR, whatever currency it shows them in.
