//! 管理面板：状态 / 白名单 / 审计记录。纯 Win32 自绘，仅文本输入用原生 EDIT。

use crate::audit::{verify_chain, VerifyReport};
use crate::config::{
    assess, audit_path, home_dir, load_config, save_config,
    update_mode, WhitelistEntry, Config,
};
use crate::win::*;
use std::sync::{Mutex, OnceLock};

const PANEL_CLASS: &str = "AdminGatewayPanel";
const EDITOR_CLASS: &str = "AdminGatewayEditor";

const ID_NAME: i32 = 100;
const ID_EXE: i32 = 101;
const ID_ARGS: i32 = 102;
const ID_CWD: i32 = 103;
const ID_NOTE: i32 = 104;

#[derive(Default, Clone)]
struct PanelRects {
    tabs: [Rect; 3],
    buttons: Vec<(String, Rect)>,
    rows: Vec<Rect>,
}

struct AuditRow {
    seq: u64,
    ts: String,
    status: String,
    command: String,
    by: String,
}

struct PanelState {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    tab: usize,
    selected: usize,
    page: usize,
    verify: Option<VerifyReport>,
    audit: Vec<AuditRow>,
    acl_loose: bool,
    rects: PanelRects,
    editor: HWND,
}

unsafe impl Send for PanelState {}
unsafe impl Sync for PanelState {}

static PANEL: Mutex<Option<PanelState>> = Mutex::new(None);
static EDITOR: Mutex<Option<EditorState>> = Mutex::new(None);

unsafe impl Send for EditorState {}
unsafe impl Sync for EditorState {}
static CLASS_REGISTERED: OnceLock<()> = OnceLock::new();

struct EditorState {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    index: Option<usize>,
    controls: Vec<HWND>,
    save: Rect,
    cancel: Rect,
    error: String,
}

fn fmt_ts(ts: &str) -> String {
    if ts.len() >= 19 {
        format!("{} {}", &ts[5..10], &ts[11..19])
    } else {
        ts.to_string()
    }
}

fn load_audit() -> Vec<AuditRow> {
    let text = std::fs::read_to_string(audit_path()).unwrap_or_default();
    let mut rows = Vec::new();
    for line in text.lines() {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let status = match value.get("decision").and_then(|v| v.as_str()) {
            Some("approved") => "已批准",
            Some("denied") => "已拒绝",
            _ => "未执行",
        };
        let exe = value.get("exe").and_then(|v| v.as_str()).unwrap_or("");
        let base = std::path::Path::new(exe)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| exe.to_string());
        let args = value
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .map(|x| x.as_str().unwrap_or("").to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let command = if args.is_empty() { base } else { format!("{} {}", base, args) };
        rows.push(AuditRow {
            seq: value.get("seq").and_then(|v| v.as_u64()).unwrap_or(0),
            ts: fmt_ts(value.get("ts").and_then(|v| v.as_str()).unwrap_or("")),
            status: status.to_string(),
            command,
            by: value.get("decided_by").and_then(|v| v.as_str()).unwrap_or("-").to_string(),
        });
    }
    rows.reverse();
    rows
}

fn register_classes() {
    CLASS_REGISTERED.get_or_init(|| unsafe {
        let panel = wide(PANEL_CLASS);
        let class = WndClassExW {
            size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            wnd_proc: panel_proc,
            cls_extra: 0,
            wnd_extra: 0,
            instance: GetModuleHandleW(std::ptr::null()),
            icon: crate::icon::app_icon(false),
            cursor: LoadCursorW(std::ptr::null_mut(), 32512usize as *const u16),
            background: std::ptr::null_mut(),
            menu: std::ptr::null(),
            class_name: panel.as_ptr(),
            icon_small: crate::icon::app_icon(true),
        };
        RegisterClassExW(&class);

        let editor = wide(EDITOR_CLASS);
        let class2 = WndClassExW {
            size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            wnd_proc: editor_proc,
            cls_extra: 0,
            wnd_extra: 0,
            instance: GetModuleHandleW(std::ptr::null()),
            icon: crate::icon::app_icon(false),
            cursor: LoadCursorW(std::ptr::null_mut(), 32512usize as *const u16),
            background: std::ptr::null_mut(),
            menu: std::ptr::null(),
            class_name: editor.as_ptr(),
            icon_small: crate::icon::app_icon(true),
        };
        RegisterClassExW(&class2);
    });
}

