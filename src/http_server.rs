use crate::audit::{rotate_log, AuditLog};
use crate::config::{
    assess, assessment_json, audit_path, config_path, conn_path, ipc_dir, load_config, log_path, Config,
};
use crate::platform::{decode_console, kill_tree};
use crate::popup::PopupView;
use crate::random_hex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const MAX_BODY: usize = 256 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_CLOCK_SKEW: Duration = Duration::from_secs(900);

static PORT: AtomicU16 = AtomicU16::new(0);

pub fn port() -> u16 {
    PORT.load(Ordering::SeqCst)
}

#[derive(Clone)]
struct CommandRequest {
    id: String,
    exe: String,
    args: Vec<String>,
    cwd: String,
    reason: String,
    risk_flags: Vec<String>,
    whitelist_name: String,
}

struct Pending {
    id: String,
    req: CommandRequest,
    state: Mutex<Option<(String, String)>>,
    cvar: Condvar,
}

impl Pending {
    fn decide(&self, decision: &str, by: &str) -> bool {
        let mut guard = self.state.lock().unwrap();
        if guard.is_some() {
            return false;
        }
        *guard = Some((decision.to_string(), by.to_string()));
        self.cvar.notify_all();
        true
    }

    fn wait_decision(&self, timeout: Duration) -> Option<(String, String)> {
        let (guard, _) = self
            .cvar
            .wait_timeout_while(self.state.lock().unwrap(), timeout, |s| s.is_none())
            .unwrap();
        guard.clone()
    }
}

struct Context {
    token: String,
    pending: Mutex<HashMap<String, Arc<Pending>>>,
    audit: Mutex<AuditLog>,
    running: AtomicBool,
    recent: Mutex<HashMap<String, (Instant, Value)>>,
}

type Reply = (u16, String, Vec<u8>);

fn json_reply(status: u16, value: Value) -> Reply {
    (status, "application/json; charset=utf-8".to_string(), serde_json::to_vec(&value).unwrap())
}

fn ok_message(status: u16, ok: bool, message: &str) -> Reply {
    json_reply(status, json!({"ok": ok, "message": message}))
}

fn has_token(body: &Value, ctx: &Context) -> bool {
    body.get("token").and_then(|v| v.as_str()) == Some(ctx.token.as_str())
}

fn to_string_slice(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|v| v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn truncate_text(text: String, limit: usize) -> String {
    if limit == 0 || text.chars().count() <= limit {
        return text;
    }
    let cut: String = text.chars().take(limit).collect();
    format!("{}\n...[已截断]", cut)
}

fn execute_command(req: &CommandRequest, cfg: &Config) -> Value {
    let timeout = Duration::from_secs(cfg.exec_timeout_sec.max(1));
    let mut command = Command::new(&req.exe);
    command.args(&req.args);
    // 白名单命令大多是控制台程序（net、sc、reg……），GUI 进程里必须压掉控制台窗口，
    // 否则每执行一条命令屏幕上就闪一下黑框。CREATE_NO_WINDOW 对 GUI 程序无副作用。
    command.creation_flags(0x0800_0000);
    if !req.cwd.trim().is_empty() {
        command.current_dir(&req.cwd);
    }
    let mut child = match command.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).spawn() {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "ok": false, "exit_code": -1, "stdout": "", "stderr": format!("执行失败: {}", e),
                "timed_out": false, "duration_ms": 0
            })
        }
    };

    let stdout_handle = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });
    let stderr_handle = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let start = Instant::now();
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    // 连同 cmd /c 派生的子孙进程一起结束
                    kill_tree(child.id());
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                thread::sleep(Duration::from_millis(40));
            }
            Err(_) => break,
        }
    }

    let out_bytes = stdout_handle.and_then(|h| h.join().ok()).unwrap_or_default();
    let err_bytes = stderr_handle.and_then(|h| h.join().ok()).unwrap_or_default();
    let stdout = truncate_text(decode_console(&out_bytes), cfg.max_output_chars);
    let mut stderr = truncate_text(decode_console(&err_bytes), cfg.max_output_chars / 2);
    if timed_out {
        stderr.push_str("\n[执行超时，进程树已被终止]");
    }
    let exit_code = child.try_wait().ok().flatten().and_then(|s| s.code()).unwrap_or(-1);
    json!({
        "ok": !timed_out && exit_code == 0,
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "timed_out": timed_out,
        "duration_ms": start.elapsed().as_millis() as u64
    })
}

