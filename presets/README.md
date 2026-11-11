# Bank presets

A preset tells Coffer how to read one bank's CSV export: which column holds the date,
the amount, the description, and so on. Presets are plain TOML, so you can add a bank
without writing Rust. Every `.toml` file here is compiled into Coffer, and the tests
load all of them.

## Where a preset goes

`presets/<region>/<bank>.toml`, one file per bank and account type. Regions are lowercase
letters (`uk`, `us`, `eu`); file names are lowercase letters, digits and hyphens
(`monzo.toml`, `nationwide-credit-card.toml`).

Every preset needs a redacted fixture at `fixtures/presets/<region>/<bank>.csv` (same
name, any extension) that it recognises and imports, plus a test that checks the rows.
Fixtures must be synthetic or fully redacted: see [`fixtures/README.md`](../fixtures/README.md)
and `AGENTS.md` → "Personal financial data". Never commit a real export.

## Example

The Monzo preset, [`uk/monzo.toml`](uk/monzo.toml), is the reference: copy it when
adding a bank, comments and all. Its fixture is
[`fixtures/presets/uk/monzo.csv`](../fixtures/presets/uk/monzo.csv) and its tests are in
`crates/import/tests/preset_uk_monzo.rs`. Without the comments it reads:

```toml
id = "uk-monzo"                    # stable forever: it's recorded on every import
name = "Monzo"                     # what the user sees
region = "uk"                      # must match the directory
currency = "GBP"                   # the account's ISO 4217 currency

encoding = "utf-8"                 # optional, default "auto"
delimiter = ","                    # optional, detected when left out
skip_rows = 0                      # optional, lines before the header
header = true                      # optional, whether the file has a header row
multiline_fields = true            # optional, whether quoted fields can hold line breaks

external_id = { column = "Transaction ID" }
date = { column = "Date", format = "%d/%m/%Y" }
time = { column = "Time", format = "%H:%M:%S" }
amount = { column = "Amount" }
decimal = "."
row_currency = { column = "Currency" }
original_amount = { column = "Local amount", currency_column = "Local currency" }
merchant = { column = "Name" }
description = { column = "Description" }
source_category = { column = "Category" }
source_type = { column = "Type" }
note = { column = "Notes and #tags" }
ignore = ["Emoji", "Address", "Receipt", "Category split", "Money Out", "Money In"]

[category_map]                     # the bank's category -> Coffer's category key
"Groceries" = "food.groceries"
"Eating out" = "food.eating_out"
"Savings" = "transfer.savings"
# … and the rest of Monzo's built-in categories that map cleanly
```

## Fields

Unknown keys are an error, so a typo fails loudly instead of being ignored.

### About the preset

| Key | Required | Meaning |
|---|---|---|
| `id` | yes | `<region>-<bank>`, optionally followed by more: `uk-monzo` or `uk-monzo-current` for `uk/monzo.toml`. Lowercase letters, digits and single hyphens. **Never change it once shipped**: import batches record it |
| `name` | yes | Shown when choosing a preset |
| `region` | yes | The directory the file is in |
| `currency` | yes | The account's ISO 4217 code. Every amount is read in it |

### Reading the file

| Key | Default | Meaning |
|---|---|---|
| `encoding` | `"auto"` | `"auto"` (a byte order mark, otherwise UTF-8), `"utf-8"`, `"utf-16le"`, `"utf-16be"`, `"windows-1252"` (also reads Latin-1) or `"iso-8859-15"`. Coffer never guesses: an export that isn't UTF-8 must say so here |
| `delimiter` | detected | `","`, `";"`, `"\t"` or `"\|"`. State it if detection could tie |
| `skip_rows` | `0` | Lines before the header (an account summary), counted as lines in the file: a summary cell with a line break inside its quotes is two. A count that ends inside such a cell fails the file |
| `header` | `true` | Whether the first line after `skip_rows` names the columns |
| `multiline_fields` | `false` | Whether the export puts line breaks inside quoted fields (a note over several lines). Leave it off otherwise: a row over several lines is then flagged, since it usually means a stray quote has swallowed the rows after it. A quote still open at the end of the file is flagged either way |
| `decimal` | `"."` | The decimal mark: `"."` for `1,234.56`, `","` for `1.234,56` |

### Columns

A column is its header name (`"Date"`, matched exactly after trimming spaces) or, when
`header = false`, its position counted from 1 (`1` is the first column).

| Key | Required | Form |
|---|---|---|
| `date` | yes | `{ column = …, format = "%d/%m/%Y" }`. A `strftime` format; it may include a time. Every value must read back exactly, so `%d/%m/%Y` refuses `1/11/2026` (use `%-d/%-m/%Y`). Time zones aren't supported |
| `time` | no | `{ column = …, format = "%H:%M:%S" }`. Overrides any time in the date |
| `amount` | yes | `{ column = … }` for one signed column (negative is money out), or `{ debit = …, credit = … }` for money-out / money-in columns. Add `signs = "signed"` when those already carry a sign; the default `"unsigned"` expects positive amounts. Prefer the signed column when an export has both |
| `description` | yes | `{ column = … }`, or `{ columns = [ … ] }` to join several with a space |
| `merchant` | no | `{ column = … }`. The bank's cleaned-up merchant name |
| `external_id` | no | `{ column = … }`. The bank's own transaction ID, used first for deduplication |
| `source_category` | no | `{ column = … }`. The bank's category (needed for `category_map`) |
| `source_type` | no | `{ column = … }`. "Card payment", "Direct debit", … |
| `row_currency` | no | `{ column = … }`. A per-row currency code; rows in another currency than `currency` are flagged |
| `original_amount` | no | `{ column = …, currency_column = … }`. The amount in the currency spent abroad |
| `note` | no | `{ column = … }`. The note the user wrote in the bank's app |
| `balance` | no | `{ column = … }`. The running balance after each row |

### Columns not read

`ignore = [ … ]` lists header columns the preset deliberately doesn't read. When a file
has a column that's neither read nor ignored (a bank added one), Coffer can say so in the
import preview rather than leave it out silently. Needs `header = true`.

### Categories

`[category_map]` maps the bank's category names (as in `source_category`, matched
exactly, case included) to Coffer's category keys: one or two parts of lowercase
letters, digits and `_`, joined by a dot (`food.groceries`, `transfer`). Map only to
the default categories listed in `docs/ARCHITECTURE.md` → "Categories" (a test checks
it): they are the ones every Coffer database has. Map a bank category to a group
(`travel`) when no subcategory fits. Unmapped categories are left for rules or the user.

### Not yet

Sub-account transfers (Monzo Pots, Starling Spaces, `[sub_account_transfer]`) come with
M1.17. Until then a preset using them is refused.
