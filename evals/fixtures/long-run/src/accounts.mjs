/** The kinds an account is of. */
export const KINDS = ['asset', 'income', 'expense']

/** The chart of accounts its rows make: each account by its code. */
export function loadAccounts(rows) {
  const accounts = new Map()
  for (const row of rows) {
    if (!/^\d{4}$/.test(row.code)) throw new Error(`an account's code is four digits, not ${row.code}`)
    if (!KINDS.includes(row.kind)) throw new Error(`account ${row.code} is of no kind: ${row.kind}`)
    accounts.set(row.code, { code: row.code, name: row.name, kind: row.kind })
  }
  return accounts
}

/** The accounts of `kind`, in the order of their codes. */
export function accountsOf(accounts, kind) {
  return [...accounts.values()]
    .filter((account) => account.kind === kind)
    .sort((a, b) => a.code.localeCompare(b.code))
}
