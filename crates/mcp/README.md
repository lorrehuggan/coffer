# coffer-mcp

Coffer's MCP server: `coffer mcp --client <name>` runs it over stdio. Design and rules:
`docs/ARCHITECTURE.md` → "MCP surface" and `AGENTS.md` → Invariants.

## How a call runs

`server.rs` handles every `tools/call` in one wrapper, **gate → call → log**:

1. `access::for_request` reads the kill switch and the client's row (on every call and
   every `tools/list`; an unknown name is recorded as `requested`), and `access::gate`
   allows the call or refuses it with an outcome (`refused_off`, `refused_not_enabled`,
   `refused_level`). When the level differs from what the client last listed, the server
   sends `notifications/tools/list_changed`.
2. The handler runs with a `Call`: the read-only connection (`Call::reader`), the
   client's level, constructors for output types and, only for tools registered as
   writing, the inbox.
3. Exactly one `mcp_access_log` row is written, whatever happened. If it can't be
   written, the call fails and no data goes out.

The database is opened for each call (and each `tools/list` and poll) and closed after
it: no connection is kept between calls. While `coffer restore` holds the file lock on
`<database>-restoring` (`backup::restoring`), checked before and after opening, calls are
refused with a message. A lock file nobody holds (a crashed restore's) doesn't block
calls.

A call refused because the database can't be opened (a restore: `refused_restoring`;
migrations pending or the lock unreadable: `refused_unavailable`) can't be logged in it,
so the wrapper records it with `access_log::pending::record` in `<database>-access-pending`
next to the database. Every call and listing that does open it first moves whatever
waits there into the log, through `Store::flush_pending`, at the
calls' own times; IDs make a repeated flush harmless. A restore keeps the log too:
`backup::restore` carries over the rows logged after the backup was taken
(`access_log::carry_forward`). Tests: `tests/restore.rs`,
`tests/unavailable.rs`; the file's own guarantees are tested in `coffer_core`.

Queries run on `coffer_core::db::Reader`, which SQLite keeps read-only (`query_only`).
The only writes are in `store.rs`: a suggestion, a report (through `Inbox`), the
access-log row (the wrapper's own, and the refused calls moved in from the pending file)
and the server's own client name as `requested`.

## Adding a tool

A tool is a change inside **one group file** in `src/tools/` (plus its core query and
its test file). Pick the group by kind of data, not level:

| File | Tools |
|---|---|
| `tools/catalog.rs` | reference data: `list_categories` (the example), `list_accounts`, `list_rules`, `list_reports` |
| `tools/aggregate.rs` | totals: `monthly_summary`, `spending_by_category`, `budget_status`, `net_worth_history` |
| `tools/rows.rs` | transaction rows: `search_transactions`, `merchant_history`, `uncategorized_transactions` |
| `tools/recurring.rs` | `recurring_payments`, schedules |
| `tools/inbox.rs` | `propose_*`, `save_report` |

1. **Query in core.** Put the SQL in a `coffer-core` function that takes
   `&coffer_core::db::Reader` and returns domain types (money as `Money`, never floats).
   Business logic stays in core; the tool only shapes the result.
2. **Spec.** A `const ToolSpec { name, description, level, writes }`:
   - `name`: `snake_case`, unique.
   - `level`: the lowest `AccessLevel` that sees it. `Aggregate` returns totals only, no
     transaction rows; `Pseudonymous` may return rows with merchants as tokens;
     `Standard` is everything else.
   - `writes`: `Writes::Nothing`, or `Writes::Inbox` for a tool that files suggestions
     or saves reports. Only `Inbox` tools get `call.inbox()`.
3. **Arguments.** A `#[derive(Deserialize, JsonSchema)]` struct with doc comments on
   each field (they become the schema's descriptions), or `NoArgs`. Don't declare
   `purpose`: the registry adds it to every tool, and the wrapper logs it. Never read or
   act on it.
4. **Result.** A `#[derive(Serialize, JsonSchema)]` struct (a JSON object) built from:
   - `call.amount(money)` → `Amount { minor, currency, home }`: integer minor units;
   - `call.merchant(display)` → `MerchantLabel`: the display name, or a token at
     `pseudonymous`;
   - `call.untrusted(raw)` → `UntrustedText`, only in a field named
     `description_untrusted`.

   Never return account numbers, sort codes, IBANs, names, addresses, user notes, imported
   notes, share labels, private tags or attachments, and never put bank text in a plain
   `String`. Tags only at `Standard` (`call.level()`).
5. **Handler.** `fn(&Call<'_>, Args) -> Result<Reply<Result>, ToolError>`. Map core
   errors with `.map_err(ToolError::core)?` (the client sees a generic message; the cause
   goes to the debug log). `Reply::new(body, rows)`: `rows` is how many records the
   result describes, for the access log.
6. **Register** it in the group's `register()`: `registry.add(SPEC, handler);`.

## Tests every tool needs

Copy `tests/list_categories.rs`. The harness is `tests/support/mod.rs`:

- `Fixture::new()` builds a database from the synthetic fixtures through the real import
  path and seeds a user note, a private tag, a share label and bank text with a name,
  sort code, account number and IBAN. `COFFER_DATA_DIR` points at a scratch directory.
- `fixture.connect(client, level).await` starts the server in-process and returns a
  `TestClient` (`call`, `call_ok`, `try_call`, `tools`, `tool_names`, `close`).
- `assert_no_pii(&value)`; `call_ok` already runs it on the whole response.
- `registered_tools(level)` lists the registry's tools for a level.
- `fixture.access_log()` reads the log back, oldest first.

Each tool's test file checks at least:

1. **Level registration**: listed at its level and above, absent below (and refused
   with `refused_level` if called anyway).
2. **No PII**: `call_ok` on the fixture database at every level that sees the tool.
3. **Access log**: one call writes exactly one row with the tool, client, level,
   outcome, `row_count` and `purpose`, and `args_redacted_json` holds no free text.

Writing tools also check that they write only `suggestion` or `report`, with
`source_client` set to the calling client. No tool changes or deletes an existing row:
deleting saved reports is the user's (`coffer inbox delete-report`), and the matrix
checks that no server code reaches it.

Then add the tool to `MATRIX` in `tests/matrix.rs`, with its level, whether it writes,
and valid arguments: `the_matrix_lists_every_registered_tool` fails until you do, and the
matrix runs the checks above for every tool at every level.

## Prompts

`src/prompts.rs` has `monthly_review(month)`. A prompt's text is built from the tools
registered at the client's level, so a step naming a tool above it is left out; mention
tools as `` `name` `` so `tests/monthly_review.rs` can check that.
