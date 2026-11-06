# The MCP server (for coding agents)

`gruff mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server on standard
input and output. It gives a coding agent the language server's knowledge of a GSC workspace
as a handful of **read-only** tools: symbols, definitions, references, callers and callees,
diagnostics, events, builtins and hashes. It is a thin layer (`crates/gsc_mcp`) over the same
core as `gruff lsp`: `gsc_sema` for names and calls, `gsc_check` for diagnostics (incremental, as
in the language server), the workspace event tables, the gruff-data builtin catalogue and
`gsc_hash`.

The answers are written for small contexts (a local 27B model with a few thousand tokens to
spare): plain text, one fact per line, `path:line` locations, pages of 25 items by default
(`limit` up to 200, `offset` to continue, named on the last line), and at most about 8,000
characters per answer.

```sh
gruff mcp [FOLDER...] [--game t7] [--data-dir ../gruff-data] [--library /path/to/game/scripts]...
        [--mode retail|community] [--rescan 2]
```

The folders default to the working directory. The game, data directory, library roots and mode
are resolved exactly as for the language server (setting, then `gruff.toml`, then detection; see
[the language server](lsp.md#settings)). The workspace loads in the background as soon as the
server starts; the first tool call waits for it. Before each tool call the server looks for
files changed, added or removed on disk (at most every `--rescan` seconds), so an agent that
edits files sees its changes on the next call.

## Tools

| Tool | Arguments | Answer |
|---|---|---|
| `gsc_workspace` | | Game and where it came from, roots with their labels and file counts, definition counts, diagnostics by code. Call it first. |
| `gsc_find_symbol` | `query`, `kind`, paging | Definitions whose name matches (exact, prefix, substring; `ns::` matches qualified names): `function ns::f( a, b )  path:line`. |
| `gsc_definition` | `name`, or `file` + `line` (+ `column`) | Signature, flags, doc summary and location of every definition of the name; locals and parameters by position. |
| `gsc_references` | `name` (functions, methods, macros, builtins, `.field`) or a position, paging | `path:line  <the line's code>` per use. |
| `gsc_callers` | `name` or a position, paging | `caller  path:line,line`: who calls (or takes a pointer to) the function. |
| `gsc_callees` | `name` or a position, paging | Script callees with their definition, builtins, unresolved and dynamic calls, with lines. |
| `gsc_check` | `path` (file or directory), `codes` (`B002,L`), paging | `path:line:col CODE message`, with counts by code. Library roots are not checked. |
| `gsc_events` | `name`, `class` (`notify`, `wait`, `endon`), paging; or `query` | The sites of an event (pattern notifies such as `"trigger_" + n` included) with receiver, payload and enclosing function, and whether the engine catalogue sends it. Without `name`: event names with their counts. |
| `gsc_builtin` | `name` | Per VM and kind: parameter types, arity, and how it was verified (engine table, handler emulation, corpus correction, `max_checked = false`, partial catalogue, name source). Suggestions for a misspelling; script functions of the same name. |
| `gsc_hash` | `text` or `value`, `family` | A string hashed with each of the game's families, or a value (`0x...`, `function_1a2b3c4d`, `hash_...`) named from every name the workspace and the catalogue know (definitions, namespaces, file paths, fields, events, builtins). |

Names are `ns::function`, `function`, `class::method`, a macro or a builtin name; `.name` is a
field. Paths are printed relative to the first folder when under it, else as
`<root label>:<path in the root>` (the labels are in `gsc_workspace`); both forms, absolute
paths and unique path suffixes are accepted as arguments. Every tool is annotated
`readOnlyHint`; errors an agent can fix (an unknown name, an ambiguous path) come back as tool
results with `isError`.

## Clients

### opencode

In `opencode.json` (the project's, or `~/.config/opencode/opencode.json`):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "gruff": {
      "type": "local",
      "command": ["gruff", "mcp", "--game", "t7", "--library", "/path/to/share/raw/scripts"],
      "enabled": true
    }
  }
}
```

opencode starts the server in the project directory, which becomes the workspace folder. With a
small local model, keep the default page size; ask for `gsc_workspace` first, then
`gsc_find_symbol` and `gsc_definition` rather than reading whole files.

### Claude Code

```sh
claude mcp add gruff -- gruff mcp --game t7 --library /path/to/share/raw/scripts
```

or, shared with the repository, in `.mcp.json` at its root:

```json
{
  "mcpServers": {
    "gruff": {
      "command": "gruff",
      "args": ["mcp", "--game", "t7", "--library", "/path/to/share/raw/scripts"]
    }
  }
}
```

## Protocol

Newline-delimited JSON-RPC 2.0 on stdio; nothing but protocol messages on standard output. The
server is *dual-era*:

- **Modern** (revision `2026-07-28`): every request carries its version in
  `_meta["io.modelcontextprotocol/protocolVersion"]` and is served statelessly; results carry
  `resultType: "complete"`. `server/discover` returns the supported versions, the `tools`
  capability, the server info and short instructions. An unsupported version gets
  `UnsupportedProtocolVersionError` (`-32022`, with `supported` and `requested`).
- **Legacy** (`2025-11-25`, `2025-06-18`, `2025-03-26`, `2024-11-05`): `initialize` negotiates
  one of them (the client's when supported, else `2025-11-25`), then `tools/list`, `tools/call`
  and `ping` as usual. Requests without `_meta` are served the same way, initialized or not.

`tools/list` returns the ten tools in a fixed order. Unknown tools are a protocol error
(`-32602`), unknown methods `-32601`. The protocol layer is written by hand over `serde_json`
(MIT or Apache-2.0, compatible with the GPL-3.0-or-later of Gruff): a dozen methods do not
justify an SDK and its async runtime.

## Tests

`cargo test -p gruff --test mcp_stdio` starts the real `gruff mcp` binary on a copy of the
language server's fixture mod and drives both eras over stdio: discovery, a refused version,
the tool list, every tool, an edit on disk picked up by the next call, the legacy handshake.
