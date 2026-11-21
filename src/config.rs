use crate::win;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::os::windows::process::CommandExt;

/// CREATE_NO_WINDOW：GUI 进程里拉起控制台程序（icacls 等）时不能闪出黑窗口。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WhitelistEntry {
    pub name: String,
    pub exe: String,
    pub arg_regex: String,
    /// 允许的工作目录；留空表示"必须继承父进程目录"。不匹配就转人工。
    pub cwd: String,
    pub note: String,
}

impl Default for WhitelistEntry {
    fn default() -> Self {
        WhitelistEntry { name: String::new(), exe: String::new(), arg_regex: String::new(), cwd: String::new(), note: String::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: String,
    pub popup_timeout_sec: u64,
    pub exec_timeout_sec: u64,
    pub max_output_chars: usize,
    pub max_in_flight: usize,
    /// 仅用于自动化测试：approve / deny，非空时跳过弹窗。面板会显著告警。
    pub dev_auto_decide: String,
    pub whitelist: Vec<WhitelistEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: "whitelist".into(),
            popup_timeout_sec: 300,
            exec_timeout_sec: 120,
            max_output_chars: 8000,
            max_in_flight: 5,
            dev_auto_decide: String::new(),
            whitelist: default_whitelist(),
        }
    }
}

pub fn default_whitelist() -> Vec<WhitelistEntry> {
    vec![
        WhitelistEntry {
            name: "查看网络配置".into(),
            exe: r"C:\Windows\System32\ipconfig.exe".into(),
            arg_regex: r"(/all|/displaydns)?".into(),
            cwd: String::new(),
            note: "只读，不改配置".into(),
        },
        WhitelistEntry {
            name: "刷新 DNS 缓存".into(),
            exe: r"C:\Windows\System32\ipconfig.exe".into(),
            arg_regex: r"/flushdns".into(),
            cwd: String::new(),
            note: "会清除本机 DNS 缓存".into(),
        },
        WhitelistEntry {
            name: "查看进程列表".into(),
            exe: r"C:\Windows\System32\tasklist.exe".into(),
            arg_regex: String::new(),
            cwd: String::new(),
            note: "只读".into(),
        },
        WhitelistEntry {
            name: "查看系统信息".into(),
            exe: r"C:\Windows\System32\systeminfo.exe".into(),
            arg_regex: String::new(),
            cwd: String::new(),
            note: "只读".into(),
        },
    ]
}

/// 策略、审计、密钥、连接文件都在程序目录下的 data/ 里，整个文件夹可以任意搬动。
/// ACL 收紧为：SYSTEM / Administrators 完全控制，Users 只读。
pub fn home_dir() -> PathBuf {
    if let Ok(v) = env::var("ADMIN_GATEWAY_HOME") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    // 就地保存：exe 旁边与程序同名的小写文件夹，整个目录可以整体搬走
    executable_dir().join("admin-gateway")
}

/// 连接文件与策略同目录；Users 有读权限，普通权限的桥接器才读得到端口与令牌。
pub fn ipc_dir() -> PathBuf {
    home_dir()
}

pub fn executable_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    home_dir().join("config.json")
}

pub fn audit_path() -> PathBuf {
    home_dir().join("audit.jsonl")
}

pub fn key_path() -> PathBuf {
    home_dir().join("audit.key")
}

pub fn log_path() -> PathBuf {
    home_dir().join("executor.log")
}

pub fn conn_path() -> PathBuf {
    ipc_dir().join("connection.json")
}

