//! 托盘图标 + 隐藏的消息窗口，作为整个界面线程的消息泵。

use crate::popup::{show_popup, PopupView, WM_APP_SHOW, WM_APP_TRAY};
use crate::win::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static CONTROLLER: OnceLock<usize> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);
static TIP: Mutex<String> = Mutex::new(String::new());

const ID_PANEL: usize = 1;
const ID_EXIT: usize = 3;
const ID_MODE_WHITELIST: usize = 10;
const ID_MODE_STRICT: usize = 11;
const ID_MODE_DENY: usize = 12;
const TRAY_ID: u32 = 1;
const WM_APP_TIP: u32 = 0x8000 + 7;

fn controller() -> Option<HWND> {
    CONTROLLER.get().map(|v| *v as HWND)
}

fn tray_data(hwnd: HWND) -> NotifyIconDataW {
    unsafe {
        let mut data: NotifyIconDataW = std::mem::zeroed();
        data.size = std::mem::size_of::<NotifyIconDataW>() as u32;
        data.hwnd = hwnd;
        data.id = TRAY_ID;
        data.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.callback_message = WM_APP_TRAY;
        // Windows 自带的盾牌徽标，和 UAC 提示上一模一样
        data.icon = crate::icon::tray();
        if data.icon.is_null() {
            data.icon = LoadIconW(std::ptr::null_mut(), 32512usize as *const u16);
        }
        data
    }
}

fn copy_tip(data: &mut NotifyIconDataW, text: &str) {
    let wide_text = wide(text);
    let n = wide_text.len().min(data.tip.len());
    data.tip[..n].copy_from_slice(&wide_text[..n]);
}

pub fn set_tip(text: &str) {
    if let Ok(mut guard) = TIP.lock() {
        *guard = text.to_string();
    }
    if let Some(hwnd) = controller() {
        unsafe {
            let mut data = tray_data(hwnd);
            data.flags = NIF_TIP;
            copy_tip(&mut data, text);
            Shell_NotifyIconW(NIM_MODIFY, &mut data);
        }
    }
}

pub fn wait_ready() {
    let mut spins = 0;
    while !READY.load(Ordering::SeqCst) && spins < 3000 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        spins += 1;
    }
}

/// 把弹窗请求投递到界面线程创建。窗口必须活在会跑消息循环的线程上，
/// 否则请求线程拿到结果前一直阻塞，窗口就是「未响应」。
pub fn post_popup(view: PopupView) {
    match controller() {
        Some(hwnd) => unsafe {
            let boxed = Box::new(view);
            let raw = Box::into_raw(boxed);
            if PostMessageW(hwnd, WM_APP_SHOW, 0, raw as LPARAM) == 0 {
                // 投递失败：收回所有权并拒绝，避免请求悬着
                let reclaimed = Box::from_raw(raw);
                let _ = reclaimed.decision.send(("denied".into(), "ui-unavailable".into()));
            }
        },
        None => {
            let _ = view.decision.send(("denied".into(), "ui-unavailable".into()));
        }
    }
}

/// 托盘提示的更新同样放回界面线程，避免跨线程操作 Shell 通知区。
pub fn post_refresh_tip() {
    if let Some(hwnd) = controller() {
        unsafe {
            PostMessageW(hwnd, WM_APP_TIP, 0, 0);
        }
    }
}

pub fn request_exit() {
    if let Some(hwnd) = controller() {
        unsafe {
            PostMessageW(hwnd, WM_COMMAND, ID_EXIT, 0);
        }
    }
}

