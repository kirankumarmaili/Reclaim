# MCP tool examples

Copy-paste requests for `reclaim-mcp`. Two ways to send them:

- **Web UI (recommended):** `./run inspect`, pick a tool, fill the form, **Run**.
  The form is generated from each tool's schema; the raw JSON-RPC is shown too.
- **Headless CLI:** the same Inspector without a browser:

  ```bash
  cargo build -p reclaim-mcp
  echo '{"mcpServers":{"reclaim":{"command":"'"$PWD"'/target/debug/reclaim-mcp"}}}' > /tmp/reclaim-mcp.json
  npx -y @modelcontextprotocol/inspector@2.8.0 --cli --config /tmp/reclaim-mcp.json \
    --server reclaim --method tools/call --tool-name hash --tool-arg input=abc algo=sha256
  ```

> **CLI quirk:** `--tool-arg` values are JSON-parsed. A string that looks like JSON
> or a number must be quoted as a JSON string, e.g. `'json="{\"a\":1}"'` or
> `'value="0"'`. Without that you get `invalid type: map, expected a string`.
> The web UI does not have this problem.

The Inspector is a dev-time tool: `npx` downloads it on first use. The
`reclaim-mcp` binary itself makes no network calls.

## Pure utilities (safe to run freely)

| Tool | Arguments | Expect |
|---|---|---|
| `hash` | `input=abc`, `algo=sha256` | `hex` = `ba7816bf…0015ad` |
| `hash` | `input=abc`, `algo=md5` | `hex` = `900150983cd24fb0d6963f7d28e17f72` |
| `encode` | `input=hello`, `scheme=base64` | `output` = `aGVsbG8=` |
| `decode` | `input=aGVsbG8=`, `scheme=base64` | `text` = `hello`, `hex` = `68656c6c6f` |
| `json_prettify` | `json={"a":1,"b":[true,null]}` | indented JSON in `pretty` |
| `json_validate` | `json={"a":` | `valid: false`, `error.line` 1, `error.column` 5 |
| `json_compare` | `left={"a":1}`, `right={"a":2}` | `equal: false`, one `changed` diff at `/a` |
| `time_convert` | `value=0`, `from=epoch_s`, `to_tz=UTC` | `rfc3339` = `1970-01-01T00:00:00+00:00` |
| `time_diff` | `a=0`, `b=3600` | `seconds` = 3600, `human` = `0d 1h 0m 0s` |
| `time_now` | `tz=UTC` | current time in RFC3339 |

## Disk tools

These act on your real filesystem. Go in this order.

1. **`scan_disk`** (read-only). Optional `root` (defaults to `~/Library`). Returns
   items with `id`, `path`, `bytes`, `risk` (`safe` / `review` / `protected`) and a
   `rationale`.
2. **`propose_reclaim`** (read-only). Optional `root`, `policy` = `shadow`
   (default) / `assisted` / `auto_safe`. Only Safe-class items are ever proposed.
3. **`reclaim_space`** (**changes your disk**). `ids` (required), optional
   `root`, `mode` = `trash` (default, reversible) or `permanent` (irreversible;
   opt-in). Passing a real Safe/Review id moves it to the Trash. Start with the
   refusal examples below.

### See the safety gate refuse

`reclaim_space` reports refused ids in a `skipped` list (the call itself still
succeeds); nothing is deleted for a refused id.

- **Unknown id:** `ids=["does-not-exist"]` → `skipped[0].reason` =
  `skipped: no longer present in scan (already gone or never existed)`;
  `moved_to_trash` and `permanently_deleted` are empty.
- **Protected item:** run `scan_disk`, copy the `id` of any item whose `risk` is
  `protected`, then call `reclaim_space` with `ids=["<that id>"]`. It comes back in
  `skipped` with `reason` = `rejected: item is Protected and can never be deleted`;
  nothing is moved.

The `$HOME` boundary is enforced per item at delete time (`root` itself is not
restricted for scanning), so an item outside `$HOME` can never be deleted either.
