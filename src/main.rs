#![cfg_attr(windows, windows_subsystem = "windows")]

mod audit;
mod config;
mod http_server;
mod icon;
mod mcp;
mod panel;
mod platform;
mod popup;
mod tray;
mod win;
mod wizard;

use rand::RngCore;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static ELEVATED: AtomicBool = AtomicBool::new(false);

pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        month,
        day,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

pub fn random_hex(byte_count: usize) -> String {
    let mut buf = vec![0u8; byte_count];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn request_exit() {
    EXIT_REQUESTED.store(true, Ordering::SeqCst);
    tray::request_exit();
}

fn fail(title: &str, message: &str) -> ! {
    unsafe {
        win::MessageBoxW(
            std::ptr::null_mut(),
            win::wide(message).as_ptr(),
            win::wide(title).as_ptr(),
            0x10,
        );
    }
    std::process::exit(1);
}

/// 双击启动时：把自己以管理员身份重新拉起，再带 serve 参数进入正常流程。
fn relaunch_as_admin(params: &str) -> ! {
    let exe = env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let code = unsafe {
        win::ShellExecuteW(
            std::ptr::null_mut(),
            win::wide("runas").as_ptr(),
            win::wide(&exe).as_ptr(),
            win::wide(params).as_ptr(),
            std::ptr::null(),
            1,
        )
    } as isize;
    if code <= 32 {
        let text = if code == 5 {
            "已取消提权，程序没有启动。\n\n它需要管理员权限才能执行命令，\n再次双击并在 UAC 窗口点「是」即可。".to_string()
        } else {
            format!("无法请求管理员权限（错误码 {}）。\n\n请右键 admin-gateway.exe → 以管理员身份运行。", code)
        };
        unsafe {
            win::MessageBoxW(
                std::ptr::null_mut(),
                win::wide(&text).as_ptr(),
                win::wide("未启动").as_ptr(),
                0x30,
            );
        }
        std::process::exit(0);
    }
    std::process::exit(0);
}

fn serve(args: &[String]) {
    let dpi_mode = win::init_dpi_awareness();
    let elevated = win::is_elevated();
    ELEVATED.store(elevated, Ordering::SeqCst);
    let allow_unelevated = args.iter().any(|a| a == "--allow-unelevated");
    if !elevated && !allow_unelevated {
        fail(
            "需要管理员权限",
            "审批网关必须以管理员身份运行，否则弹窗可被同用户进程伪造点击。\n\n直接双击 admin-gateway.exe 会自动请求提权；\n若 UAC 被拒绝，请右键 → 以管理员身份运行。",
        );
    }
    // 单实例互斥名可通过环境变量覆盖，方便在同一台机器上并行调试
    let instance = env::var("ADMIN_GATEWAY_INSTANCE")
        .unwrap_or_else(|_| "Local\\AdminGatewaySingleInstance".to_string());
    if !win::ensure_single_instance(&instance) {
        fail("已在运行", "已有一个审批网关实例在运行。\n\n请在系统托盘双击图标打开管理面板。");
    }
    if let Err(e) = config::ensure_home(elevated) {
        fail("数据目录不可用", &e);
    }

    let force_wizard = args.iter().any(|a| a == "--wizard");
    let first_run = !config::config_path().exists();
    if force_wizard || first_run {
        if !wizard::run() {
            http_server::log_msg("初始化被取消，进程退出");
            std::process::exit(0);
        }
    }
    config::ensure_config_file();

    popup::spawn_ui_thread();
    http_server::log_msg(&format!(
        "DPI={} 系统DPI={} 提权={}",
        if dpi_mode <= 0 { dpi_mode.to_string() } else { format!("失败({})", dpi_mode) },
        unsafe { win::GetDpiForSystem() },
        elevated
    ));
    http_server::run_server();
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("mcp") => mcp::run_mcp(),
        Some("serve") => serve(&args),
        Some("wizard") => {
            // 重新运行初始化向导
            if win::is_elevated() {
                let mut with = args.clone();
                with.push("--wizard".to_string());
                serve(&with);
            } else {
                relaunch_as_admin("serve --wizard");
            }
        }
        Some("panel") => {
            // 直接打开管理面板（不启动服务、不进托盘），便于快速查看与调试
            ELEVATED.store(win::is_elevated(), Ordering::SeqCst);
            win::init_dpi_awareness();
            panel::preload();
            panel::show();
            let mut msg = win::Msg {
                hwnd: std::ptr::null_mut(),
                message: 0,
                w_param: 0,
                l_param: 0,
                time: 0,
                pt: win::Point::default(),
            };
            unsafe {
                while win::GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    win::TranslateMessage(&msg);
                    win::DispatchMessageW(&msg);
                }
            }
        }
        _ => {
            // 无参数 = 双击启动
            if win::is_elevated() {
                serve(&args);
            } else {
                relaunch_as_admin("serve");
            }
        }
    }
}
