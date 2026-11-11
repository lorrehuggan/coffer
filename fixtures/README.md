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

## `ofx/`

Synthetic OFX and QFX files for the OFX importer (M1.10), written by hand
(account numbers are zeros or end in `1111`-style digits):

| File | Shape |
|---|---|
| `bank-sgml.ofx` | OFX 1.02 SGML, `ENCODING:USASCII` + `CHARSET:1252` (`é`, `£` as single bytes), CRLF. GBP current account: `NAME` + `MEMO`, `&amp;`, a `NAME` cut short from its `MEMO`, a cheque, an `ORIGCURRENCY` row; `LEDGERBAL` and `AVAILBAL` |
| `creditcard-xml.ofx` | OFX 2.2 XML with an XML declaration, UTF-8, LF. USD credit card: times with `[-5:EST]` offsets, an empty `<MEMO></MEMO>`, a `PAYEE` aggregate with an address, `CURRENCY` naming the statement's own currency, `EXTDNAME`; a negative `LEDGERBAL` |
| `intuit.qfx` | QFX: OFX 1.02 SGML with `INTU.BID` / `INTU.USERID` and some values closed (`<NAME>…</NAME>`), LF, USD checking, placeholder `120000` times |
| `edge-cases.ofx` | OFX 1.03 SGML, `ENCODING:UTF-8`, EUR, no `LEDGERBAL`, no trailing newline. Read: no `FITID`, a decimal comma, an empty SGML `<NAME>`, an `HHMM` time, an unknown bank value, a transaction with no text. Flagged: no `DTPOSTED`, 30 February, too many decimals, a repeated `FITID`, a `CORRECTACTION`, an amount in USD, `TRNAMT` twice, no `TRNAMT`, an unknown `CURSYM` |
| `multi-statement.ofx` | OFX 2.1.1 XML without an XML declaration: two GBP bank statements and a GBP credit card in one file |

## `presets/`

One fixture per bank preset, at `presets/<region>/<bank>.<ext>` to match
`presets/<region>/<bank>.toml`. A test requires each bundled preset to have one, to
recognise it from its header and to import it.

| File | Shape |
|---|---|
| `uk/monzo.csv` | Monzo's CSV export (M1.9), written by hand from the export's documented columns, not from a real file. UTF-8, LF, 18 columns. One row per quirk: a refund as a positive amount under "Money Out", pot transfers both ways (pot name as `Name`, pot ID as `Description`; `Type` is `Pot transfer`) into and out of one pot, into a second pot, and into the first pot again under a new name (the user renamed it; same pot ID), a card payment in EUR with its local amount, a £0.00 card check, two identical same-day coffees with their own IDs, a note over two lines with escaped quotes, a payment from a person, emoji and non-ASCII merchants, quoted addresses with commas |
