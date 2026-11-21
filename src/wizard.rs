//! 首次启动初始化向导：三页，自绘，支持鼠标与 Enter/Space/Esc。

use crate::config::{acl_is_loose, default_whitelist, home_dir, save_config, Config};
use crate::win::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const CLASS: &str = "AdminGatewayWizard";
const STEPS: usize = 3;

#[derive(Clone, Copy, Default)]
struct Rects {
    next: Rect,
    prev: Rect,
    cancel: Rect,
    modes: [Rect; 3],
    check: Rect,
}

struct WizardState {
    dpi: u32,
    fonts: Fonts,
    step: usize,
    mode: String,
    use_defaults: bool,
    acl_loose: bool,
    error: String,
    rects: Rects,
}

unsafe impl Send for WizardState {}
unsafe impl Sync for WizardState {}

static STATE: Mutex<Option<WizardState>> = Mutex::new(None);
static FINISHED: AtomicBool = AtomicBool::new(false);
static REGISTERED: OnceLock<()> = OnceLock::new();

fn register() {
    REGISTERED.get_or_init(|| unsafe {
        let class = wide(CLASS);
        let wc = WndClassExW {
            size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            wnd_proc: proc_,
            cls_extra: 0,
            wnd_extra: 0,
            instance: GetModuleHandleW(std::ptr::null()),
            icon: crate::icon::app_icon(false),
            cursor: LoadCursorW(std::ptr::null_mut(), 32512usize as *const u16),
            background: std::ptr::null_mut(),
            menu: std::ptr::null(),
            class_name: class.as_ptr(),
            icon_small: crate::icon::app_icon(true),
        };
        RegisterClassExW(&wc);
    });
}

fn draw_text(hdc: HDC, value: &str, rect: Rect, font: HFONT, color: u32, flags: u32) {
    text(hdc, value, rect, font, color, flags);
}

