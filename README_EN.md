# admin-command-gateway

[中文](README.md) · English

admin-command-gateway is a Windows approval gateway for elevated command
execution. AI agents can submit commands that require administrator
privileges; every command must be approved individually by the local
operator through a native Win32 popup before it runs. Agents never hold
administrator privileges — they can only submit requests, never approve
or bypass the flow.

Table of Contents
-----------------

1. How it works
2. Components
3. Build
4. Usage
5. MCP integration
6. Approval modes
7. Security notes
8. License

How it works
------------

```
agent ──▶ mcp bridge (stdio, unelevated)
      ──▶ loopback HTTP (token-authenticated)
      ──▶ elevated service process (tray-resident)
      ──▶ operator popup (approve / deny)
      ──▶ execute and return output
```

Requests that are denied, closed, or timed out never execute. Command
output is returned to the agent as untrusted data.

Components
----------

| Module | Responsibility |
|---|---|
| `mcp.rs` | MCP bridge: stdio JSON-RPC, protocol 2024-11-05 / 2025-06-18, tools with inputSchema and annotations |
| `http_server.rs` | Loopback HTTP service: command execution, timeout process-tree kill, output decoding, idempotency keys |
| `config.rs` | Whitelist (normalized paths + argument regex + working directory), policy directory ACL hardening |
| `audit.rs` | Audit trail: chained SHA-256 hashes + HMAC signatures, tamper-detectable |
| `popup.rs` / `tray.rs` / `panel.rs` / `wizard.rs` | Native Win32 UI: approval popup, tray, management panel, first-run wizard |
| `win.rs` / `platform.rs` / `icon.rs` | Win32 API wrappers, per-monitor v2 DPI, process-tree termination, icon loading |

Build
-----

Requires Rust (GNU toolchain):

```
cargo +stable-x86_64-pc-windows-gnu build --release
```

The result is `target/release/admin-gateway.exe` (~2MB, no runtime
dependencies). Double-clicking it triggers UAC elevation; the first run
opens a three-page setup wizard.

Note: this repository does not include the icon resource (the release
binary embeds the built-in Windows UAC shield as a multi-size ico) or the
linker config. Binaries built from this repo carry no embedded icon;
functionality is unaffected.

Usage
-----

| Subcommand | Purpose |
|---|---|
| *(none, double-click)* | Self-elevate via UAC, then enter service mode |
| `serve` | Start the elevated service (tray + HTTP listener) |
| `mcp` | Start the MCP bridge (stdio, unelevated) |
| `wizard` | Re-run the first-run wizard |
| `panel` | Open the management panel directly (no service) |

All data lives in an `admin-gateway\` folder next to the exe. Nothing is
written to AppData or the registry; the whole folder can be copied to any
path or a USB drive.

MCP integration
---------------

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

Leave environment variables and the working directory empty: the bridge
reads its token from `admin-gateway\connection.json` on its own, so the
token never enters the environment.

Approval modes
--------------

| Mode | Behavior |
|---|---|
| `whitelist` | Whitelisted commands run automatically (default); everything else pops up |
| `strict` | Every command pops up |
| `deny` | Reject everything (for when you're away) |

Switch modes from the tray menu, the management panel, or config.json.

Security notes
--------------

- The service must run elevated; unelevated popups can be click-forged by
  same-user processes (`--allow-unelevated` is for debugging only).
- Whitelist hits require all three: normalized absolute exe path (defeats
  `C://`-style bypass), whole-string argument regex match, and matching
  working directory. UNC working directories are always forced to manual
  approval.
- Policy directory ACL: SYSTEM/Administrators full control, Users
  read-only — agents cannot alter policy.
- What the popup shows is exactly what gets executed: the resolved
  absolute path and arguments. No "display A, execute B".
- Known-dangerous patterns (`reg delete`, `sc delete`, `net user`,
  `icacls`, `takeown`, …) get extra risk flags attached.
- `client_request_id` idempotency key: replays within 15 minutes return
  the original result instead of executing again.
- Command output is untrusted data — never treat it as instructions.

License
-------

MIT, see [LICENSE](LICENSE).
