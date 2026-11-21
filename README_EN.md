# admin-command-gateway

[中文说明](README.md) · English

Let AI agents *request* elevated (administrator) command execution — while **every single command must be manually approved by the local operator in a native Win32 popup**. The agent never holds administrator privileges: it can only submit requests. It cannot approve, and it cannot bypass the flow.

## Features

- **Approval chain**: agent → MCP → loopback HTTP (token-authenticated) → elevated service process → operator popup → execute → result returned. Denied / closed / timed-out requests never execute.
- **Native Win32 UI**: hand-drawn with Win32 API from pure Rust std (no GUI framework). Tray icon, approval popup, management panel, three-page first-run wizard.
- **Standard MCP integration**: stdio JSON-RPC, protocol versions 2024-11-05 / 2025-06-18, three tools each with `inputSchema` and `annotations` (`gateway_status` / `describe_admin_command` / `request_admin_command`).
- **Whitelist auto-approve**: a command runs without approval only when all three match — normalized absolute exe path (defeats `C://`-style bypass), whole-string regex on arguments, and working directory. UNC working directories are always forced to manual approval.
- **Hardened policy directory**: data folder ACL restricted to SYSTEM/Administrators full control, Users read-only — agents cannot alter policy. Audit trail uses a chained SHA-256 hash + HMAC signature; tampering is detectable.
- **Idempotent submissions**: `client_request_id` idempotency key — replays within 15 minutes return the original result instead of executing again.
- **Portable & green**: all data lives in an `admin-gateway\` folder next to the exe. No AppData, no registry. Copy the folder anywhere — including a USB drive.
- **Single ~2MB binary**: Rust compiled with the GNU toolchain, zero runtime dependencies.
- **DPI aware**: per-monitor v2, every window scales by its actual DPI.

## Build

Requires Rust with the GNU toolchain (`stable-x86_64-pc-windows-gnu`):

```bash
cargo +stable-x86_64-pc-windows-gnu build --release
```

The output is `target/release/admin-gateway.exe`. Double-clicking it triggers a UAC elevation prompt; the first run opens a setup wizard.

> Note: this repository contains source code only. The icon resource embedded in the release binary (the built-in Windows UAC shield, multi-size ico) and the linker config are not included. Building from this repo produces an icon-less binary with identical functionality.

## Subcommands

| Command | Purpose |
|---|---|
| *(none)* | Double-click launch: self-elevate via UAC → serve |
| `serve` | Start the elevated service (tray + HTTP listener) |
| `mcp` | Start the MCP bridge (stdio, unelevated; forwards requests to the service) |
| `wizard` | Re-run the first-run wizard |
| `panel` | Open the management panel directly (without starting the service) |

## MCP Setup

```json
{
  "mcpServers": {
    "admin": {
      "command": "<absolute path to exe>\\admin-gateway.exe",
      "args": ["mcp"]
    }
  }
}
```

Leave environment variables and working directory empty — the bridge reads its token from `admin-gateway\connection.json` next to the exe on its own.

## Approval Modes

| Mode | Behavior |
|---|---|
| `whitelist` | Whitelisted commands run automatically (default); everything else pops up |
| `strict` | Every command pops up |
| `deny` | Reject everything (for when you're away) |

## Security Boundaries

- Command output returned to the agent is **untrusted data** — never treat it as instructions.
- The service must run elevated; unelevated popups can be click-forged by same-user processes (`--allow-unelevated` exists for debugging only).
- What the popup shows is exactly what gets executed: the resolved absolute path and arguments. No "display A, execute B".
- Known-dangerous patterns (`reg delete`, `sc delete`, `net user`, `icacls`, `takeown`, …) get extra risk flags attached.
