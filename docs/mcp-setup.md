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

- You need the `coffer` binary. Coffer isn't packaged yet, so build it from the repository
  in the dev shell (`cargo build --release -p coffer-app`) and use the absolute path of
  `target/release/coffer` (`realpath target/release/coffer`). Clients start the server
  without your shell's `PATH`, so every example below gives `command` as an absolute path,
  shown as `/path/to/coffer`. A binary built in the dev shell also runs outside it: it
  finds its libraries in the Nix store. If a garbage collection removes them, rebuild.
- A database must exist: import something first (`coffer import <file>`). The server
  never creates or migrates a database.
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
client's level changes (or it's revoked, or the kill switch is off), the server also sends
`notifications/tools/list_changed`, within about 5 seconds even when the client is idle.
Claude Code then fetches the new tool list. Codex and Claude Desktop don't document what
they do with it, so restart them to see the new list. Either way, a tool above the level is
refused when called.

## 2. Configure the client

Each example names the client after itself; use the name you enabled. Replace
`/path/to/coffer` with the absolute path of your `coffer` binary (see "Before you start"),
and leave out `COFFER_DATA_DIR` when you use the default data directory. Notes such as
*(checked)* say how each step was checked in November 2026: Claude Code 2.1.316 and Codex
CLI 0.166.0 were run on Linux against a synthetic database; the rest comes from each
client's documentation only.

### Claude Code

Add Coffer for your user, in every project *(checked)*:

```sh
claude mcp add --scope user coffer -e COFFER_DATA_DIR=/path/to/your/coffer/data \
  -- /path/to/coffer mcp --client claude-code
```

Put the server name (`coffer`) before `-e`: `-e` takes several values, and a name right
after it is read as another `KEY=value` and rejected. Everything after `--` is the
command Claude Code runs. `--scope user` writes the server to `~/.claude.json` (in your
home directory on every OS, or under `CLAUDE_CONFIG_DIR` if you set it). Without
`--scope`, the server is added for the current directory only (`local` scope, also in
`~/.claude.json`).

Or add it to one project, in a `.mcp.json` at the project's root *(checked)*:

```json
{
  "mcpServers": {
    "coffer": {
      "command": "/path/to/coffer",
      "args": ["mcp", "--client", "claude-code"],
      "env": { "COFFER_DATA_DIR": "/path/to/your/coffer/data" }
    }
  }
}
```

Claude Code asks you to approve a project's `.mcp.json` servers when you first start
`claude` there. Until you do, they show as `⏸ Pending approval`.

**Check it works** *(checked)*. `claude mcp list` (or `claude mcp get coffer`) starts the
server, checks it, and doesn't call a model:

```text
$ claude mcp list
Checking MCP server health…

coffer: /path/to/coffer mcp --client claude-code - ✔ Connected
```

- `✔ Connected` means the server started and found the database. It doesn't mean
  the client is enabled: a name Coffer doesn't know connects too, and it gets no tools.
  `coffer ai-access list` shows whether it's enabled. A name the server saw but you haven't
  enabled is listed as `requested`.
- `✘ Failed to connect — ENOENT: Executable not found in $PATH`: `command` isn't an
  absolute path to the binary.
- `✘ Failed to connect — CONNECTION_CLOSED`: the server started and then stopped. To see
  why, run the same command in a terminal with stdin closed, for example
  `COFFER_DATA_DIR=/path/to/your/coffer/data /path/to/coffer mcp --client claude-code < /dev/null`.
  `there is no Coffer database at …` means the data directory is wrong or empty.
  `connection closed: initialize request` means the server found the database and only
  stopped because nothing talked to it, so the server itself is fine.

In a session, `/mcp` lists Coffer's tools.

### Claude Desktop

