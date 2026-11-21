use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// 每条记录都带 HMAC-SHA256：密钥在管理员专属目录，普通用户读不到，
/// 因此无法在篡改后重算出一条"看起来合法"的链。
pub struct AuditLog {
    path: PathBuf,
    last_hash: String,
    seq: u64,
    key: [u8; 32],
}

fn hmac_hex(key: &[u8], message: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let mut h = Sha256::new();
        h.update(key);
        k[..32].copy_from_slice(&h.finalize());
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    let mut ipad = [0u8; BLOCK];
    let mut opad = [0u8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] = k[i] ^ 0x36;
        opad[i] = k[i] ^ 0x5c;
    }
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    format!("{:x}", outer.finalize())
}

impl AuditLog {
    pub fn new(path: PathBuf, key: [u8; 32]) -> Self {
        let mut log = AuditLog { path, last_hash: String::new(), seq: 0, key };
        if let Ok(text) = fs::read_to_string(&log.path) {
            for line in text.lines() {
                if let Ok(Value::Object(rec)) = serde_json::from_str::<Value>(line) {
                    if let Some(h) = rec.get("hash").and_then(|v| v.as_str()) {
                        log.last_hash = h.to_string();
                    }
                    if let Some(s) = rec.get("seq").and_then(|v| v.as_u64()) {
                        log.seq = s;
                    }
                }
            }
        }
        log
    }

    pub fn append(&mut self, fields: Map<String, Value>) -> u64 {
        self.seq += 1;
        let mut rec = Map::new();
        rec.insert("seq".into(), json!(self.seq));
        rec.insert("ts".into(), json!(crate::now_rfc3339()));
        rec.insert("prev_hash".into(), json!(self.last_hash));
        for (k, v) in fields {
            rec.insert(k, v);
        }
        let body = serde_json::to_vec(&Value::Object(rec.clone())).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&body);
        let hash = format!("{:x}", hasher.finalize());
        let signature = hmac_hex(&self.key, &body);
        rec.insert("hash".into(), json!(hash.clone()));
        rec.insert("hmac".into(), json!(signature));
        let line = serde_json::to_string(&Value::Object(rec)).unwrap();
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(file, "{}", line);
        }
        self.last_hash = hash;
        self.seq
    }
}

pub struct VerifyReport {
    pub records: u64,
    pub ok: bool,
    pub detail: String,
}

pub fn verify_chain(path: &PathBuf, key: &[u8; 32]) -> VerifyReport {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return VerifyReport { records: 0, ok: true, detail: "暂无审计记录".into() },
    };
    let mut prev = String::new();
    let mut count = 0u64;
    for (index, line) in text.lines().enumerate() {
        let rec: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                return VerifyReport {
                    records: count,
                    ok: false,
                    detail: format!("第 {} 行不是合法 JSON，文件已被改写", index + 1),
                }
            }
        };
        let seq = rec.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
        if rec.get("prev_hash").and_then(|v| v.as_str()).unwrap_or("") != prev {
            return VerifyReport { records: count, ok: false, detail: format!("序号 {} 的前序哈希断链", seq) };
        }
        let mut body = rec.clone();
        if let Some(obj) = body.as_object_mut() {
            obj.remove("hash");
            obj.remove("hmac");
        }
        let canonical = serde_json::to_vec(&body).unwrap();
        let hash = {
            let mut h = Sha256::new();
            h.update(&canonical);
            format!("{:x}", h.finalize())
        };
        if rec.get("hash").and_then(|v| v.as_str()).unwrap_or("") != hash {
            return VerifyReport { records: count, ok: false, detail: format!("序号 {} 内容被改动（哈希不符）", seq) };
        }
        let expected = hmac_hex(key, &canonical);
        if rec.get("hmac").and_then(|v| v.as_str()).unwrap_or("") != expected {
            return VerifyReport {
                records: count,
                ok: false,
                detail: format!("序号 {} 签名不匹配（记录不是本机管理员进程写入的）", seq),
            };
        }
        prev = hash;
        count += 1;
    }
    VerifyReport { records: count, ok: true, detail: format!("{} 条记录，哈希链与签名全部通过", count) }
}

/// 日志轮转：超过 4MB 就换文件，最多保留 3 份。
pub fn rotate_log(path: &PathBuf) {
    let too_big = fs::metadata(path).map(|m| m.len() > 4 * 1024 * 1024).unwrap_or(false);
    if !too_big {
        return;
    }
    for index in (1..3).rev() {
        let from = path.with_extension(format!("log.{}", index));
        let to = path.with_extension(format!("log.{}", index + 1));
        if from.exists() {
            let _ = fs::rename(&from, &to);
        }
    }
    let _ = fs::rename(path, path.with_extension("log.1"));
}