/// 收紧目录权限：去掉继承，SYSTEM/Administrators 完全控制，Users 只读。
/// 目录由提权进程创建，属主是 Administrators，普通权限进程无法改回 ACL。
pub fn harden_acl(dir: &Path) -> bool {
    let target = dir.to_string_lossy().to_string();
    let out = Command::new("icacls")
        .args([
            &target,
            "/inheritance:r",
            "/grant:r",
            "*S-1-5-18:(OI)(CI)F",
            "*S-1-5-32-544:(OI)(CI)F",
            "*S-1-5-32-545:(OI)(CI)RX",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

pub fn acl_snapshot(dir: &Path) -> String {
    match Command::new("icacls")
        .arg(dir.to_string_lossy().to_string())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => "无法读取 ACL".into(),
    }
}

/// 检查是否还有普通用户可写的 ACE。
/// icacls 的输出是本地化组名而不是 SID，所以不能按名字判断；
/// 只数"带写标志的 ACE 条数"：正常只有 SYSTEM 与 Administrators 两条。
pub fn acl_is_loose(dir: &Path) -> bool {
    let text = acl_snapshot(dir).to_lowercase();
    let mut write_aces = 0;
    for line in text.lines() {
        if line.contains("(f)") || line.contains("(m)") || line.contains("(w)") {
            write_aces += 1;
        }
    }
    write_aces > 2
}

pub fn ensure_home(elevated: bool) -> Result<(), String> {
    let dir = home_dir();
    if !dir.exists() {
        // 老版本用过 exe 同级的 data\ 和 AdminGateway\，优先原样搬过来，
        // 别让已有的白名单、审计记录丢失
        for name in ["data", "AdminGateway"] {
            let legacy = executable_dir().join(name);
            if legacy.is_dir() {
                if fs::rename(&legacy, &dir).is_err() {
                    let _ = copy_tree(&legacy, &dir);
                }
                break;
            }
        }
    }
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("无法创建策略目录 {}: {}", dir.display(), e))?;
    }
    let dev = env::var("ADMIN_GATEWAY_HOME").map(|v| !v.is_empty()).unwrap_or(false);
    if elevated && !dev {
        if !harden_acl(&dir) {
            return Err(format!("无法收紧 {} 的访问控制", dir.display()));
        }
        if acl_is_loose(&dir) {
            return Err(format!(
                "{} 仍对普通用户可写，拒绝启动（策略必须只有管理员能改）",
                dir.display()
            ));
        }
    }
    let ipc = ipc_dir();
    if !ipc.exists() {
        fs::create_dir_all(&ipc).map_err(|e| format!("无法创建通信目录 {}: {}", ipc.display(), e))?;
    }
    Ok(())
}

/// 首次运行时落一份默认策略，方便用户直接看/改。
pub fn ensure_config_file() {
    if config_path().exists() {
        return;
    }
    migrate_legacy();
    if !config_path().exists() {
        let _ = save_config(&Config::default());
    }
}

/// 首次迁移：优先取旧版放在 exe 同级或 ProgramData 下的配置（只迁白名单）。
pub fn migrate_legacy() {
    if config_path().exists() {
        return;
    }
    let mut legacy = executable_dir().join("config.json");
    if !legacy.exists() {
        let older = executable_dir().join("data").join("config.json");
        if older.exists() {
            legacy = older;
        }
    }
    if !legacy.exists() {
        if let Ok(v) = env::var("ProgramData") {
            let alt = PathBuf::from(v).join("AdminGateway").join("config.json");
            if alt.exists() {
                legacy = alt;
            }
        }
    }
    if !legacy.exists() {
        return;
    }
    if let Ok(text) = fs::read_to_string(&legacy) {
        if let Ok(old) = serde_json::from_str::<Value>(&text) {
            let mut cfg = Config::default();
            if let Some(list) = old.get("whitelist").and_then(|v| v.as_array()) {
                let mut migrated = Vec::new();
                for item in list {
                    if let Ok(mut entry) = serde_json::from_value::<WhitelistEntry>(item.clone()) {
                        entry.cwd = entry.cwd.trim().to_string();
                        if !entry.exe.trim().is_empty() {
                            migrated.push(entry);
                        }
                    }
                }
                if !migrated.is_empty() {
                    cfg.whitelist = migrated;
                }
            }
            let _ = save_config(&cfg);
        }
    }
}

pub fn load_config() -> Config {
    match fs::read_to_string(config_path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

pub fn save_config(cfg: &Config) -> Result<(), String> {
    let mut out = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    out.push('\n');
    let tmp = config_path().with_extension("json.tmp");
    fs::write(&tmp, out).map_err(|e| format!("写入临时配置失败: {}", e))?;
    fs::rename(&tmp, config_path()).map_err(|e| format!("替换配置失败: {}", e))
}

pub fn update_mode(mode: &str) -> Result<(), String> {
    let mut cfg = load_config();
    cfg.mode = mode.to_string();
    save_config(&cfg)
}

pub fn audit_key() -> [u8; 32] {
    if let Ok(bytes) = fs::read(key_path()) {
        if bytes.len() >= 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes[..32]);
            return key;
        }
    }
    let hex = crate::random_hex(32);
    let bytes = hex.as_bytes();
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes[..32]);
    let _ = fs::write(key_path(), key);
    key
}

