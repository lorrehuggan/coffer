# Fixtures

Sample bank files for tests. Every file here is synthetic or fully redacted: fake
merchants, IDs, amounts, notes, account numbers and addresses. Only the structure of a
real export (headers, formats, encodings, quirks) is kept. Real exports never go here;
they belong in the gitignored `samples/` directory (`AGENTS.md` → "Personal financial data").

## `csv/`

Generic CSV shapes for the CSV importer (M1.7), written by hand:

| File | Shape |
|---|---|
| `signed-amount.csv` | UTF-8, LF. Signed amount, transaction IDs, separate time column, merchant, category, type, local amount + currency (one foreign spend), notes. Rows that must be flagged: an impossible date, an unquoted comma, an amount with too many decimals |
| `debit-credit-windows-1252.csv` | Windows-1252 (`é`, `£`), CRLF. Three preamble lines before the header, `Paid out` / `Paid in` columns holding positive amounts, running balance, quoted thousands. Flagged: both columns filled, a negative money-out |
| `semicolon-decimal-comma.csv` | UTF-8 with BOM, `;`-separated, `DD.MM.YYYY`, `1.234,56` decimals, a per-row currency column (one row in USD, flagged), description from two columns |
