# Using Coffer from an AI client (MCP)

Coffer runs a local [MCP](https://modelcontextprotocol.io) server, `coffer mcp`, that AI
clients such as Claude Code, Claude Desktop and Codex start themselves. It talks over
stdin/stdout only: no port is opened and nothing leaves your machine except what the
AI client itself sends to its model provider. Design and rules: `ARCHITECTURE.md` →
"MCP surface".

What a client can do:

- **Read** your data through tools such as `monthly_summary`, `spending_by_category`,
  `search_transactions` and `recurring_payments`, limited by the access level you give it.
- **Suggest** changes (categories, rules, transfer pairs, merchant names, refund links)
  and **save reports**. Suggestions wait in Coffer's inbox until you approve or reject
  them; nothing the AI does changes a transaction, rule, category or account.
- Use the **`monthly_review` prompt**, which walks a month and saves the review as a
  report.

Every call is recorded in Coffer's access log.

## Before you start

- Coffer must be installed and on your `PATH` (`coffer --version`), and a database must
  exist: import something first (`coffer import <file>`). The server never creates or
  migrates a database.
- If you keep your data somewhere other than the default data directory, set
  `COFFER_DATA_DIR` in the client's configuration (each example below shows where).

## 1. Choose a name and level for the client

Each client is named in its own configuration with `coffer mcp --client <name>`. The name
is how Coffer recognises it: letters, digits, `.`, `_` and `-`, for example
`claude-code`, `claude-desktop` or `codex`. It isn't a password. Any program on your
computer can start `coffer mcp` with any name, and any program running as you can read
the database file. The levels guard against over-sharing with a client you trust; they
are not a security boundary.

There are three levels:

| Level | The client sees |
|---|---|
| `aggregate` | Totals only: monthly summaries, spending by category, budgets, net worth (accounts by type, not name), categories and saved reports. It can save reports. No transaction rows. |
| `pseudonymous` | Also transaction rows and recurring payments, with every merchant replaced by a stable token (`M-3f9a0c…`), account names, and filters by date, account, category and amount. No bank descriptions, no text search, no tags. |
| `standard` | Everything above with real merchant names, bank descriptions (as labelled, sanitised untrusted text), text search, public tags, rules and merchant history, and the `propose_*` tools. Full access by design: a merchant name or bank description can be a person's name. |

Notes, share labels, private tags, merchant addresses, account numbers, sort codes and
IBANs are never sent at any level.

Enable the client, then set its level. Enabling starts at `aggregate`:

```sh
coffer ai-access enable claude-code
coffer ai-access level claude-code pseudonymous    # or aggregate, standard
coffer ai-access list                              # every client, its level and status
```

A client you haven't enabled can still start `coffer mcp`, but it sees no tools and no
prompts. Coffer records its name as `requested`, so `coffer ai-access list` shows who
asked. `coffer ai-access ignore <name>` hides a request.

A change applies to a running client's next call: you don't need to restart it. When a
level goes down (or a client is revoked), the server also tells the client to list its
tools again.

## 2. Configure the client

Each example names the client after itself; use the name you enabled.

### Claude Code

Add Coffer as a server for your user (all projects):

```sh
claude mcp add --scope user coffer -- coffer mcp --client claude-code
```

or in a project's `.mcp.json`:

```json
{
  "mcpServers": {
    "coffer": {
      "command": "coffer",
      "args": ["mcp", "--client", "claude-code"],
      "env": { "COFFER_DATA_DIR": "/path/to/your/coffer/data" }
    }
  }
}
```

Leave out `env` when you use the default data directory. `/mcp` in Claude Code shows
whether the server connected and lists its tools.

### Claude Desktop

Edit `claude_desktop_config.json` (Settings → Developer → Edit Config; on macOS it is
`~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "coffer": {
      "command": "coffer",
      "args": ["mcp", "--client", "claude-desktop"]
    }
  }
}
```

Claude Desktop may not see your shell's `PATH`; if the server doesn't start, use the
full path to `coffer` (`which coffer`) as `command`. Restart Claude Desktop after
editing.

### Codex

In `~/.codex/config.toml`:

```toml
[mcp_servers.coffer]
command = "coffer"
args = ["mcp", "--client", "codex"]
# env = { COFFER_DATA_DIR = "/path/to/your/coffer/data" }
```

or `codex mcp add coffer -- coffer mcp --client codex`.

## 3. Use a profile with Coffer as the only data source

Transaction text comes from your bank statements, and anyone who can put text on a
statement (a payee's reference, a merchant's name) can try to give the AI instructions.
Coffer withholds text that reads like instructions and marks bank text as untrusted, and
the AI can only *suggest* changes. The remaining risk is an AI that reads your data and
then sends it somewhere. So run Coffer in a client profile that has:

- Coffer as its only MCP server: no other servers that read or send data;
- no web search, web fetch or browser tools;
- no shell or network access it doesn't need.

How to do that, per client (check your client's current documentation; options change):

- **Claude Code**: start a session with only Coffer's server and without the web
  tools, from a directory that holds nothing else:

  ```sh
  claude --strict-mcp-config --mcp-config coffer-only.json \
         --disallowedTools WebFetch WebSearch
  ```

  where `coffer-only.json` holds the `.mcp.json` above. You can also deny `WebFetch` and
  `WebSearch` under `permissions.deny` in that directory's `.claude/settings.json`.
- **Claude Desktop**: keep Coffer the only entry under `mcpServers`, and turn off web
  search and other connectors for the conversations where you use it.
- **Codex**: keep Coffer the only `[mcp_servers]` entry in the profile you use for it,
  leave web search off, and keep the sandbox's network access off.

## 4. Ask, review, approve

Ask in the client, for example *"Where did my money go last month?"*, or use the
`monthly_review` prompt with a month (`2026-10`). The client calls Coffer's tools; each
call carries a short `purpose`, which you'll see in the log.

Suggestions and reports wait in the inbox:

```sh
coffer inbox                       # pending suggestions
coffer inbox show 12               # one suggestion, with what approving it would change
coffer inbox approve 12 13         # approve several at once: all or nothing
coffer inbox approve 14 --apply-to-history   # a rule, also applied to past rows
coffer inbox reject 15 --reason "wrong category"
```

Saved reports:

```sh
coffer inbox reports               # saved reports, newest first (no bodies)
coffer inbox report 7              # one report, as plain text
```

Text an AI wrote is shown labelled as unverified, without hidden characters; a report's
markdown and links are printed as they are, never rendered or opened.

## 5. Check what the AI read: the access log

```sh
coffer ai-access log                       # the latest 50 calls, newest first
coffer ai-access log --client codex --limit 200
```

Each row shows the time, client, tool, the level and outcome (`ok`, `refused_off`,
`refused_not_enabled`, `refused_level`, `refused_restoring`, `refused_unavailable`,
`error`), how many rows the call returned, how many
descriptions were withheld as instructions, the client's `purpose`, and the call's
arguments with any free text replaced by its length (`{"length":12,"type":"string"}`).
The log never stores what was returned. Rows older than 12 months are deleted when a
server starts.

Calls refused because the database couldn't be opened are logged too, a little later:
`refused_restoring` while a backup was being restored, `refused_unavailable` when the
database needed updating (migrations pending) or the restore lock couldn't be checked.
The log is inside the database, so the server keeps them in `coffer.db-access-pending`
next to it (same fields, same redaction) and moves them in, at the time they happened,
on the next call that can open the database, or when you run `coffer ai-access`. If a
client keeps calling for a long time while the database is unavailable, the file stops at
1,000 calls; the rest are counted, and the log shows one row per client and tool with
how many (`refused_restoring (40 calls)`).

## The kill switch

```sh
coffer ai-access off    # every client is refused from its next call
coffer ai-access on     # the enabled clients work again, at their levels
coffer ai-access revoke codex   # one client; it stays listed and can be enabled again
```

`off` takes effect on a running client's next call and is logged as `refused_off`.

## Restoring a backup while a client is connected

`coffer restore` works while a client is connected: the server keeps no database
connection open between calls. While the files are swapped it refuses calls with
"Coffer is restoring a backup", and its next call reads the restored database. Those
refused calls are in the restored database's log as `refused_restoring`, at the time
they were made. Restoring an older backup doesn't roll the log back either: the calls
logged since that backup was taken are copied into the restored database, and
`coffer restore` says how many.

If a restore is killed or crashes, nothing stays blocked: the restore's lock on
`coffer.db-restoring` goes with its process, and the database is complete either way (the
one before the restore, or the backup). The file itself stays next to the database, empty
between restores; that's expected. The next `coffer restore` warns that an earlier one
stopped, and goes ahead.

## Troubleshooting

- **No tools listed**: the client isn't enabled (`coffer ai-access list`; enable it under
  the exact name in its configuration), or the kill switch is off.
- **"Coffer couldn't open its database"**: no database at the data dir the server sees
  (check `COFFER_DATA_DIR`), or it needs migrating: run any `coffer` command, such as
  `coffer accounts`, which updates it.
- **A tool is missing**: it's above the client's level (`coffer ai-access level`).
- **"Coffer can't tell whether a backup is being restored"**: the server can't open or lock
  `coffer.db-restoring` next to the database. Check that you own it and that the data dir
  is on a local disk (file locks may not work on a network share). When no `coffer
  restore` is running, deleting the file is safe.
- **Debug output**: set `COFFER_LOG=debug` in the server's `env`. Logs go to stderr and
  never hold transaction text at `info` or above.