#[derive(Clone, Debug, Serialize)]
pub struct Assessment {
    /// 解析后的绝对路径，实际执行的就是它
    pub exe: String,
    /// 提交时的原始写法
    pub requested: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub whitelisted: bool,
    pub whitelist_name: String,
    pub whitelist_note: String,
    pub risk_flags: Vec<String>,
    pub error: String,
}

const RISK_RULES: [(&str, &str); 6] = [
    (r"(?i)\b(del|erase|rmdir|format|diskpart|cipher|vssadmin|wbadmin|bcdedit|bootrec)\b", "磁盘或文件破坏性操作"),
    (r"(?i)\b(reg\s+delete|sc\s+delete|net\s+user|icacls|takeown|subinacl)\b", "账户、注册表或权限变更"),
    (r"(?i)\b(taskkill|stop-process|shutdown|restart-computer|logoff)\b", "终止进程或关机"),
    (r"(?i)(-enc|-encodedcommand|iex|invoke-expression|frombase64string)", "脚本动态执行"),
    (r"[|&;<`$()]", "含 Shell 特殊字符"),
    (r"(?i)\.(bat|cmd|ps1|vbs|js|hta)$", "脚本文件而非可执行程序"),
];

/// 归一化路径用于比较：统一分隔符、合并重复分隔符、去 \\?\ 前缀与尾斜杠、小写。
/// 不合并重复分隔符会让 `C://Windows//x.exe` 这类写法静默绕过白名单，必须处理。
fn normalize(path: &str) -> String {
    let raw = path.trim().replace('\\', "/");
    let mut merged = String::with_capacity(raw.len());
    let mut prev_slash = false;
    for (index, ch) in raw.chars().enumerate() {
        if ch == '/' {
            if prev_slash && index > 1 {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        merged.push(ch);
    }
    let lower = merged.to_ascii_lowercase();
    let lower = lower.strip_prefix("//?/").map(|s| s.to_string()).unwrap_or(lower);
    let mut out = lower;
    while out.ends_with('/') && out.len() > 2 {
        out.pop();
    }
    out
}

pub fn assess(command: &str, args: &[String], cwd: &str, cfg: &Config) -> Assessment {
    let requested = command.trim().to_string();
    let resolved = win::resolve_executable(command);
    let (exe, error) = match resolved {
        Ok(p) => (p, String::new()),
        Err(e) => (requested.clone(), e),
    };

    let mut flags = Vec::new();
    let base = Path::new(&exe)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| exe.clone());
    let mut probe = base.clone();
    probe.push(' ');
    probe.push_str(&args.join(" "));
    for (pattern, label) in RISK_RULES {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(&probe) {
                flags.push(label.to_string());
            }
        }
    }
    if cwd.trim().starts_with("\\\\") {
        flags.push("工作目录是 UNC 网络路径，可能泄露凭据".into());
    }
    if exe.starts_with("\\\\") {
        flags.push("可执行文件位于网络路径".into());
    }

    let arg_text = args.join(" ");
    let cwd_norm = normalize(cwd);
    let exe_norm = normalize(&exe);
    let mut whitelisted = false;
    let mut name = String::new();
    let mut note = String::new();
    if error.is_empty() && !cwd.trim().starts_with("\\\\") {
        for entry in &cfg.whitelist {
            if normalize(&entry.exe) != exe_norm {
                continue;
            }
            if normalize(&entry.cwd) != cwd_norm {
                continue;
            }
            let pattern = format!("^(?:{})$", entry.arg_regex);
            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(&arg_text) {
                    whitelisted = true;
                    name = entry.name.clone();
                    note = entry.note.clone();
                    break;
                }
            }
        }
    }

    Assessment {
        exe,
        requested,
        args: args.to_vec(),
        cwd: cwd.to_string(),
        whitelisted,
        whitelist_name: name,
        whitelist_note: note,
        risk_flags: flags,
        error,
    }
}

pub fn assessment_json(a: &Assessment) -> Value {
    json!({
        "exe": a.exe,
        "requested": a.requested,
        "args": a.args,
        "cwd": a.cwd,
        "whitelisted": a.whitelisted,
        "whitelist_name": a.whitelist_name,
        "whitelist_note": a.whitelist_note,
        "risk_flags": a.risk_flags,
        "error": a.error
    })
}

/// 逐项复制（跨卷或权限差异导致 rename 失败时兜底）。
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