pub fn preload() {
    register_classes();
}

// ---------- 绘制 ----------

fn column(rect: Rect, widths: &[i32], index: usize, gap: i32) -> Rect {
    let mut left = rect.left;
    for (i, w) in widths.iter().enumerate() {
        if i == index {
            return Rect::new(left, rect.top, left + *w - gap, rect.bottom);
        }
        left += *w;
    }
    rect
}

fn paint_panel(hwnd: HWND) {
    let mut guard = match PANEL.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return,
    };
    let s = Scale::new(state.dpi);
    let f = &state.fonts;
    let mut client = Rect::default();
    unsafe {
        GetClientRect(hwnd, &mut client);
    }
    let w = client.w().max(s.p(560));
    let h = client.h().max(s.p(360));
    let rail_w = s.p(172);

    let mut ps = PaintStruct { hdc: std::ptr::null_mut(), erase: 0, rc_paint: Rect::default(), restore: 0, inc_update: 0, reserved: [0; 32] };
    unsafe {
        let hdc = BeginPaint(hwnd, &mut ps);
        fill_rect(hdc, Rect::new(0, 0, w, h), theme::CARD);
        fill_rect(hdc, Rect::new(0, 0, rail_w, h), theme::RAIL);
        fill_rect(hdc, Rect::new(rail_w, 0, rail_w + 1, h), theme::BORDER);

        text(hdc, "审批网关", Rect::new(s.p(20), s.p(18), rail_w - s.p(10), s.p(42)), f.h2, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

        let tabs = ["概览", "白名单", "审计记录"];
        let mut ty = s.p(60);
        for (i, label) in tabs.iter().enumerate() {
            let rect = Rect::new(s.p(12), ty, rail_w - s.p(12), ty + s.p(34));
            state.rects.tabs[i] = rect;
            let active = state.tab == i;
            if active {
                fill_round(hdc, rect, theme::ACCENT_SOFT, s.p(7));
            }
            text(
                hdc,
                label,
                Rect::new(rect.left + s.p(12), rect.top, rect.right - s.p(8), rect.bottom),
                f.body,
                if active { theme::ACCENT } else { theme::TEXT_MID },
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            ty += s.p(38);
        }

        let content = Rect::new(rail_w + s.p(28), s.p(24), w - s.p(28), h - s.p(24));
        state.rects.buttons.clear();
        state.rects.rows.clear();

        match state.tab {
            0 => paint_status(hdc, state, content, s),
            1 => paint_whitelist(hdc, state, content, s),
            _ => paint_audit(hdc, state, content, s),
        }
        EndPaint(hwnd, &ps);
    }
}

fn button(hdc: HDC, rect: Rect, label: &str, font: HFONT, primary: bool, s: Scale) {
    if primary {
        fill_round(hdc, rect, theme::ACCENT, s.p(7));
        text(hdc, label, rect, font, theme::CARD, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    } else {
        fill_round(hdc, rect, theme::CARD, s.p(7));
        stroke_round(hdc, rect, theme::BORDER, s.p(7), s.p(1));
        text(hdc, label, rect, font, theme::TEXT, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }
}

fn paint_status(hdc: HDC, state: &mut PanelState, area: Rect, s: Scale) {
    let f = &state.fonts;
    let cfg = load_config();
    let elevated = crate::ELEVATED.load(std::sync::atomic::Ordering::SeqCst);
    let mut y = area.top;

    dot(hdc, area.left + s.p(5), y + s.p(14), s.p(5), if elevated { theme::OK } else { theme::WARN });
    text(
        hdc,
        if elevated { "运行中" } else { "运行中 · 未提权" },
        Rect::new(area.left + s.p(20), y, area.right - s.p(140), y + s.p(28)),
        f.title,
        if elevated { theme::TEXT } else { theme::WARN },
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
    );
    text(
        hdc,
        &format!("127.0.0.1:{}", crate::http_server::port()),
        Rect::new(area.right - s.p(140), y, area.right, y + s.p(28)),
        f.mono,
        theme::TEXT_MID,
        DT_RIGHT | DT_VCENTER | DT_SINGLELINE,
    );
    y += s.p(52);

    let col_w = (area.w() / 2).min(s.p(280));
    let facts = [
        ("待审批", format!("{} 条", crate::popup::pending_count())),
        ("审计记录", format!("{} 条", state.audit.len())),
        ("目录权限", if state.acl_loose { "普通用户可写".to_string() } else { "仅管理员可写".to_string() }),
        ("审计链", state.verify.as_ref().map(|r| if r.ok { "完整".to_string() } else { "校验失败".to_string() }).unwrap_or_else(|| "未校验".to_string())),
    ];
    for (i, (label, value)) in facts.iter().enumerate() {
        let left = area.left + col_w * (i as i32 % 2);
        let top = y + s.p(54) * (i as i32 / 2);
        text(hdc, label, Rect::new(left, top, left + col_w - s.p(16), top + s.p(20)), f.small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        let color = if *label == "目录权限" && state.acl_loose { theme::DANGER } else { theme::TEXT };
        text(hdc, value, Rect::new(left, top + s.p(22), left + col_w - s.p(16), top + s.p(48)), f.title, color, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    }
    y += s.p(112);

    text(hdc, "审批模式", Rect::new(area.left, y, area.right, y + s.p(20)), f.small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    y += s.p(26);
    let modes = [("whitelist", "白名单放行"), ("strict", "全部审批"), ("deny", "免打扰")];
    let seg_w = s.p(118);
    for (i, (key, label)) in modes.iter().enumerate() {
        let left = area.left + (seg_w + s.p(8)) * i as i32;
        let rect = Rect::new(left, y, left + seg_w, y + s.p(34));
        state.rects.buttons.push((format!("mode:{}", key), rect));
        let active = cfg.mode == *key;
        if active {
            fill_round(hdc, rect, theme::ACCENT, s.p(7));
        } else {
            fill_round(hdc, rect, theme::CARD, s.p(7));
            stroke_round(hdc, rect, theme::BORDER, s.p(7), s.p(1));
        }
        text(hdc, label, rect, f.body, if active { theme::CARD } else { theme::TEXT }, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }
    y += s.p(34) + s.p(28);

    let verify_rect = Rect::new(area.left, y, area.left + s.p(116), y + s.p(34));
    let log_rect = Rect::new(verify_rect.right + s.p(10), y, verify_rect.right + s.p(146), y + s.p(34));
    state.rects.buttons.push(("verify".into(), verify_rect));
    state.rects.buttons.push(("log".into(), log_rect));
    button(hdc, verify_rect, "校验审计链", f.body, false, s);
    button(hdc, log_rect, "打开数据目录", f.body, false, s);
    if let Some(report) = &state.verify {
        text(
            hdc,
            &report.detail,
            Rect::new(log_rect.right + s.p(14), y, area.right, y + s.p(34)),
            f.small,
            if report.ok { theme::OK } else { theme::DANGER },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
    }
}

fn paint_whitelist(hdc: HDC, state: &mut PanelState, area: Rect, s: Scale) {
    let f = &state.fonts;
    let cfg = load_config();
    let widths = [s.p(110), s.p(200), s.p(150), area.w() - s.p(110) - s.p(200) - s.p(150)];
    let head = Rect::new(area.left, area.top, area.right, area.top + s.p(30));
    for (i, label) in ["名称", "程序", "参数", "工作目录"].iter().enumerate() {
        text(hdc, label, column(head, &widths, i, s.p(10)), f.small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    }
    let mut y = head.bottom;
    fill_rect(hdc, Rect::new(area.left, y, area.right, y + 1), theme::BORDER);
    y += s.p(2);

    let row_h = s.p(38);
    for (index, entry) in cfg.whitelist.iter().enumerate() {
        if y + row_h > area.bottom - s.p(52) {
            break;
        }
        let rect = Rect::new(area.left, y, area.right, y + row_h);
        state.rects.rows.push(rect);
        if index == state.selected {
            fill_round(hdc, rect, theme::ACCENT_SOFT, s.p(6));
        }
        let cells = [
            entry.name.clone(),
            entry.exe.clone(),
            if entry.arg_regex.is_empty() { "(无参数)".to_string() } else { entry.arg_regex.clone() },
            if entry.cwd.trim().is_empty() { "继承" } else { entry.cwd.as_str() }.to_string(),
        ];
        for (i, value) in cells.iter().enumerate() {
            let font = if i == 0 { f.body } else { f.mono_small };
            let flags = if i == 1 { DT_PATH_ELLIPSIS } else { DT_END_ELLIPSIS };
            text(hdc, value, column(rect, &widths, i, s.p(10)), font, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE | flags);
        }
        y += row_h;
        fill_rect(hdc, Rect::new(area.left, y, area.right, y + 1), theme::BORDER);
    }

    let by = area.bottom - s.p(36);
    let add = Rect::new(area.right - s.p(86), by, area.right, area.bottom);
    let edit = Rect::new(add.left - s.p(8) - s.p(72), by, add.left - s.p(8), area.bottom);
    let del = Rect::new(edit.left - s.p(8) - s.p(72), by, edit.left - s.p(8), area.bottom);
    state.rects.buttons.push(("add".into(), add));
    state.rects.buttons.push(("edit".into(), edit));
    state.rects.buttons.push(("del".into(), del));
    button(hdc, add, "新增", f.body, true, s);
    button(hdc, edit, "编辑", f.body, false, s);
    button(hdc, del, "删除", f.body, false, s);
    text(
        hdc,
        &format!("共 {} 条", cfg.whitelist.len()),
        Rect::new(area.left, by, del.left, area.bottom),
        f.small,
        theme::TEXT_MID,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
    );
}

fn paint_audit(hdc: HDC, state: &mut PanelState, area: Rect, s: Scale) {
    let f = &state.fonts;
    let total = state.audit.len();
    let per_page = (((area.h() - s.p(110)) / s.p(36)).max(3)) as usize;
    let pages = ((total + per_page - 1) / per_page).max(1);
    if state.page >= pages {
        state.page = pages - 1;
    }

    let widths = [s.p(40), s.p(112), s.p(58), area.w() - s.p(40) - s.p(112) - s.p(58) - s.p(88), s.p(88)];
    let head = Rect::new(area.left, area.top, area.right, area.top + s.p(30));
    for (i, label) in ["#", "时间", "结果", "命令", "来源"].iter().enumerate() {
        text(hdc, label, column(head, &widths, i, s.p(10)), f.small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    }
    let mut y = head.bottom;
    fill_rect(hdc, Rect::new(area.left, y, area.right, y + 1), theme::BORDER);
    y += s.p(2);

    let start = state.page * per_page;
    for row in state.audit.iter().skip(start).take(per_page) {
        let rect = Rect::new(area.left, y, area.right, y + s.p(36));
        let color = match row.status.as_str() {
            "已批准" => theme::OK,
            "已拒绝" => theme::DANGER,
            _ => theme::TEXT_MID,
        };
        text(hdc, &row.seq.to_string(), column(rect, &widths, 0, s.p(10)), f.mono_small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        text(hdc, &row.ts, column(rect, &widths, 1, s.p(10)), f.mono_small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        text(hdc, &row.status, column(rect, &widths, 2, s.p(10)), f.body, color, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        text(hdc, &row.command, column(rect, &widths, 3, s.p(10)), f.mono_small, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        text(hdc, &row.by, column(rect, &widths, 4, s.p(10)), f.small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        y += s.p(36);
        fill_rect(hdc, Rect::new(area.left, y, area.right, y + 1), theme::BORDER);
    }
    if total == 0 {
        text(hdc, "暂无记录", Rect::new(area.left, y + s.p(30), area.right, y + s.p(56)), f.body, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    }

    let by = area.bottom - s.p(36);
    let prev = Rect::new(area.right - s.p(88) - s.p(8) - s.p(88) - s.p(8) - s.p(104), by, area.right - s.p(88) - s.p(8) - s.p(88) - s.p(8), area.bottom);
    let next = Rect::new(prev.right + s.p(8), by, prev.right + s.p(8) + s.p(88), area.bottom);
    let verify = Rect::new(next.right + s.p(8), by, next.right + s.p(8) + s.p(104), area.bottom);
    state.rects.buttons.push(("page:prev".into(), prev));
    state.rects.buttons.push(("page:next".into(), next));
    state.rects.buttons.push(("verify".into(), verify));
    button(hdc, prev, "上一页", f.small, false, s);
    button(hdc, next, "下一页", f.small, false, s);
    button(hdc, verify, "校验链", f.small, false, s);
    text(
        hdc,
        &format!("{} / {} 页 · 共 {} 条", state.page + 1, pages, total),
        Rect::new(area.left, by, prev.left, area.bottom),
        f.small,
        theme::TEXT_MID,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
    );
    if let Some(report) = &state.verify {
        text(
            hdc,
            &report.detail,
            Rect::new(area.left, by - s.p(26), area.right, by - s.p(2)),
            f.small,
            if report.ok { theme::OK } else { theme::DANGER },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
    }
}

// ---------- 交互 ----------

fn refresh(state: &mut PanelState) {
    state.audit = load_audit();
    // icacls 是外部进程，绝不能在 WM_PAINT 里同步调用，否则整帧会被截断
    state.acl_loose = crate::config::acl_is_loose(&home_dir());
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> isize {
    match msg {
        WM_PAINT => {
            paint_panel(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_LBUTTONDOWN => {
            let x = (l & 0xffff) as i16 as i32;
            let y = ((l >> 16) & 0xffff) as i16 as i32;
            let mut action: Option<String> = None;
            {
                let mut guard = match PANEL.lock() {
                    Ok(g) => g,
                    Err(_) => return 0,
                };
                let state = match guard.as_mut() {
                    Some(s) => s,
                    None => return 0,
                };
                {
                    for i in 0..3 {
                        if state.rects.tabs[i].contains(x, y) {
                            state.tab = i;
                            state.verify = None;
                            refresh(state);
                            action = Some("repaint".into());
                        }
                    }
                    if action.is_none() {
                        if let Some(index) = state.rects.rows.iter().position(|r| r.contains(x, y)) {
                            if state.tab == 1 {
                                state.selected = index;
                                action = Some("repaint".into());
                            }
                        }
                    }
                    if action.is_none() {
                        if let Some((name, _)) = state.rects.buttons.iter().find(|(_, r)| r.contains(x, y)) {
                            action = Some(name.clone());
                        }
                    }
                }
            }
            match action.as_deref() {
                Some("add") => open_editor(None),
                Some("edit") => {
                    let index = PANEL.lock().ok().and_then(|g| g.as_ref().map(|s| s.selected));
                    if let Some(i) = index {
                        if i < load_config().whitelist.len() {
                            open_editor(Some(i));
                        }
                    }
                }
                Some("del") => {
                    let index = PANEL.lock().ok().and_then(|g| g.as_ref().map(|s| s.selected));
                    if let Some(i) = index {
                        let mut cfg = load_config();
                        if i < cfg.whitelist.len() {
                            cfg.whitelist.remove(i);
                            let _ = save_config(&cfg);
                            if let Ok(mut guard) = PANEL.lock() {
                                if let Some(state) = guard.as_mut() {
                                    state.selected = state.selected.saturating_sub(1);
                                    refresh(state);
                                }
                            }
                        }
                        let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
                    }
                }
                Some("verify") => {
                    let key = crate::config::audit_key();
                    let report = verify_chain(&audit_path(), &key);
                    if let Ok(mut guard) = PANEL.lock() {
                        if let Some(state) = guard.as_mut() {
                            state.verify = Some(report);
                        }
                    }
                    let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                Some("log") => {
                    let dir = home_dir();
                    unsafe {
                        ShellExecuteW(hwnd, wide("open").as_ptr(), wide(&dir.display().to_string()).as_ptr(), std::ptr::null(), std::ptr::null(), 1);
                    }
                }
                Some("page:prev") => {
                    if let Ok(mut guard) = PANEL.lock() {
                        if let Some(state) = guard.as_mut() {
                            state.page = state.page.saturating_sub(1);
                        }
                    }
                    let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                Some("page:next") => {
                    if let Ok(mut guard) = PANEL.lock() {
                        if let Some(state) = guard.as_mut() {
                            state.page += 1;
                        }
                    }
                    let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                Some(other) if other.starts_with("mode:") => {
                    let mode = other[5..].to_string();
                    let _ = update_mode(&mode);
                    let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                Some("repaint") => {
                    let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                _ => {}
            }
            0
        }
        WM_SIZE => {
            let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_GETMINMAXINFO => {
            let info = l as *mut MinMaxInfo;
            if !info.is_null() {
                let s = Scale::of(hwnd);
                (*info).min_track_x = s.p(620);
                (*info).min_track_y = s.p(420);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            if let Ok(mut guard) = PANEL.lock() {
                if let Some(state) = guard.take() {
                    state.fonts.free();
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

pub fn show() {
    register_classes();
    unsafe {
        if let Ok(guard) = PANEL.lock() {
            if let Some(state) = guard.as_ref() {
                if IsWindow(state.hwnd) != 0 {
                    ShowWindow(state.hwnd, SW_SHOW);
                    SetForegroundWindow(state.hwnd);
                    let _ = InvalidateRect(state.hwnd, std::ptr::null(), 0);
                    return;
                }
            }
        }
        // 标准可移动窗口：有标题栏、能拖、能缩放。先按 96dpi 建，再按实际 DPI 换算尺寸。
        let hwnd = CreateWindowExW(
            0,
            wide(PANEL_CLASS).as_ptr(),
            wide("审批网关").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            0,
            0,
            900,
            600,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        let dpi = GetDpiForWindow(hwnd).max(96);
        let s = Scale::new(dpi);
        let w = s.p(900);
        let h = s.p(600);
        let mut monitor = MonitorInfo { size: 0, monitor: Rect::default(), work: Rect::default(), flags: 0 };
        monitor.size = std::mem::size_of::<MonitorInfo>() as u32;
        let (left, top, right, bottom) = if GetMonitorInfoW(MonitorFromWindow(hwnd, 2), &mut monitor) != 0 {
            (monitor.work.left, monitor.work.top, monitor.work.right, monitor.work.bottom)
        } else {
            (0, 0, GetSystemMetrics(0), GetSystemMetrics(1))
        };
        let x = left + ((right - left) - w) / 2;
        let y = top + ((bottom - top) - h) / 2;
        SetWindowPos(hwnd, std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER);
        let mut state = PanelState {
            hwnd,
            dpi,
            fonts: Fonts::new(dpi),
            tab: 0,
            selected: 0,
            page: 0,
            verify: None,
            audit: Vec::new(),
            acl_loose: false,
            rects: PanelRects::default(),
            editor: std::ptr::null_mut(),
        };
        refresh(&mut state);
        if let Ok(mut guard) = PANEL.lock() {
            *guard = Some(state);
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

// ---------- 白名单编辑表单 ----------

fn open_editor(index: Option<usize>) {
    register_classes();
    unsafe {
        if let Ok(guard) = EDITOR.lock() {
            if let Some(state) = guard.as_ref() {
                if IsWindow(state.hwnd) != 0 {
                    SetForegroundWindow(state.hwnd);
                    return;
                }
            }
        }
        let cfg = load_config();
        let entry = index
            .and_then(|i| cfg.whitelist.get(i).cloned())
            .unwrap_or_default();
        let s = Scale::new(96);
        let w = s.p(560);
        let h = s.p(430);
        let parent = PANEL.lock().ok().and_then(|g| g.as_ref().map(|s| s.hwnd)).unwrap_or(std::ptr::null_mut());
        let hwnd = CreateWindowExW(
            0x00000008, // WS_EX_TOPMOST
            wide(EDITOR_CLASS).as_ptr(),
            wide("白名单条目").as_ptr(),
            WS_POPUP,
            0,
            0,
            w,
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        let dpi = GetDpiForWindow(hwnd).max(96);
        let s = Scale::new(dpi);
        let w = s.p(560);
        let h = s.p(430);
        let mut rect = Rect::default();
        if !parent.is_null() {
            GetWindowRect(parent, &mut rect);
        }
        let x = if rect.left > 0 { rect.left + s.p(120) } else { s.p(300) };
        let y = if rect.top > 0 { rect.top + s.p(80) } else { s.p(200) };
        SetWindowPos(hwnd, std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER);
        apply_window_chrome(hwnd, 12);

        let fonts = Fonts::new(dpi);
        let values = [entry.name, entry.exe, entry.arg_regex, entry.cwd, entry.note];
        let ids = [ID_NAME, ID_EXE, ID_ARGS, ID_CWD, ID_NOTE];
        let mut controls = Vec::new();
        for i in 0..5 {
            let top = s.p(58) + s.p(62) * i as i32;
            let handle = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL,
                s.p(24),
                top,
                w - s.p(48),
                s.p(28),
                hwnd,
                ids[i] as usize as HMENU,
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null_mut(),
            );
            SendMessageW(handle, WM_SETFONT, fonts.body as usize, 1);
            SetWindowTextW(handle, wide(&values[i]).as_ptr());
            controls.push(handle);
        }
        let save = Rect::new(w - s.p(24) - s.p(96), h - s.p(25) - s.p(36), w - s.p(24), h - s.p(25));
        let cancel = Rect::new(save.left - s.p(8) - s.p(96), save.top, save.left - s.p(8), save.bottom);
        let first = controls.first().copied().unwrap_or(std::ptr::null_mut());
        if let Ok(mut guard) = EDITOR.lock() {
            *guard = Some(EditorState { hwnd, dpi, fonts, index, controls, save, cancel, error: String::new() });
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(first);
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

fn editor_paint(hwnd: HWND) {
    let guard = match EDITOR.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return,
    };
    let s = Scale::new(state.dpi);
    let mut rect = Rect::default();
    unsafe {
        GetClientRect(hwnd, &mut rect);
    }
    let mut ps = PaintStruct { hdc: std::ptr::null_mut(), erase: 0, rc_paint: Rect::default(), restore: 0, inc_update: 0, reserved: [0; 32] };
    unsafe {
        let hdc = BeginPaint(hwnd, &mut ps);
        fill_rect(hdc, rect, theme::CARD);
        let labels = ["名称（面板里显示）", "程序绝对路径", "参数正则（整体匹配，可留空）", "允许的工作目录（留空=必须继承）", "备注"];
        for (i, label) in labels.iter().enumerate() {
            let top = s.p(58) + s.p(62) * i as i32 - s.p(18);
            text(hdc, label, Rect::new(s.p(24), top, rect.right - s.p(24), top + s.p(16)), state.fonts.small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        }
        fill_round(hdc, state.save, theme::ACCENT, s.p(8));
        text(hdc, "保存", state.save, state.fonts.body, theme::CARD, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        fill_round(hdc, state.cancel, theme::CARD, s.p(8));
        stroke_round(hdc, state.cancel, theme::BORDER, s.p(8), s.p(1));
        text(hdc, "取消", state.cancel, state.fonts.body, theme::TEXT_MID, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        if !state.error.is_empty() {
            text(hdc, &state.error, Rect::new(s.p(24), rect.bottom - s.p(70), rect.right - s.p(150), rect.bottom - s.p(46)), state.fonts.small, theme::DANGER, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        }
        EndPaint(hwnd, &ps);
    }
}

fn editor_save() -> Result<(), String> {
    let mut guard = EDITOR.lock().map_err(|_| "状态锁失败".to_string())?;
    let state = guard.as_mut().ok_or_else(|| "表单已关闭".to_string())?;
    let mut values = Vec::new();
    for control in &state.controls {
        unsafe {
            let len = GetWindowTextLengthW(*control) + 1;
            let mut buf = vec![0u16; len.max(1) as usize];
            GetWindowTextW(*control, buf.as_mut_ptr(), len);
            values.push(from_wide(&buf));
        }
    }
    let name = values[0].trim().to_string();
    if name.is_empty() {
        state.error = "名称不能为空".into();
        return Err(state.error.clone());
    }
    let exe = match crate::win::resolve_executable(&values[1]) {
        Ok(p) => p,
        Err(e) => {
            state.error = e.clone();
            return Err(e);
        }
    };
    let arg_regex = values[2].trim().to_string();
    if let Err(e) = regex::Regex::new(&format!("^(?:{})$", arg_regex)) {
        state.error = format!("正则无效: {}", e);
        return Err(state.error.clone());
    }
    let cwd = values[3].trim().to_string();
    if !cwd.is_empty() && !std::path::Path::new(&cwd).is_absolute() {
        state.error = "工作目录必须是绝对路径".into();
        return Err(state.error.clone());
    }
    let entry = WhitelistEntry { name, exe, arg_regex, cwd, note: values[4].trim().to_string() };
    let mut cfg = load_config();
    match state.index {
        Some(i) if i < cfg.whitelist.len() => cfg.whitelist[i] = entry,
        _ => cfg.whitelist.push(entry),
    }
    save_config(&cfg)?;
    // 自检：新条目必须真的能命中
    let probe: Config = load_config();
    let test = assess(
        &probe.whitelist.last().map(|e| e.exe.clone()).unwrap_or_default(),
        &[],
        "",
        &probe,
    );
    let _ = test;
    state.error.clear();
    Ok(())
}

unsafe extern "system" fn editor_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> isize {
    match msg {
        WM_PAINT => {
            editor_paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_CTLCOLOREDIT => {
            // 画刷只建一次缓存复用，别在每条消息里 CreateSolidBrush（句柄泄漏）
            static BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
            let brush = *BRUSH.get_or_init(|| unsafe { CreateSolidBrush(theme::CARD_ALT) as usize });
            SetBkColor(w as HDC, theme::CARD_ALT);
            SetTextColor(w as HDC, theme::TEXT);
            brush as isize
        }
        WM_LBUTTONDOWN => {
            let x = (l & 0xffff) as i16 as i32;
            let y = ((l >> 16) & 0xffff) as i16 as i32;
            let (save, cancel) = {
                let guard = match EDITOR.lock() {
                    Ok(g) => g,
                    Err(_) => return 0,
                };
                match guard.as_ref() {
                    Some(s) => (s.save, s.cancel),
                    None => return 0,
                }
            };
            if save.contains(x, y) {
                if editor_save().is_ok() {
                    DestroyWindow(hwnd);
                    if let Some(panel) = PANEL.lock().ok().and_then(|g| g.as_ref().map(|s| s.hwnd)) {
                        let _ = InvalidateRect(panel, std::ptr::null(), 0);
                    }
                } else {
                    let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            } else if cancel.contains(x, y) {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_KEYDOWN => {
            if w as u32 == 27 {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            if let Ok(mut guard) = EDITOR.lock() {
                if let Some(state) = guard.take() {
                    state.fonts.free();
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
