# Exported transactions

What `coffer export` and the app's export dialog (Settings → Export…) write: one row per
ledger line, as CSV or JSON. Both run the same code (`coffer_core::export`); this page is
its module documentation, copied here so the columns are documented outside the code. If
the two disagree, the code's documentation is right; update this page to match.

The app's dialog maps its controls onto the same options: the dates, account, category
and tag become the filters below; "Excluded transactions" leaves excluded rows out when
off (the CLI always includes them); "Notes and tags · including private ones" turns on
both `--include-notes` and `--include-private-tags`. Files are created readable only by
you (`0600`), and an export is never written over Coffer's database.

Exporting transactions as CSV or JSON (M1.29, `coffer export`).

An export is one row per line of the ledger (the `ledger_line` view): a
transaction that isn't split is one row, and a split transaction is one
row per split line, each carrying its parent's `transaction_id` with its
own `split_id` and `line_no`. Money in with confirmed links (a refund,
reimbursement or settlement, M1.25) is its own row for the unlinked rest
plus one row per link part, each with `link_id`: a refund's part has the
purchase's category (and one row per purchase line when the purchase is
split). A transaction's rows always sum to its amount. Transaction fields (date, account,
description, …) repeat on each line of a split; the amount, my amount
and category are the line's.

The export doesn't filter by status unless asked: excluded rows and
transfers are included, with `excluded_reason` and `transfer_status` to
filter by, as in `ledger_line`. `Options::include_excluded` off leaves
the excluded rows out (the app's export dialog, M3.16). Rows are ordered by date, then transaction ID, then
line number, then link.

## Filters (`Filter`)

Every filter given must match (AND); several values of one filter match
any of them (OR).

- **Dates**: `from` and `to`, both included, on the transaction's date.
- **Accounts**: by ID. A pot is its own account: naming its parent
  doesn't include it.
- **Categories**: by key, on the row's own category (a split line's, or
  the transaction's). A group's key includes its subcategories:
  `food` matches `food` and `food.groceries`. Uncategorised rows match no
  category filter.
- **Tags**: by name, ignoring case like tag names do; a row matches when
  its transaction has any of the tags. A private tag can be filtered by
  only when private tags are included (`Options::include_private_tags`):
  otherwise which rows carry it would leave the app.

## What is left out, and the opt-ins (`Options`)

- **Share labels never leave the app** (decision record "Data model":
  `share_label`), so no export has them. `my_amount` still says how much
  of a row is the user's.
- **Private tags** are left out by default: `tags` has the public ones
  only (`crate::tag::public_tags_of`). With
  `Options::include_private_tags`, a `private_tags` column is added
  with the private ones; `tags` stays public-only, so the two are told
  apart.
- **Notes** are left out by default. With `Options::include_notes`,
  four columns are added: `user_note` (the user's note on the
  transaction), `line_note` (a split line's note), `excluded_note` (the
  note on an exclusion) and `bank_note` (the note imported from the bank's
  export, which in e.g. Monzo's is text the user wrote in the bank's
  app). Without it, none of these columns is there.

## Formats

- **CSV**: UTF-8, comma-separated, a header row of the column names,
  `\n` line endings, fields quoted (with `"` doubled) only when they hold
  a comma, a quote or a line break. An empty field is a missing value.
  Booleans are `true`/`false`. A list (`tags`, `private_tags`) is its
  names joined with `"; "`; a tag name can itself hold `;`, so the JSON
  form is the exact one. The CSV has no version marker: its header row
  is the schema.
- **JSON**: one object, `{"schema": "coffer.export", "schema_version":
  1, "includes": {"notes": …, "private_tags": …}, "rows": [ … ]}`, each
  row an object with the columns below as keys, in that order. A missing
  value is `null`, a list is an array of strings, a boolean is
  `true`/`false`, and numbers are integers only (IDs, line numbers and
  `*_minor` amounts).

Neither has a float: every amount is an exact decimal string in major
units (`-1234.56`, `Money::to_decimal_string`) beside its `*_minor`
integer, in the row's `currency` (`original_*` in `original_currency`).

## Columns (schema version 1)

| Column | Type | Meaning | Present |
|---|---|---|---|
| `transaction_id` | integer | The transaction's ID; a split line's parent | always |
| `split_id` | integer or empty | The split line's ID | split lines |
| `line_no` | integer or empty | The split line's number, from 1 | split lines |
| `date` | `YYYY-MM-DD` | The transaction's date | always |
| `time` | `HH:MM:SS` or empty | The transaction's time, when the bank gave one | when known |
| `account_id` | integer | The account's ID | always |
| `account_name` | text | The account's name | always |
| `currency` | ISO 4217 code | The currency of every amount but `original_*` | always |
| `amount` | decimal text | The row's amount (the line's, or the transaction's); negative is money out | always |
| `amount_minor` | integer | `amount` in minor units | always |
| `my_amount` | decimal text | The user's part of `amount` (all of it unless a share is set) | always |
| `my_amount_minor` | integer | `my_amount` in minor units | always |
| `transaction_amount` | decimal text | The whole transaction's amount; equals `amount` unless split | always |
| `transaction_amount_minor` | integer | `transaction_amount` in minor units | always |
| `original_amount` | decimal text or empty | The whole transaction in the currency spent, when it differs | foreign spending |
| `original_amount_minor` | integer or empty | `original_amount` in minor units | foreign spending |
| `original_currency` | ISO 4217 code or empty | The currency spent | foreign spending |
| `merchant` | text or empty | The merchant as the bank gave it | when given |
| `merchant_display` | text or empty | The cleaned merchant name the app shows | when known |
| `description` | text | The bank's description, as imported (or as entered) | always |
| `category_key` | text or empty | The row's category key, e.g. `food.groceries` | categorised rows |
| `category_label` | text or empty | The category's label | categorised rows |
| `category_source` | `rule`, `preset_map`, `user`, `ai_approved` or empty | What set the transaction's category | categorised, unsplit rows |
| `tags` | list | The transaction's public tags, by name | always (may be empty) |
| `reimbursable` | boolean | Whether the user expects the money back | always |
| `excluded_reason` | `duplicate`, `bank_error`, `disputed`, `other` or empty | Why the transaction is excluded from reports (only `duplicate` also leaves the balance) | excluded rows |
| `excluded_at` | UTC timestamp or empty | When it was excluded | excluded rows |
| `transfer_group_id` | integer or empty | The transfer pair the transaction is in | paired rows |
| `transfer_status` | `suggested`, `confirmed`, `rejected` or empty | That pair's status | paired rows |
| `origin` | `imported`, `mirrored`, `manual` | Where the transaction came from (`mirrored`: the pot side of a pot transfer) | always |
| `batch_id` | integer or empty | The import batch that brought it | imported and mirrored rows |
| `external_id` | text or empty | The bank's own ID for the transaction | when the file had one |
| `source_category` | text or empty | The bank's category, as exported | when the file had one |
| `source_type` | text or empty | The bank's transaction type, as exported | when the file had one |
| `created_at` | UTC timestamp | When the transaction entered the database | always |
| `link_id` | integer or empty | The confirmed link this row is a part of | link parts |
| `link_kind` | `refund`, `reimbursement`, `settlement` or empty | That link's kind | link parts |
| `linked_transaction_id` | integer or empty | The purchase a refund or reimbursement part nets against | refund and reimbursement parts |
| `in_reports` | boolean | Whether the row counts in spending, income and reports: not excluded, not a confirmed transfer (for a link part, nor is its purchase) | always |
| `personal_amount` | decimal text | What the row counts as the user's own spending or income: `my_amount`, or 0 when not in reports or reimbursable | always |
| `personal_amount_minor` | integer | `personal_amount` in minor units | always |
| `user_note` | text or empty | The user's note on the transaction | with notes |
| `line_note` | text or empty | The user's note on the split line | with notes |
| `excluded_note` | text or empty | The user's note on the exclusion | with notes |
| `bank_note` | text or empty | The note imported from the bank's export | with notes |
| `private_tags` | list | The transaction's private tags, by name | with private tags |

Timestamps are `YYYY-MM-DDTHH:MM:SS.SSSZ`. A later schema version may add
columns at the end; a change to an existing column's meaning or type
bumps `SCHEMA_VERSION`.

Exports are personal data: `to_file` creates the file `0600` (on Unix).
Nothing here logs row text.
