//! 图标：直接取 Windows 自带的「管理员」盾牌（UAC 徽标），不自绘任何图形。

use std::ffi::c_void;

#[repr(C)]
struct StockIconInfo {
    size: u32,
    icon: *mut c_void,
    sys_index: i32,
    index: i32,
    path: [u16; 260],
}

#[link(name = "shell32")]
extern "system" {
    fn SHGetStockIconInfo(id: u32, flags: u32, info: *mut StockIconInfo) -> i32;
}

const SIID_SHIELD: u32 = 77;
const SHGSI_ICON: u32 = 0x0000_0100;
const SHGSI_LARGEICON: u32 = 0x0000_0000;
const SHGSI_SMALLICON: u32 = 0x0000_0001;

/// 取系统盾牌图标。返回的 HICON 由系统缓存持有，不要自行 DestroyIcon。
pub fn shield(small: bool) -> *mut c_void {
    unsafe {
        let mut info: StockIconInfo = std::mem::zeroed();
        info.size = std::mem::size_of::<StockIconInfo>() as u32;
        let flags = SHGSI_ICON | if small { SHGSI_SMALLICON } else { SHGSI_LARGEICON };
        if SHGetStockIconInfo(SIID_SHIELD, flags, &mut info) == 0 {
            info.icon
        } else {
            std::ptr::null_mut()
        }
    }
}

/// 托盘用：取大图标交给外壳缩放，比直接要 16px 再放大清晰。
pub fn tray() -> *mut c_void {
    let icon = shield(false);
    if icon.is_null() {
        shield(true)
    } else {
        icon
    }
}

/// exe 内嵌的盾牌图标（资源 ID 1，与托盘系统盾牌同源同形）。
/// 窗口类、任务栏、资源管理器里都显示这一个。
pub fn app_icon(small: bool) -> *mut c_void {
    use crate::win::{GetModuleHandleW, GetSystemMetrics, LoadImageW};
    unsafe {
        // 49 = SM_CXICON（大图标），50 = SM_CYSMICON（小图标）
        let size = if small { GetSystemMetrics(50) } else { GetSystemMetrics(49) };
        LoadImageW(
            GetModuleHandleW(std::ptr::null_mut()),
            1usize as *const u16, // MAKEINTRESOURCE(1)
            1,                    // IMAGE_ICON
            size,
            size,
            0,
        )
    }
}
