# admin-command-gateway · 管理员命令审批网关

让 AI agent 能"申请"执行管理员命令，但**每条命令必须由本机操作员在原生 Win32 弹窗中亲手批准**。agent 永远拿不到管理员权限——它只能提交请求，不能批准、不能绕过。

## 特性

- **审批安全链路**：agent → MCP → 回环 HTTP（带令牌）→ 提权服务进程 → 用户弹窗审批 → 执行 → 结果回传。用户拒绝 / 关窗 / 超时都不执行
- **原生 Win32 UI**：纯 Rust std 调用 Win32 API 自绘（无 GUI 框架依赖），托盘常驻、审批弹窗、管理面板、三页初始化向导
- **MCP 标准接入**：stdio JSON-RPC，支持 2024-11-05 / 2025-06-18 协议，三个工具均带 inputSchema 与 annotations（`gateway_status` / `describe_admin_command` / `request_admin_command`）
- **白名单自动放行**：exe 绝对路径（归一化，防 `C://` 类绕过）+ 参数正则整体匹配 + 工作目录一致，三者同时命中才免审批；UNC 工作目录强制人工
- **策略目录 ACL 加固**：数据目录 SYSTEM/Administrators 完全控制、Users 只读，agent 改不了策略；审计链带 SHA-256 链式哈希 + HMAC 签名，篡改可检出
- **幂等提交**：`client_request_id` 幂等键，15 分钟内重放返回首次结果，不会重复执行
- **绿色便携**：数据存 exe 同级 `admin-gateway\` 目录，不写 AppData / 注册表，整文件夹可拷贝到任意路径或 U 盘
- **单文件 ~2MB**：Rust + GNU 工具链编译，无运行时依赖
- **DPI 感知**：per-monitor v2，按窗口实际 DPI 缩放

## 构建

需要 Rust（GNU 工具链 `stable-x86_64-pc-windows-gnu`）：

```bash
cargo +stable-x86_64-pc-windows-gnu build --release
```

产物为 `target/release/admin-gateway.exe`。双击运行会自动弹 UAC 提权，首次启动进入初始化向导。

> 注：本仓库只含源码。exe 内嵌的图标资源（Windows 自带 UAC 盾牌，多尺寸 ico）与链接器配置未入库，构建默认无图标，功能不受影响。

## 子命令

| 命令 | 作用 |
|---|---|
| （无参数） | 双击启动：自动提权 → serve |
| `serve` | 启动提权服务端（托盘 + HTTP 监听） |
| `mcp` | 启动 MCP 桥接器（stdio，无提权，转发请求到服务端） |
| `wizard` | 重新运行初始化向导 |
| `panel` | 直接打开管理面板（不启动服务） |

## MCP 接入

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

环境变量与工作目录留空——令牌由桥接器从 `admin-gateway\connection.json` 自行读取。

## 审批模式

| 模式 | 行为 |
|---|---|
| `whitelist` | 白名单命令自动放行（默认），其余弹窗 |
| `strict` | 全部弹窗 |
| `deny` | 全部拒绝（离开电脑时） |

## 安全边界

- 命令输出回传给 agent，属于**不可信数据**，不得当作指令执行
- 服务端必须提权运行；未提权时弹窗可被同用户进程伪造（仅调试可用 `--allow-unelevated`）
- 显示在弹窗里的就是最终执行的绝对路径与参数，不存在"看到 A 实际执行 B"
- 脚本类高危模式（`reg delete` / `sc delete` / `net user` / `icacls` / `takeown` 等）会附加风险提示