fn handle_preview(ctx: &Context, body: &Value) -> Reply {
    if !has_token(body, ctx) {
        return ok_message(401, false, "invalid token");
    }
    let command = body.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let cwd = body.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
    let cfg = load_config();
    let a = assess(command, &to_string_slice(body.get("args")), cwd, &cfg);
    let mode = cfg.mode.clone();
    let timeout = cfg.popup_timeout_sec;
    json_reply(200, json!({
        "ok": true,
        "assessment": assessment_json(&a),
        "mode": mode,
        "popup_timeout_sec": timeout,
        "dev_auto_decide": !cfg.dev_auto_decide.trim().is_empty()
    }))
}

fn handle_submit(ctx: &Context, body: &Value) -> Reply {
    if !has_token(body, ctx) {
        return ok_message(401, false, "invalid token");
    }
    let cfg = load_config();

    // 幂等：同一个 client_request_id 在窗口期内只执行一次
    let idem = body.get("client_request_id").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if !idem.is_empty() {
        let mut cache = ctx.recent.lock().unwrap();
        cache.retain(|_, (at, _)| at.elapsed() < MAX_CLOCK_SKEW);
        if let Some((_, value)) = cache.get(&idem) {
            let mut replay = value.clone();
            if let Some(obj) = replay.as_object_mut() {
                obj.insert("replayed".into(), json!(true));
            }
            return json_reply(200, replay);
        }
    }

    let command = body.get("command").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if command.is_empty() {
        return ok_message(400, false, "command 不能为空");
    }
    let cwd = body.get("cwd").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let mut reason = body.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if reason.chars().count() > 500 {
        reason = reason.chars().take(500).collect();
    }
    let args = to_string_slice(body.get("args"));
    let a = assess(&command, &args, &cwd, &cfg);
    if !a.error.is_empty() {
        return ok_message(400, false, &a.error);
    }

    let id = format!("req_{}", random_hex(6));
    let req = CommandRequest {
        id: id.clone(),
        exe: a.exe.clone(),
        args: args.clone(),
        cwd: cwd.clone(),
        reason: reason.clone(),
        risk_flags: a.risk_flags.clone(),
        whitelist_name: a.whitelist_name.clone(),
    };

    let mut audit_fields = Map::new();
    audit_fields.insert("request_id".into(), json!(id));
    audit_fields.insert("requested".into(), json!(a.requested));
    audit_fields.insert("exe".into(), json!(a.exe));
    audit_fields.insert("args".into(), json!(args));
    audit_fields.insert("cwd".into(), json!(cwd));
    audit_fields.insert("reason".into(), json!(reason));
    audit_fields.insert("whitelisted".into(), json!(a.whitelisted));
    audit_fields.insert("risk_flags".into(), json!(a.risk_flags));

    let dev = cfg.dev_auto_decide.trim().to_string();
    let (decision, by, message): (String, String, String) = if !dev.is_empty() {
        let approved = dev == "approve";
        (
            if approved { "approved".into() } else { "denied".into() },
            format!("dev-auto-{}", dev),
            format!("dev_auto_decide={}，弹窗被跳过（仅用于测试）", dev),
        )
    } else if cfg.mode == "deny" {
        ("denied".into(), "mode-deny-all".into(), "当前为免打扰模式，已自动拒绝".into())
    } else if cfg.mode == "whitelist" && a.whitelisted {
        ("approved".into(), "auto-whitelist".into(), String::new())
    } else {
        let mut wait_sec = cfg.popup_timeout_sec;
        if let Some(v) = body.get("timeout_sec").and_then(|x| x.as_u64()) {
            if v > 0 {
                wait_sec = v;
            }
        }
        wait_sec = wait_sec.clamp(5, 600);

        let mut pending_guard = ctx.pending.lock().unwrap();
        if pending_guard.len() >= cfg.max_in_flight.max(1) {
            return ok_message(429, false, "待审批队列已满，请先处理弹窗");
        }
        let p = Arc::new(Pending { id: id.clone(), req: req.clone(), state: Mutex::new(None), cvar: Condvar::new() });
        pending_guard.insert(id.clone(), p.clone());
        drop(pending_guard);

        let (tx, rx) = mpsc::channel::<(String, String)>();
        let pending_clone = p.clone();
        thread::spawn(move || {
            if let Ok(pair) = rx.recv() {
                pending_clone.decide(&pair.0, &pair.1);
            }
        });
        crate::tray::post_refresh_tip();
        crate::tray::post_popup(PopupView {
            id: id.clone(),
            exe: req.exe.clone(),
            args: req.args.clone(),
            cwd: req.cwd.clone(),
            reason: req.reason.clone(),
            risk_flags: req.risk_flags.clone(),
            whitelist: req.whitelist_name.clone(),
            wait_sec,
            mode: cfg.mode.clone(),
            decision: tx,
        });

        let outcome = p.wait_decision(Duration::from_secs(wait_sec));
        let result = match outcome {
            Some(pair) => pair,
            None => {
                crate::popup::notify_decision(&id, "denied", "timeout");
                p.decide("denied", "timeout");
                ("denied".to_string(), "timeout".to_string())
            }
        };
        ctx.pending.lock().unwrap().remove(&id);
        crate::tray::post_refresh_tip();
        let msg = if result.1 == "timeout" { "等待审批超时，已自动拒绝".to_string() } else { String::new() };
        (result.0, result.1, msg)
    };

    audit_fields.insert("decision".into(), json!(decision));
    audit_fields.insert("decided_by".into(), json!(by));

    let reply = if decision == "denied" {
        let seq = ctx.audit.lock().unwrap().append(audit_fields);
        json!({
            "ok": true, "request_id": id, "status": "denied", "decision_by": by,
            "message": message, "audit_seq": seq
        })
    } else {
        let result = execute_command(&req, &cfg);
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(format!("{}{}", result["stdout"], result["stderr"]).as_bytes());
        audit_fields.insert("exit_code".into(), result["exit_code"].clone());
        audit_fields.insert("timed_out".into(), result["timed_out"].clone());
        audit_fields.insert("duration_ms".into(), result["duration_ms"].clone());
        audit_fields.insert("output_sha256".into(), json!(format!("{:x}", hasher.finalize())));
        let seq = ctx.audit.lock().unwrap().append(audit_fields);
        json!({
            "ok": true, "request_id": id, "status": "approved", "decision_by": by,
            "audit_seq": seq, "result": result
        })
    };

    if !idem.is_empty() {
        ctx.recent.lock().unwrap().insert(idem, (Instant::now(), reply.clone()));
    }
    json_reply(200, reply)
}

