//! 审批弹窗：per-monitor DPI 感知、自绘、每个请求独立窗口实例。
//!
//! 布局只算一次（show_popup 里用屏幕 DC 量文字），绘制与命中测试共用同一份矩形，
//! 避免"先画后判"导致的第一下点击落空。

use crate::win::*;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::thread;

pub const WM_APP_SHOW: u32 = 0x8000 + 1;
pub const WM_APP_TRAY: u32 = 0x8000 + 3;

pub const WM_APP_POPUP_CLOSE: u32 = 0x8000 + 5;
const WM_APP_POPUP_DESTROY: u32 = 0x8000 + 6;

pub struct PopupView {
    pub id: String,
    pub exe: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub reason: String,
    pub risk_flags: Vec<String>,
    pub whitelist: String,
    pub wait_sec: u64,
    pub mode: String,
    pub decision: Sender<(String, String)>,
}

#[derive(Default, Clone)]
struct Layout {
    width: i32,
    height: i32,
    close: Rect,
    card: Rect,
    lines: Vec<String>,
    line_h: i32,
    cwd: Rect,
    risk: Rect,
    reason: Rect,
    approve: Rect,
    deny: Rect,
    note: Rect,
}

struct PopupState {
    hwnd: HWND,
    dpi: u32,
    view: PopupView,
    remaining: u64,
    total: u64,
    done: bool,
    pending_decision: Option<(String, String)>,
    layout: Layout,
    fonts: Fonts,
}

unsafe impl Send for PopupState {}
unsafe impl Sync for PopupState {}

static POPUPS: Mutex<Vec<PopupState>> = Mutex::new(Vec::new());