unsafe extern "system" fn controller_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> isize {
    match msg {
        WM_APP_SHOW => {
            let boxed = Box::from_raw(l as *mut PopupView);
            show_popup(*boxed);
            0
        }
        WM_APP_TIP => {
            crate::popup::refresh_tip();
            0
        }
        WM_APP_TRAY => {
            let event = l as u32;
            if event == WM_RBUTTONUP {
                let mut point = Point { x: 0, y: 0 };
                GetCursorPos(&mut point);
                SetForegroundWindow(hwnd);
                let menu = CreatePopupMenu();
                let mode = crate::config::load_config().mode;
                let pending = crate::popup::pending_count();
                let headline = if pending == 0 {
                    "管理员命令审批网关 · 空闲".to_string()
                } else {
                    format!("{} 条命令等待批准", pending)
                };
                AppendMenuW(menu, 0x0, 0, wide(&headline).as_ptr());
                AppendMenuW(menu, 0x800, 0, std::ptr::null());
                AppendMenuW(menu, 0x0, ID_PANEL, wide("打开管理面板…").as_ptr());
                // 审批模式收进二级菜单，弹窗里不再放一排按钮
                let submenu = CreatePopupMenu();
                for (id, key, label) in [
                    (ID_MODE_WHITELIST, "whitelist", "白名单放行"),
                    (ID_MODE_STRICT, "strict", "全部审批"),
                    (ID_MODE_DENY, "deny", "免打扰"),
                ] {
                    let flags = if mode == key { 0x8 } else { 0x0 }; // MF_CHECKED
                    AppendMenuW(submenu, flags, id, wide(label).as_ptr());
                }
                AppendMenuW(menu, 0x10, submenu as usize, wide("审批模式").as_ptr());
                AppendMenuW(menu, 0x800, 0, std::ptr::null());
                AppendMenuW(menu, 0x0, ID_EXIT, wide("退出").as_ptr());
                let cmd = TrackPopupMenuEx(menu, 0x0100 | 0x0002, point.x, point.y, hwnd, std::ptr::null_mut());
                DestroyMenu(menu);
                PostMessageW(hwnd, WM_NULL, 0, 0);
                match cmd as usize {
                    ID_PANEL => crate::panel::show(),
                    ID_EXIT => crate::request_exit(),
                    ID_MODE_WHITELIST => {
                        let _ = crate::config::update_mode("whitelist");
                        crate::popup::refresh_tip();
                    }
                    ID_MODE_STRICT => {
                        let _ = crate::config::update_mode("strict");
                        crate::popup::refresh_tip();
                    }
                    ID_MODE_DENY => {
                        let _ = crate::config::update_mode("deny");
                        crate::popup::refresh_tip();
                    }
                    _ => {}
                }
            } else if event == WM_LBUTTONDBLCLK {
                crate::panel::show();
            }
            0
        }
        WM_COMMAND => {
            match (w & 0xffff) as usize {
                ID_EXIT => {
                    crate::EXIT_REQUESTED.store(true, Ordering::SeqCst);
                    remove_tray(hwnd);
                    PostQuitMessage(0);
                }
                ID_PANEL => crate::panel::show(),
                _ => {}
            }
            0
        }
        WM_CLOSE => 0,
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn add_tray(hwnd: HWND) {
    unsafe {
        let mut data = tray_data(hwnd);
        let tip = TIP.lock().map(|t| t.clone()).unwrap_or_default();
        let text = if tip.is_empty() { "管理员命令审批网关 · 空闲".to_string() } else { tip };
        copy_tip(&mut data, &text);
        let added = Shell_NotifyIconW(NIM_ADD, &mut data);
        crate::http_server::log_msg(&format!(
            "托盘图标注册={} 图标句柄={}",
            added,
            if data.icon.is_null() { "空（回退默认图标）".to_string() } else { format!("{:p}", data.icon) }
        ));
    }
}

fn remove_tray(hwnd: HWND) {
    unsafe {
        let mut data = tray_data(hwnd);
        Shell_NotifyIconW(NIM_DELETE, &mut data);
    }
}

pub fn run_message_loop() {
    unsafe {
        let class_name = wide("AdminGatewayController");
        let class = WndClassExW {
            size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            wnd_proc: controller_proc,
            cls_extra: 0,
            wnd_extra: 0,
            instance: GetModuleHandleW(std::ptr::null()),
            icon: crate::icon::shield(false),
            cursor: std::ptr::null_mut(),
            background: std::ptr::null_mut(),
            menu: std::ptr::null(),
            class_name: class_name.as_ptr(),
            icon_small: crate::icon::shield(true),
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(
            0x00000080, // WS_EX_TOOLWINDOW，不占任务栏
            class_name.as_ptr(),
            wide("AdminGateway").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        );
        let _ = CONTROLLER.set(hwnd as usize);
        add_tray(hwnd);
        crate::panel::preload();
        READY.store(true, Ordering::SeqCst);
        let mut msg: Msg = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