fn handle_mode(ctx: &Context, body: &Value) -> Reply {
    if !has_token(body, ctx) {
        return ok_message(401, false, "invalid token");
    }
    let mode = body.get("mode").and_then(|v| v.as_str()).unwrap_or("");
    if !matches!(mode, "whitelist" | "strict" | "deny") {
        return ok_message(400, false, "无效模式");
    }
    match crate::config::update_mode(mode) {
        Ok(_) => json_reply(200, json!({"ok": true, "mode": mode})),
        Err(e) => ok_message(500, false, &e),
    }
}

fn handle_shutdown(ctx: &Context, body: &Value) -> Reply {
    if !has_token(body, ctx) {
        return ok_message(401, false, "invalid token");
    }
    ctx.running.store(false, Ordering::SeqCst);
    crate::tray::request_exit();
    json_reply(200, json!({"ok": true}))
}

fn dispatch(ctx: &Context, method: &str, path: &str, body: &Value) -> Reply {
    match (method, path) {
        ("GET", "/health") => json_reply(200, json!({"ok": true, "mode": load_config().mode, "elevated": crate::ELEVATED.load(Ordering::SeqCst)})),
        ("GET", "/status") => {
            let pending = ctx.pending.lock().unwrap().len();
            let cfg = load_config();
            json_reply(200, json!({
                "ok": true, "mode": cfg.mode, "pending": pending,
                "port": port(), "elevated": crate::ELEVATED.load(Ordering::SeqCst),
                "policy_dir": crate::config::home_dir().display().to_string()
            }))
        }
        ("POST", "/preview") => handle_preview(ctx, body),
        ("POST", "/submit") => handle_submit(ctx, body),
        ("POST", "/mode") => handle_mode(ctx, body),
        ("POST", "/shutdown") => handle_shutdown(ctx, body),
        _ => ok_message(404, false, "not found"),
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |c: u8| -> Option<u8> {
                match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(raw: &str) -> HashMap<String, String> {
    raw.split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn handle_stream(stream: TcpStream, ctx: Arc<Context>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    if request_line.len() > 4096 {
        return;
    }
    let parts: Vec<&str> = request_line.trim_end().split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("").to_string();
    let target = parts.get(1).copied().unwrap_or("").to_string();
    let mut content_length = 0usize;
    let mut header_bytes = 0usize;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).unwrap_or(0);
        if read == 0 {
            break;
        }
        header_bytes += read;
        if header_bytes > MAX_HEADER_BYTES {
            return;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    if content_length > MAX_BODY {
        let (status, content_type, payload) = ok_message(413, false, "请求体过大");
        respond(stream, status, &content_type, &payload);
        return;
    }
    let mut raw_body = vec![0u8; content_length];
    if content_length > 0 && reader.read_exact(&mut raw_body).is_err() {
        return;
    }
    let body: Value = if raw_body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&raw_body).unwrap_or_else(|_| json!({}))
    };
    let (path, query_raw) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target.as_str(), ""),
    };
    let _ = parse_query(query_raw);
    let (status, content_type, payload) = dispatch(&ctx, &method, path, &body);
    respond(stream, status, &content_type, &payload);
}