fn fmt_argv(view: &PopupView) -> String {
    if view.args.is_empty() {
        return String::new();
    }
    view.args
        .iter()
        .map(|a| {
            if a.is_empty() {
                "\"\"".to_string()
            } else if a.contains(' ') || a.contains('"') {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_layout(hdc: HDC, s: Scale, fonts: &Fonts, view: &PopupView) -> Layout {
    let pad = s.p(20);
    let width = s.p(420);
    let line_h = s.p(18);
    let inner = width - pad * 2 - s.p(24);

    let mut command = view.exe.clone();
    let argv = fmt_argv(view);
    if !argv.is_empty() {
        command.push(' ');
        command.push_str(&argv);
    }
    let mut lines = wrap_lines(hdc, &command, fonts.mono, inner, 3);
    if lines.is_empty() {
        lines.push(command);
    }

    let mut y = s.p(16);
    y += s.p(22); // 标题
    y += s.p(17) + s.p(14); // 副标题 + 间距
    let card = Rect::new(pad, y, width - pad, y + s.p(11) + line_h * lines.len() as i32 + s.p(11));
    y = card.bottom;

    let cwd = Rect::default();
    let risk = if view.risk_flags.is_empty() {
        Rect::default()
    } else {
        y += s.p(10);
        let r = Rect::new(pad, y, width - pad, y + s.p(18));
        y = r.bottom;
        r
    };
    let reason = if view.reason.trim().is_empty() {
        Rect::default()
    } else {
        y += s.p(12);
        let r = Rect::new(pad, y, width - pad, y + s.p(20));
        y = r.bottom;
        r
    };
    y += s.p(22);
    let half = (width - pad * 2 - s.p(10)) / 2;
    let approve = Rect::new(pad, y, pad + half, y + s.p(42));
    let deny = Rect::new(approve.right + s.p(10), y, width - pad, y + s.p(42));
    y = approve.bottom + s.p(20);
    let note = Rect::default();

    Layout {
        width,
        height: y,
        close: Rect::new(width - s.p(44), s.p(12), width - s.p(18), s.p(36)),
        card,
        lines,
        line_h,
        cwd,
        risk,
        reason,
        approve,
        deny,
        note,
    }
}

fn paint(hwnd: HWND) {
    let mut guard = match POPUPS.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let state = match guard.iter_mut().find(|p| p.hwnd == hwnd) {
        Some(s) => s,
        None => return,
    };
    let s = Scale::new(state.dpi);
    let f = &state.fonts;
    let layout = state.layout.clone();

    let mut ps = PaintStruct {
        hdc: std::ptr::null_mut(),
        erase: 0,
        rc_paint: Rect::default(),
        restore: 0,
        inc_update: 0,
        reserved: [0; 32],
    };
    unsafe {
        let hdc = BeginPaint(hwnd, &mut ps);
        fill_rect(hdc, Rect::new(0, 0, layout.width, layout.height), theme::CARD);

        // 顶部随剩余时间缩短的细线，替代原来的进度条 + 文字两块
        if state.total > 0 {
            let ratio = state.remaining as f64 / state.total as f64;
            let width = (layout.width as f64 * ratio) as i32;
            if width > 0 {
                fill_rect(hdc, Rect::new(0, 0, width, s.p(2)), theme::ACCENT);
            }
        }

        let pad = s.p(20);
        text(
            hdc,
            "需要你批准",
            Rect::new(pad, s.p(14), layout.width - s.p(52), s.p(36)),
            f.title,
            theme::TEXT,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        let subtitle = if state.view.mode == "strict" {
            format!("全部审批模式 · {} 秒后自动拒绝", state.remaining)
        } else {
            format!("非白名单命令 · {} 秒后自动拒绝", state.remaining)
        };
        text(
            hdc,
            &subtitle,
            Rect::new(pad, s.p(36), layout.width - s.p(52), s.p(55)),
            f.small,
            theme::TEXT_MID,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        text(hdc, "×", layout.close, f.h2, theme::TEXT_MID, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        // 命令块：一个等宽文本区，不再分「可执行文件 / 参数」两栏
        fill_round(hdc, layout.card, theme::BG, s.p(8));
        let mut cy = layout.card.top + s.p(11);
        for line in &layout.lines {
            text(
                hdc,
                line,
                Rect::new(layout.card.left + s.p(12), cy, layout.card.right - s.p(12), cy + layout.line_h),
                f.mono,
                theme::TEXT,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            cy += layout.line_h;
        }

        if layout.cwd.bottom > layout.cwd.top {
            text(
                hdc,
                &format!("工作目录 {}", state.view.cwd),
                layout.cwd,
                f.small,
                theme::TEXT_FAINT,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS,
            );
        }
        if layout.risk.bottom > layout.risk.top {
            dot(hdc, layout.risk.left + s.p(3), layout.risk.top + s.p(9), s.p(3), theme::DANGER);
            text(
                hdc,
                &state.view.risk_flags.join(" · "),
                Rect::new(layout.risk.left + s.p(14), layout.risk.top, layout.risk.right, layout.risk.bottom),
                f.small,
                theme::DANGER,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
        if layout.reason.bottom > layout.reason.top {
            text(
                hdc,
                state.view.reason.trim(),
                layout.reason,
                f.body,
                theme::TEXT_MID,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }

        fill_round(hdc, layout.approve, theme::ACCENT, s.p(8));
        text(hdc, "批准一次", layout.approve, f.h2, theme::CARD, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        fill_round(hdc, layout.deny, theme::CARD, s.p(8));
        stroke_round(hdc, layout.deny, theme::BORDER, s.p(8), s.p(1));
        text(hdc, "拒绝", layout.deny, f.h2, theme::TEXT_MID, DT_CENTER | DT_VCENTER | DT_SINGLELINE);


        EndPaint(hwnd, &ps);
    }
}

/// 只标记结束并投递销毁消息。不能在持有 POPUPS 锁时调 DestroyWindow——
/// WM_DESTROY 会再次申请同一把非重入锁，直接死锁。
fn close_with(state: &mut PopupState, decision: &str, by: &str) {
    if state.done {
        return;
    }
    state.done = true;
    let _ = state.view.decision.send((decision.to_string(), by.to_string()));
    unsafe {
        KillTimer(state.hwnd, 1);
        PostMessageW(state.hwnd, WM_APP_POPUP_DESTROY, 0, 0);
    }
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> isize {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_LBUTTONDOWN => {
            let x = (l & 0xffff) as i16 as i32;
            let y = ((l >> 16) & 0xffff) as i16 as i32;
            let mut guard = match POPUPS.lock() {
                Ok(g) => g,
                Err(_) => return 0,
            };
            if let Some(state) = guard.iter_mut().find(|p| p.hwnd == hwnd) {
                let layout = &state.layout;
                if layout.close.contains(x, y) {
                    close_with(state, "denied", "user-closed");
                } else if layout.approve.contains(x, y) {
                    close_with(state, "approved", "user-click");
                } else if layout.deny.contains(x, y) {
                    close_with(state, "denied", "user-click");
                }
            }
            0
        }
        WM_KEYDOWN => {
            if w as u32 == 27 {
                let mut guard = match POPUPS.lock() {
                    Ok(g) => g,
                    Err(_) => return 0,
                };
                if let Some(state) = guard.iter_mut().find(|p| p.hwnd == hwnd) {
                    close_with(state, "denied", "user-esc");
                }
            }
            0
        }
        WM_TIMER => {
            let expired = {
                let mut guard = match POPUPS.lock() {
                    Ok(g) => g,
                    Err(_) => return 0,
                };
                match guard.iter_mut().find(|p| p.hwnd == hwnd) {
                    Some(state) => {
                        if state.remaining > 0 {
                            state.remaining -= 1;
                        }
                        let done = state.remaining == 0;
                        if done {
                            close_with(state, "denied", "timeout");
                        }
                        !done
                    }
                    None => false,
                }
            };
            if expired {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_APP_POPUP_CLOSE => {
            let mut guard = match POPUPS.lock() {
                Ok(g) => g,
                Err(_) => return 0,
            };
            if let Some(state) = guard.iter_mut().find(|p| p.hwnd == hwnd) {
                let pair = state.pending_decision.take();
                if let Some((decision, by)) = pair {
                    close_with(state, &decision, &by);
                }
            }
            0
        }
        WM_CLOSE => {
            let mut guard = match POPUPS.lock() {
                Ok(g) => g,
                Err(_) => return 0,
            };
            if let Some(state) = guard.iter_mut().find(|p| p.hwnd == hwnd) {
                close_with(state, "denied", "user-closed");
            }
            0
        }
        WM_APP_POPUP_DESTROY => {
            // 先摘掉状态并释放字体，再销毁窗口；WM_DESTROY 里绝不再加锁
            let removed = match POPUPS.lock() {
                Ok(mut guard) => guard.iter().position(|p| p.hwnd == hwnd).map(|pos| guard.remove(pos)),
                Err(_) => None,
            };
            if let Some(state) = removed {
                state.fonts.free();
            }
            DestroyWindow(hwnd);
            refresh_tip();
            0
        }
        WM_DESTROY => 0,
        WM_DPICHANGED => {
            let dpi = GetDpiForWindow(hwnd);
            let screen = GetDC(std::ptr::null_mut());
            let mut guard = match POPUPS.lock() {
                Ok(g) => g,
                Err(_) => {
                    ReleaseDC(std::ptr::null_mut(), screen);
                    return 0;
                }
            };
            if let Some(state) = guard.iter_mut().find(|p| p.hwnd == hwnd) {
                state.fonts.free();
                state.dpi = dpi;
                state.fonts = Fonts::new(dpi);
                let s = Scale::new(dpi);
                state.layout = build_layout(screen, s, &state.fonts, &state.view);
                let layout = state.layout.clone();
                ReleaseDC(std::ptr::null_mut(), screen);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    layout.width,
                    layout.height,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            } else {
                ReleaseDC(std::ptr::null_mut(), screen);
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn position_for(hwnd: HWND, width: i32, height: i32, slot: usize, s: Scale) {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, 2 /* NEAREST */);
        let mut info = MonitorInfo { size: 0, monitor: Rect::default(), work: Rect::default(), flags: 0 };
        info.size = std::mem::size_of::<MonitorInfo>() as u32;
        let (right, bottom) = if GetMonitorInfoW(monitor, &mut info) != 0 {
            (info.work.right, info.work.bottom)
        } else {
            (GetSystemMetrics(0), GetSystemMetrics(1))
        };
        let margin = s.p(16);
        let gap = s.p(12);
        let x = right - width - margin;
        let y = bottom - margin - height - slot as i32 * (height + gap);
        let y = if y < margin { margin + slot as i32 * s.p(28) } else { y };
        SetWindowPos(hwnd, std::ptr::null_mut(), x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER);
    }
}

pub fn show_popup(view: PopupView) {
    let total = view.wait_sec.max(1);
    unsafe {
        // 先用 96dpi 量一个占位尺寸建窗口，再按窗口实际 DPI 重算，避免缩放不一致
        let screen = GetDC(std::ptr::null_mut());
        let probe = Fonts::new(96);
        let mut layout = build_layout(screen, Scale::new(96), &probe, &view);
        probe.free();

        let class_name = wide("AdminGatewayPopup");
        let class = WndClassExW {
            size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            wnd_proc: popup_proc,
            cls_extra: 0,
            wnd_extra: 0,
            instance: GetModuleHandleW(std::ptr::null()),
            icon: crate::icon::shield(false),
            cursor: LoadCursorW(std::ptr::null_mut(), 32512usize as *const u16),
            background: std::ptr::null_mut(),
            menu: std::ptr::null(),
            class_name: class_name.as_ptr(),
            icon_small: crate::icon::shield(true),
        };
        RegisterClassExW(&class);
        let title = wide("管理员命令审批");
        let hwnd = CreateWindowExW(
            0x00000008, // WS_EX_TOPMOST
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            layout.width,
            layout.height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen);
            let _ = view.decision.send(("denied".into(), "ui-error".into()));
            return;
        }
        let dpi = GetDpiForWindow(hwnd).max(96);
        let s = Scale::new(dpi);
        let fonts = Fonts::new(dpi);
        layout = build_layout(screen, s, &fonts, &view);
        ReleaseDC(std::ptr::null_mut(), screen);

        let slot = POPUPS.lock().map(|g| g.len()).unwrap_or(0);
        position_for(hwnd, layout.width, layout.height, slot, s);
        apply_window_chrome(hwnd, 12);
        if let Ok(mut guard) = POPUPS.lock() {
            guard.push(PopupState {
                hwnd,
                dpi,
                view,
                remaining: total,
                total,
                done: false,
                pending_decision: None,
                layout,
                fonts,
            });
        }
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, 1, 1000, std::ptr::null_mut());
        InvalidateRect(hwnd, std::ptr::null(), 0);
        UpdateWindow(hwnd);
        crate::tray::set_tip(&format!("有 {} 条命令等待批准", slot + 1));
    }
}

/// 服务端超时或其他来源的决策，投递到对应请求的窗口线程自行关闭。
pub fn notify_decision(id: &str, decision: &str, by: &str) {
    let target = {
        let guard = match POPUPS.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.iter().find(|p| p.view.id == id).map(|p| p.hwnd)
    };
    if let Some(hwnd) = target {
        if let Ok(mut guard) = POPUPS.lock() {
            if let Some(state) = guard.iter_mut().find(|p| p.hwnd == hwnd) {
                if !state.done {
                    state.pending_decision = Some((decision.to_string(), by.to_string()));
                    drop(guard);
                    unsafe {
                        PostMessageW(hwnd, WM_APP_POPUP_CLOSE, 0, 0);
                    }
                }
            }
        }
    }
}

pub fn pending_count() -> usize {
    POPUPS.lock().map(|g| g.len()).unwrap_or(0)
}

pub fn refresh_tip() {
    let count = pending_count();
    if count == 0 {
        crate::tray::set_tip("管理员命令审批网关 · 空闲");
    } else {
        crate::tray::set_tip(&format!("有 {} 条命令等待批准", count));
    }
}

pub fn spawn_ui_thread() {
    thread::Builder::new()
        .name("approval-ui".into())
        .spawn(|| crate::tray::run_message_loop())
        .expect("无法创建界面线程");
    crate::tray::wait_ready();
}
