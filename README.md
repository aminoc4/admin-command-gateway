# admin-command-gateway

中文 · [English](README_EN.md)

admin-command-gateway 是一个 Windows 管理员命令审批网关。AI agent 可以提交
需要管理员权限的命令，每条命令都必须由本机操作员在原生 Win32 弹窗中逐条
批准后才会执行。agent 始终不持有管理员权限，只能提交请求，无法批准或绕过
审批流程。

目录
----

1. 工作原理
2. 组件
3. 构建
4. 使用
5. MCP 接入
6. 审批模式
7. 安全边界
8. 许可证

工作原理
--------

```
agent ──▶ mcp（stdio 桥接，无提权）
      ──▶ 回环 HTTP（令牌认证）
      ──▶ 提权服务进程（托盘常驻）
      ──▶ 操作员弹窗（批准 / 拒绝）
      ──▶ 执行并回传结果
```

未获批准（拒绝、关闭窗口、超时）的请求不会执行。命令输出按不可信数据回传。

组件
----

| 模块 | 职责 |
|---|---|
| `mcp.rs` | MCP 桥接器：stdio JSON-RPC，协议 2024-11-05 / 2025-06-18，工具带 inputSchema 与 annotations |
| `http_server.rs` | 回环 HTTP 服务：命令执行、超时杀进程树、输出解码、幂等键 |
| `config.rs` | 白名单（路径归一化 + 参数正则 + 工作目录）、策略目录 ACL 加固 |
| `audit.rs` | 审计链：SHA-256 链式哈希 + HMAC 签名，篡改可检出 |
| `popup.rs` / `tray.rs` / `panel.rs` / `wizard.rs` | 原生 Win32 界面：审批弹窗、托盘、管理面板、初始化向导 |
| `win.rs` / `platform.rs` / `icon.rs` | Win32 API 封装、per-monitor v2 DPI、进程树终止、图标加载 |

构建
----

需要 Rust（GNU 工具链）：

```
cargo +stable-x86_64-pc-windows-gnu build --release
```

产物为 `target/release/admin-gateway.exe`（约 2MB，无运行时依赖）。
双击运行自动弹 UAC 提权，首次启动进入三页初始化向导。

注：本仓库不含图标资源（发布版内嵌系统 UAC 盾牌多尺寸 ico）与链接器配置，
从源码构建得到的产物没有内嵌图标，功能不受影响。

使用
----

| 子命令 | 作用 |
|---|---|
| （无参数，双击） | 自动提权后进入服务模式 |
| `serve` | 启动提权服务端（托盘 + HTTP 监听） |
| `mcp` | 启动 MCP 桥接器（stdio，无提权） |
| `wizard` | 重新运行初始化向导 |
| `panel` | 直接打开管理面板（不启动服务） |

数据存放在 exe 同级的 `admin-gateway\` 目录，不写 AppData 与注册表，
整个文件夹可拷贝到任意路径或 U 盘。

MCP 接入
--------

```json
{
  "mcpServers": {
    "admin": {
      "command": "<exe 绝对路径>\\admin-gateway.exe",
      "args": ["mcp"]
    }
  }
}
```

环境变量与工作目录留空：令牌由桥接器从 `admin-gateway\connection.json`
自行读取，不进环境变量。

审批模式
--------

| 模式 | 行为 |
|---|---|
| `whitelist` | 白名单命令自动放行（默认），其余弹窗 |
| `strict` | 全部弹窗 |
| `deny` | 全部拒绝（离开电脑时） |

在托盘右键菜单「审批模式」子菜单、管理面板或 config.json 中切换。

安全边界
--------

- 服务端必须提权运行；未提权时弹窗可被同用户进程伪造点击
  （`--allow-unelevated` 仅供调试，切勿日常使用）。
- 白名单要求 exe 绝对路径（归一化，防 `C://` 类绕过）、参数正则整体匹配、
  工作目录一致三者同时命中；UNC 工作目录强制转人工。
- 策略目录 ACL：SYSTEM/Administrators 完全控制，Users 只读，agent 改不了策略。
- 弹窗显示的就是最终执行的绝对路径与参数，不存在"看到 A 实际执行 B"。
- 脚本类高危模式（`reg delete`、`sc delete`、`net user`、`icacls`、
  `takeown` 等）附加风险提示。
- `client_request_id` 幂等键：15 分钟内重放返回首次结果，不会重复执行。
- 命令输出属于不可信数据，不得当作指令执行。

许可证
------

MIT，见 [LICENSE](LICENSE)。
