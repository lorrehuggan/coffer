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
| `newest-first-tab.tsv` | M1.19. UTF-8, LF, tab-separated (detected), ISO dates, no ID column, a running balance with the newest row first (opening 1000.00 at the end of 9 November). Two identical same-day coffees; a 250.00 transfer out on 15 November that pairs with `ofx/multi-statement.ofx`'s savings statement |
| `newest-first-tab-later.tsv` | M1.19. The next export of the same account: repeats the whole of 20 November (found again by fingerprint), then three new rows; its opening (end of 19 November) checks the first file's rows |
| `headerless-preamble.csv` | M1.19. UTF-8, LF, two free-text preamble lines (one with a mid-field quote), no header row, `DD Mon YYYY` dates, a type column, oldest first. The running balance on line 5 doesn't follow from line 4 (a 7.50 row missing), so the import has a balance mismatch |
| `repeated-ids-us.csv` | M1.19. UTF-8, LF, USD, `MM/DD/YYYY`, a `Debit` / `Credit` pair that carries its signs, IDs. One ID repeated with the same date and amount (repeated in file), one with another date and amount (flagged), a quoted memo over two lines (flagged unless `multiline_fields`), two identical rows without an ID, a `0.00` debit beside a credit |

### `csv/mappings/`

Test presets (M1.19) that read the generic CSV fixtures above, at
`mappings/<region>/<name>.toml` so `Presets::from_files` loads them. They aren't
bundled (only `presets/` is) and use fake bank names. The end-to-end tests
(`crates/import/tests/csv_paths.rs`, `commit_paths.rs`, `crates/app/tests/cli_paths.rs`)
read files through them.

| File | Reads |
|---|---|
| `uk/example-current.toml` | `signed-amount.csv`, every optional column and a category map |
| `uk/example-savings.toml` | `debit-credit-windows-1252.csv` |
| `eu/example-giro.toml` | `semicolon-decimal-comma.csv` |
| `uk/example-tab.toml` | `newest-first-tab.tsv`, `newest-first-tab-later.tsv` |
| `uk/example-headerless.toml` | `headerless-preamble.csv` (positions; chosen by name only, as a file without a header can't be recognised) |
| `us/example-checking.toml` | `repeated-ids-us.csv` |

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
| `bank-sgml-next.ofx` | M1.19. The next statement of `bank-sgml.ofx`'s account: OFX 1.02 SGML, `ENCODING:USASCII` + `CHARSET:NONE`, LF. Repeats `EX-0006` as is and `EX-0007` re-worded (already imported), `EX-0005` for another amount (a source ID conflict), then three December rows; a `LEDGERBAL` of 3806.98 on 31 December that reconciles against November's |

## `presets/`

One fixture per bank preset, at `presets/<region>/<bank>.<ext>` to match
`presets/<region>/<bank>.toml`. A test requires each bundled preset to have one, to
recognise it from its header and to import it.

| File | Shape |
|---|---|
| `uk/monzo.csv` | Monzo's CSV export (M1.9), written by hand from the export's documented columns, not from a real file. UTF-8, LF, 18 columns. One row per quirk: a refund as a positive amount under "Money Out", pot transfers both ways (pot name as `Name`, pot ID as `Description`; `Type` is `Pot transfer`) into and out of one pot, into a second pot, and into the first pot again under a new name (the user renamed it; same pot ID), a card payment in EUR with its local amount, a £0.00 card check, two identical same-day coffees with their own IDs, a note over two lines with escaped quotes, a payment from a person, emoji and non-ASCII merchants, quoted addresses with commas |
