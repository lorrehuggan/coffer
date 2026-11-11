# Migrations

SQL migrations for Coffer's SQLite database, embedded into `coffer-core` at
compile time by `refinery` and applied by `coffer_core::db::Database::open` (the
app and the CLI). `Database::open_existing` (`coffer mcp`) never applies them: it
refuses a database with pending migrations.

- Name files `V<n>__<snake_case_name>.sql`, with `<n>` one higher than the last
  file (`V1__accounts.sql`, `V2__…`). Only `V` (versioned) migrations are used.
- **Never edit a migration that has shipped.** `refinery` checksums every
  applied migration and refuses to open a database whose history differs from
  these files. A schema change is always a new file (`AGENTS.md` → Invariants).
- All pending migrations run in one transaction: a failure leaves the database
  at its previous version.
- A database at a newer version than this build knows, or whose applied migrations
  differ from these files, is refused before anything is written: its contents,
  permissions and journal mode stay as they were.
- Follow the conventions at the top of `V1__initial_schema.sql` (`STRICT` tables,
  checked dates, no `ON DELETE` actions, indexed foreign keys, cross-row rules as
  `BEFORE` triggers with indexed lookups), and add new tables to `TABLES` in
  `crates/core/src/db/schema_tests.rs`.
- Never write `"transaction"` with `REPLACE`, `INSERT OR REPLACE`, `UPDATE OR REPLACE`
  or an upsert (`ON CONFLICT`). REPLACE deletes the conflicting row and inserts a new
  one, which no `UPDATE` trigger sees, so it would get round the read-only source
  fields. The `transaction_insert_new` trigger refuses an insert that collides with a
  stored row, and `no_replace_on_transactions` in `schema_tests.rs` scans
  `crates/core/src` for such statements. A new table with read-only fields needs the
  same protection.
- Adding a column or an index is a plain `ALTER TABLE` / `CREATE INDEX`. Changing a
  `CHECK` (such as adding an enum key) means rebuilding the table (SQLite's "12-step"
  procedure), which needs foreign keys off around the migration. `db::migrations::run`
  doesn't support that yet: add it with the first migration that needs it.