*(Documentation only: Claude Desktop wasn't installed where this was checked.)* Claude
Desktop runs on macOS and Windows. On Linux it's in beta, for Ubuntu 22.04+ and Debian 12+
only; on other distributions, use Claude Code. Edit `claude_desktop_config.json`:

| OS | File |
|---|---|
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` |
| Linux | `~/.config/Claude/claude_desktop_config.json` |

The app opens it for you from **Settings → Developer → Edit Config**, and creates it if it
doesn't exist. In newer versions the local MCP servers are under **Settings → Desktop app →
Extensions**.

```json
{
  "mcpServers": {
    "coffer": {
      "command": "/path/to/coffer",
      "args": ["mcp", "--client", "claude-desktop"],
      "env": { "COFFER_DATA_DIR": "/path/to/your/coffer/data" }
    }
  }
}
```

`command` must be an absolute path: Claude Desktop doesn't see your shell's `PATH`. On
Windows, write each `\` in a path as `\\`. Quit Claude Desktop completely and start it
again after each edit.

**Check it works.** Click **+** in the chat box, then **Connectors**: Coffer should be
listed with its tools. The **Local MCP servers** section (or Settings → Developer in older
versions) shows its status. If it doesn't start, look in Claude Desktop's logs:
`mcp-server-coffer.log` holds the server's error output. The logs are in
`~/Library/Logs/Claude` (macOS), `%APPDATA%\Claude\logs` (Windows) or
`~/.config/Claude/logs` (Linux). Then run the command in a terminal, as described under
Claude Code above.

### Codex

Add Coffer with the CLI *(checked)*:

```sh
codex mcp add coffer --env COFFER_DATA_DIR=/path/to/your/coffer/data \
  -- /path/to/coffer mcp --client codex
```

or write it yourself in `~/.codex/config.toml` (`%USERPROFILE%\.codex\config.toml` on
Windows, or `$CODEX_HOME/config.toml` if you set `CODEX_HOME`). This is what the command
above writes *(checked)*:

```toml
[mcp_servers.coffer]
command = "/path/to/coffer"
args = ["mcp", "--client", "codex"]

[mcp_servers.coffer.env]
COFFER_DATA_DIR = "/path/to/your/coffer/data"
```

A project's `.codex/config.toml` works too, but only in a project you've marked as
trusted. The Codex CLI, the IDE extension and the ChatGPT desktop app share this file.

**Check it works.** `codex mcp list` and `codex mcp get coffer` show the configuration
*(checked)*, but they don't start the server: their `enabled` is the setting, not a
connection. To check the command itself, run it in a terminal as described under Claude
Code above *(checked: the command Codex wrote, started the way an MCP client starts it,
listed the seven `aggregate` tools and the `monthly_review` prompt)*. In a Codex session,
`/mcp` lists the servers that are running *(documentation only)*. Codex gives a server 10
seconds to start (`startup_timeout_sec`).

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

- **Claude Code**: put the `.mcp.json` above in a file of its own, say
  `coffer-only.json`, in an empty directory, and start sessions from there with:

  ```sh
  ENABLE_CLAUDEAI_MCP_SERVERS=false \
    claude --strict-mcp-config --mcp-config coffer-only.json --tools ""
  ```

  - `--strict-mcp-config` uses only the servers in `--mcp-config` and ignores every
    other MCP configuration, such as your user, project and local servers.
  - `--tools ""` turns off every built-in tool: the shell, file tools, `WebFetch` and
    `WebSearch`. MCP tools aren't affected, so Coffer's tools stay.
  - `ENABLE_CLAUDEAI_MCP_SERVERS=false` keeps out the connectors you've added on
    claude.ai *(documentation only)*.

  If you want some built-in tools, name them instead (`--tools "Read"`), or keep the
  defaults and remove the web tools with `--disallowedTools WebFetch WebSearch`. That
  still leaves the shell, which can reach the network. These options each take a list,
  so put nothing after them: a prompt there is read as another file or tool name. *(Checked:
  `claude --help`, and Claude Code accepted these flags with this file. A session can't
  be started without signing in, so the session itself wasn't checked.)*
  `claude mcp list` ignores `--mcp-config`. To check the file, use it as a project's
  `.mcp.json` once (see "Check it works" above).
- **Claude Desktop** *(documentation only)*: keep Coffer the only entry under
  `mcpServers`, turn off other extensions under Settings → Desktop app → Extensions, and
  turn off web search and other connectors (**+** → **Connectors**) in the conversations
  where you use Coffer.
- **Codex**: profiles (`--profile`) only add settings on top of `~/.codex/config.toml`,
  so they can't remove servers already set up there. Give Coffer a Codex home of its
  own instead, say `~/.codex-coffer`, and start Codex with `CODEX_HOME=~/.codex-coffer codex`.
  Its `config.toml` *(checked: Codex accepts these settings and rejects invalid values;
  what they do comes from its documentation)*:

  ```toml
  web_search = "disabled"     # the default, "cached", is on
  sandbox_mode = "read-only"  # commands run without network access

  [features]
  apps = false                # no ChatGPT connectors

  [mcp_servers.coffer]
  command = "/path/to/coffer"
  args = ["mcp", "--client", "codex"]

  [mcp_servers.coffer.env]
  COFFER_DATA_DIR = "/path/to/your/coffer/data"
  ```

  Codex keeps its login in its home too, so sign in once with
  `CODEX_HOME=~/.codex-coffer codex login`. Web search and connectors aren't covered by
  the sandbox's network rules, which is why they're turned off here on their own
  *(documentation only)*.

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