fn paint(hwnd: HWND) {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return,
    };
    let dpi = state.dpi;
    let step = state.step;
    let mode = state.mode.clone();
    let use_defaults = state.use_defaults;
    let acl_loose = state.acl_loose;
    let error = state.error.clone();
    let f_title = state.fonts.title;
    let f_h2 = state.fonts.h2;
    let f_body = state.fonts.body;
    let f_small = state.fonts.small;
    let f_mono = state.fonts.mono_small;

    let s = Scale::new(dpi);
    let w = s.p(580);
    let h = s.p(440);
    let mut rects = Rects::default();

    let mut ps = PaintStruct { hdc: std::ptr::null_mut(), erase: 0, rc_paint: Rect::default(), restore: 0, inc_update: 0, reserved: [0; 32] };
    unsafe {
        let hdc = BeginPaint(hwnd, &mut ps);
        fill_rect(hdc, Rect::new(0, 0, w, h), theme::CARD);

        dot(hdc, s.p(30), s.p(30), s.p(6), theme::ACCENT);
        draw_text(hdc, "管理员命令审批网关", Rect::new(s.p(46), s.p(14), w - s.p(130), s.p(44)), f_title, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        draw_text(hdc, &format!("第 {} 步 / 共 {} 步", step + 1, STEPS), Rect::new(w - s.p(150), s.p(14), w - s.p(56), s.p(44)), f_small, theme::TEXT_MID, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);

        let bar_w = (w - s.p(60) - s.p(6) * (STEPS as i32 - 1)) / STEPS as i32;
        for i in 0..STEPS {
            let left = s.p(30) + (bar_w + s.p(6)) * i as i32;
            let rect = Rect::new(left, s.p(52), left + bar_w, s.p(55));
            fill_round(hdc, rect, if i <= step { theme::ACCENT } else { theme::BORDER }, s.p(2));
        }
        fill_rect(hdc, Rect::new(0, s.p(68), w, s.p(69)), theme::BORDER);

        let left = s.p(30);
        let right = w - s.p(30);
        let mut y = s.p(88);

        match step {
            0 => {
                draw_text(hdc, "它解决什么问题", Rect::new(left, y, right, y + s.p(28)), f_title, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                y += s.p(38);
                let lines = [
                    "AI agent 需要管理员权限时，只能向这个程序提交申请，自己永远拿不到管理员权限。",
                    "每条非白名单命令都会弹出窗口，由你逐字核对后点「批准一次」才会执行。",
                    "拒绝、关闭窗口、等待超时，结果都是不执行。",
                ];
                for line in lines.iter() {
                    draw_text(hdc, line, Rect::new(left, y, right, y + s.p(42)), f_body, theme::TEXT_MID, DT_LEFT | DT_WORDBREAK);
                    y += s.p(42);
                }
                y += s.p(4);
                let tip = Rect::new(left, y, right, y + s.p(64));
                fill_round(hdc, tip, theme::ACCENT_SOFT, s.p(10));
                draw_text(hdc, "批准开关只在你的鼠标上：程序以管理员运行，", Rect::new(tip.left + s.p(14), tip.top + s.p(9), tip.right - s.p(12), tip.top + s.p(34)), f_body, theme::ACCENT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                draw_text(hdc, "但没有任何「自动批准」通路，agent 也无法伪造你的点击。", Rect::new(tip.left + s.p(14), tip.top + s.p(33), tip.right - s.p(12), tip.bottom - s.p(6)), f_body, theme::ACCENT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            }
            1 => {
                draw_text(hdc, "策略与数据目录", Rect::new(left, y, right, y + s.p(28)), f_title, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                y += s.p(40);
                draw_text(hdc, "数据目录", Rect::new(left, y, left + s.p(72), y + s.p(20)), f_small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                draw_text(hdc, &home_dir().display().to_string(), Rect::new(left + s.p(78), y, right, y + s.p(20)), f_mono, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS);
                y += s.p(26);
                draw_text(hdc, "目录权限", Rect::new(left, y, left + s.p(72), y + s.p(20)), f_small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                let (label, color) = if acl_loose {
                    ("普通用户可写，策略可能被改写（不安全）", theme::DANGER)
                } else {
                    ("仅管理员可写，agent 改不了策略", theme::OK)
                };
                draw_text(hdc, label, Rect::new(left + s.p(78), y, right, y + s.p(20)), f_small, color, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                y += s.p(36);

                draw_text(hdc, "审批模式", Rect::new(left, y, right, y + s.p(24)), f_h2, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                y += s.p(30);
                let modes = [("whitelist", "白名单放行"), ("strict", "全部审批"), ("deny", "免打扰")];
                let seg = (right - left - s.p(16)) / 3;
                for (i, (key, label)) in modes.iter().enumerate() {
                    let l = left + (seg + s.p(8)) * i as i32;
                    let rect = Rect::new(l, y, l + seg, y + s.p(36));
                    rects.modes[i] = rect;
                    let active = mode == *key;
                    fill_round(hdc, rect, if active { theme::ACCENT } else { theme::CARD }, s.p(8));
                    if !active {
                        stroke_round(hdc, rect, theme::BORDER, s.p(8), s.p(1));
                    }
                    draw_text(hdc, label, rect, f_body, if active { theme::CARD } else { theme::TEXT_MID }, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                }
                y += s.p(50);
                let desc = match mode.as_str() {
                    "strict" => "所有命令都弹窗，最严格，适合首次使用或处理敏感操作。",
                    "deny" => "全部自动拒绝，适合离开电脑时，agent 提交什么都不会执行。",
                    _ => "白名单内的只读命令直接放行，其余命令弹窗等你确认。推荐日常使用。",
                };
                draw_text(hdc, desc, Rect::new(left, y, right, y + s.p(42)), f_body, theme::TEXT_MID, DT_LEFT | DT_WORDBREAK);
            }
            _ => {
                draw_text(hdc, "默认白名单", Rect::new(left, y, right, y + s.p(28)), f_title, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                y += s.p(36);
                draw_text(hdc, "下面这些命令会直接放行、不弹窗。都是只读操作，可随时在管理面板里增删。", Rect::new(left, y, right, y + s.p(38)), f_body, theme::TEXT_MID, DT_LEFT | DT_WORDBREAK);
                y += s.p(44);

                let entries = default_whitelist();
                let list = Rect::new(left, y, right, y + s.p(118));
                fill_round(hdc, list, theme::CARD_ALT, s.p(10));
                stroke_round(hdc, list, theme::BORDER, s.p(10), s.p(1));
                for (i, entry) in entries.iter().enumerate() {
                    let ly = list.top + s.p(8) + (i as i32) * s.p(26);
                    if ly + s.p(22) > list.bottom {
                        break;
                    }
                    dot(hdc, list.left + s.p(16), ly + s.p(10), s.p(2), theme::TEXT_MID);
                    draw_text(hdc, &entry.name, Rect::new(list.left + s.p(26), ly, list.left + s.p(168), ly + s.p(20)), f_body, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                    draw_text(hdc, &entry.exe, Rect::new(list.left + s.p(174), ly, list.right - s.p(12), ly + s.p(20)), f_mono, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS);
                }
                y = list.bottom + s.p(18);

                let check = Rect::new(left, y, left + s.p(18), y + s.p(18));
                rects.check = check;
                fill_round(hdc, check, if use_defaults { theme::ACCENT } else { theme::CARD }, s.p(4));
                if use_defaults {
                    draw_text(hdc, "✓", check, f_small, theme::CARD, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                } else {
                    stroke_round(hdc, check, theme::BORDER, s.p(4), s.p(1));
                }
                draw_text(hdc, "启用这 4 条默认白名单", Rect::new(check.right + s.p(10), y - s.p(2), right, y + s.p(20)), f_body, theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                y += s.p(30);
                draw_text(hdc, "不勾选则一条都不放行，所有命令都走弹窗。", Rect::new(left, y, right, y + s.p(20)), f_small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            }
        }

        let footer_y = h - s.p(60);
        fill_rect(hdc, Rect::new(0, footer_y - s.p(12), w, footer_y - s.p(11)), theme::BORDER);
        let next = Rect::new(w - s.p(30) - s.p(128), footer_y, w - s.p(30), footer_y + s.p(38));
        let prev = Rect::new(next.left - s.p(10) - s.p(96), footer_y, next.left - s.p(10), footer_y + s.p(38));
        let cancel = Rect::new(s.p(30), footer_y, s.p(30) + s.p(72), footer_y + s.p(38));
        rects.next = next;
        rects.prev = prev;
        rects.cancel = cancel;

        let last = step + 1 == STEPS;
        fill_round(hdc, next, theme::ACCENT, s.p(9));
        draw_text(hdc, if last { "完成" } else { "下一步" }, next, f_h2, theme::CARD, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        if step > 0 {
            fill_round(hdc, prev, theme::CARD, s.p(9));
            stroke_round(hdc, prev, theme::BORDER, s.p(9), s.p(1));
            draw_text(hdc, "上一步", prev, f_body, theme::TEXT_MID, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        } else {
            draw_text(hdc, "上一步", prev, f_body, theme::TEXT_MID, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        draw_text(hdc, "取消", cancel, f_body, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

        let hint_rect = Rect::new(cancel.right + s.p(14), footer_y, prev.left - s.p(12), footer_y + s.p(38));
        if !error.is_empty() {
            draw_text(hdc, &error, hint_rect, f_small, theme::DANGER, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        } else if last {
            draw_text(hdc, "完成后写入策略并开始监听", hint_rect, f_small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        } else {
            draw_text(hdc, "Enter 继续　Esc 取消", hint_rect, f_small, theme::TEXT_MID, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        }

        EndPaint(hwnd, &ps);
    }
    state.rects = rects;
}

fn finish() -> Result<(), String> {
    let (mode, use_defaults) = {
        let guard = STATE.lock().map_err(|_| "状态锁失败".to_string())?;
        let st = guard.as_ref().ok_or("窗口已关闭")?;
        (st.mode.clone(), st.use_defaults)
    };
    let mut cfg = Config::default();
    cfg.mode = mode;
    if !use_defaults {
        cfg.whitelist.clear();
    }
    save_config(&cfg)?;
    crate::http_server::log_msg(&format!(
        "初始化完成：模式={} 白名单={} 条 目录={}",
        cfg.mode,
        cfg.whitelist.len(),
        home_dir().display()
    ));
    FINISHED.store(true, Ordering::SeqCst);
    Ok(())
}

fn advance(hwnd: HWND) {
    let mut do_finish = false;
    if let Ok(mut g) = STATE.lock() {
        if let Some(st) = g.as_mut() {
            st.error.clear();
            if st.step + 1 < STEPS {
                st.step += 1;
            } else {
                do_finish = true;
            }
        }
    }
    if do_finish {
        match finish() {
            Ok(_) => unsafe {
                DestroyWindow(hwnd);
            },
            Err(e) => {
                if let Ok(mut g) = STATE.lock() {
                    if let Some(st) = g.as_mut() {
                        st.error = format!("写入失败: {}", e);
                    }
                }
                unsafe {
                    let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
        }
    } else {
        unsafe {
            let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> isize {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_LBUTTONDOWN => {
            let x = (l & 0xffff) as i16 as i32;
            let y = ((l >> 16) & 0xffff) as i16 as i32;
            let mut action = String::new();
            if let Ok(guard) = STATE.lock() {
                if let Some(st) = guard.as_ref() {
                    let r = st.rects;
                    if r.next.contains(x, y) {
                        action = "next".into();
                    } else if r.prev.contains(x, y) && st.step > 0 {
                        action = "prev".into();
                    } else if r.cancel.contains(x, y) {
                        action = "cancel".into();
                    } else if st.step == 2 && r.check.contains(x, y) {
                        action = "toggle".into();
                    } else if st.step == 1 {
                        for i in 0..3 {
                            if r.modes[i].contains(x, y) {
                                action = format!("mode{}", i);
                            }
                        }
                    }
                }
            }
            if action == "next" {
                advance(hwnd);
            } else if action == "prev" {
                if let Ok(mut g) = STATE.lock() {
                    if let Some(st) = g.as_mut() {
                        st.step = st.step.saturating_sub(1);
                        st.error.clear();
                    }
                }
                let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
            } else if action == "cancel" {
                DestroyWindow(hwnd);
            } else if action == "toggle" {
                if let Ok(mut g) = STATE.lock() {
                    if let Some(st) = g.as_mut() {
                        st.use_defaults = !st.use_defaults;
                    }
                }
                let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
            } else if action.starts_with("mode") {
                let idx = action[4..].parse::<usize>().unwrap_or(0).min(2);
                if let Ok(mut g) = STATE.lock() {
                    if let Some(st) = g.as_mut() {
                        st.mode = ["whitelist", "strict", "deny"][idx].to_string();
                    }
                }
                let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_KEYDOWN => {
            let key = w as u32;
            if key == 13 || key == 32 {
                advance(hwnd);
            } else if key == 27 {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            if let Ok(mut g) = STATE.lock() {
                if let Some(st) = g.take() {
                    st.fonts.free();
                }
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

/// 显示向导，阻塞到用户完成或取消。返回 true 表示已写入配置。
pub fn run() -> bool {
    register();
    FINISHED.store(false, Ordering::SeqCst);
    unsafe {
        let s = Scale::new(96);
        let w = s.p(580);
        let h = s.p(440);
        let hwnd = CreateWindowExW(
            0x00000008, // WS_EX_TOPMOST：首次设置必须压在其它窗口之上
            wide(CLASS).as_ptr(),
            wide("初始化设置").as_ptr(),
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
            return false;
        }
        let dpi = GetDpiForWindow(hwnd).max(96);
        let s = Scale::new(dpi);
        let w = s.p(580);
        let h = s.p(440);
        let mut monitor = MonitorInfo { size: 0, monitor: Rect::default(), work: Rect::default(), flags: 0 };
        monitor.size = std::mem::size_of::<MonitorInfo>() as u32;
        let (mut x, mut y) = ((GetSystemMetrics(0) - w) / 2, (GetSystemMetrics(1) - h) / 2);
        if GetMonitorInfoW(MonitorFromWindow(hwnd, 2), &mut monitor) != 0 {
            x = (monitor.work.left + monitor.work.right - w) / 2;
            y = (monitor.work.top + monitor.work.bottom - h) / 2;
        }
        SetWindowPos(hwnd, std::ptr::null_mut(), x.max(10), y.max(10), w, h, SWP_NOZORDER);
        apply_window_chrome(hwnd, 14);

        let mut state = WizardState {
            dpi,
            fonts: Fonts::new(dpi),
            step: 0,
            mode: "whitelist".to_string(),
            use_defaults: true,
            acl_loose: false,
            error: String::new(),
            rects: Rects::default(),
        };
        state.acl_loose = acl_is_loose(&home_dir());
        if let Ok(mut g) = STATE.lock() {
            *g = Some(state);
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        let _ = InvalidateRect(hwnd, std::ptr::null(), 0);
        UpdateWindow(hwnd);

        let mut msg: Msg = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    FINISHED.load(Ordering::SeqCst)
}