fn respond(mut stream: TcpStream, status: u16, content_type: &str, payload: &[u8]) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        content_type,
        payload.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(payload);
    let _ = stream.flush();
}

pub fn log_msg(line: &str) {
    eprintln!("[admin-gateway] {}", line);
    let path = log_path();
    rotate_log(&path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {}", crate::now_rfc3339(), line);
    }
}

pub fn run_server() {
    let dir = ipc_dir();
    let _ = fs::create_dir_all(&dir);
    let token = random_hex(32);
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            log_msg(&format!("监听失败: {}", e));
            std::process::exit(1);
        }
    };
    let port = listener.local_addr().unwrap().port();
    PORT.store(port, Ordering::SeqCst);
    let ctx = Arc::new(Context {
        token: token.clone(),
        pending: Mutex::new(HashMap::new()),
        audit: Mutex::new(AuditLog::new(audit_path(), crate::config::audit_key())),
        running: AtomicBool::new(true),
        recent: Mutex::new(HashMap::new()),
    });
    let conn = json!({
        "port": port,
        "token": token,
        "pid": std::process::id(),
        "started_at": crate::now_rfc3339()
    });
    let _ = fs::write(conn_path(), serde_json::to_string_pretty(&conn).unwrap());

    log_msg(&format!("已启动，端口 {}，提权={}", port, crate::ELEVATED.load(Ordering::SeqCst)));
    log_msg(&format!("策略 {}", config_path().display()));
    log_msg(&format!("审计 {}", audit_path().display()));

    listener.set_nonblocking(true).unwrap();
    while ctx.running.load(Ordering::SeqCst) && !crate::EXIT_REQUESTED.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let c = ctx.clone();
                thread::spawn(move || handle_stream(stream, c));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(60));
            }
            Err(_) => break,
        }
    }
    let _ = fs::remove_file(conn_path());
    log_msg("已停止");
}
