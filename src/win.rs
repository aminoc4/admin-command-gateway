//! Win32 基础封装：FFI 声明、DPI、字体、绘制原语。所有界面模块共用。
#![allow(non_snake_case, dead_code)]

use std::ffi::c_void;

pub type HWND = *mut c_void;
pub type HDC = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HPEN = *mut c_void;
pub type HFONT = *mut c_void;
pub type HMENU = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type WPARAM = usize;
pub type LPARAM = isize;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(l: i32, t: i32, r: i32, b: i32) -> Self {
        Rect { left: l, top: t, right: r, bottom: b }
    }
    pub fn w(&self) -> i32 {
        self.right - self.left
    }
    pub fn h(&self) -> i32 {
        self.bottom - self.top
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
    pub fn inset(&self, d: i32) -> Self {
        Rect::new(self.left + d, self.top + d, self.right - d, self.bottom - d)
    }
}

#[repr(C)]
pub struct PaintStruct {
    pub hdc: HDC,
    pub erase: i32,
    pub rc_paint: Rect,
    pub restore: i32,
    pub inc_update: i32,
    pub reserved: [u8; 32],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct Msg {
    pub hwnd: HWND,
    pub message: u32,
    pub w_param: WPARAM,
    pub l_param: LPARAM,
    pub time: u32,
    pub pt: Point,
}

#[repr(C)]
pub struct WndClassExW {
    pub size: u32,
    pub style: u32,
    pub wnd_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> isize,
    pub cls_extra: i32,
    pub wnd_extra: i32,
    pub instance: HINSTANCE,
    pub icon: *mut c_void,
    pub cursor: *mut c_void,
    pub background: HBRUSH,
    pub menu: *const u16,
    pub class_name: *const u16,
    pub icon_small: *mut c_void,
}

#[repr(C)]
pub struct NotifyIconDataW {
    pub size: u32,
    pub hwnd: HWND,
    pub id: u32,
    pub flags: u32,
    pub callback_message: u32,
    pub icon: *mut c_void,
    pub tip: [u16; 128],
    pub state: u32,
    pub state_mask: u32,
    pub info: [u16; 256],
    pub version_or_timeout: u32,
    pub info_title: [u16; 64],
    pub info_flags: u32,
    pub guid_item: [u8; 16],
    pub balloon_icon: *mut c_void,
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    pub fn LoadLibraryW(name: *const u16) -> HINSTANCE;
    pub fn GetProcAddress(module: HINSTANCE, name: *const i8) -> *mut c_void;
    pub fn GetLastError() -> u32;
    pub fn CloseHandle(h: *mut c_void) -> i32;
    pub fn GetCurrentProcess() -> *mut c_void;
    pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    pub fn GetCurrentProcessId() -> u32;
    pub fn GetFullPathNameW(name: *const u16, len: u32, buf: *mut u16, file_part: *mut *mut u16) -> u32;
    pub fn SearchPathW(path: *const u16, name: *const u16, ext: *const u16, len: u32, buf: *mut u16, part: *mut *mut u16) -> u32;
    pub fn GetFileAttributesW(name: *const u16) -> u32;
    pub fn CreateMutexW(attr: *mut c_void, owner: i32, name: *const u16) -> *mut c_void;
    pub fn MultiByteToWideChar(cp: u32, flags: u32, s: *const u8, cb: i32, out: *mut u16, cch: i32) -> i32;
    pub fn WideCharToMultiByte(cp: u32, flags: u32, s: *const u16, cch: i32, out: *mut u8, cb: i32, def: *const i8, used: *mut i32) -> i32;
    pub fn GetTokenInformation(token: *mut c_void, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(class: *const WndClassExW) -> u16;
    pub fn UnregisterClassW(class: *const u16, instance: HINSTANCE) -> i32;
    pub fn CreateWindowExW(
        ex: u32, class: *const u16, title: *const u16, style: u32,
        x: i32, y: i32, w: i32, h: i32, parent: HWND, menu: HMENU,
        instance: HINSTANCE, param: *mut c_void,
    ) -> HWND;
    pub fn DestroyWindow(hwnd: HWND) -> i32;
    pub fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
    pub fn IsWindow(hwnd: HWND) -> i32;
    pub fn IsWindowVisible(hwnd: HWND) -> i32;
    pub fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    pub fn GetWindowRect(hwnd: HWND, rect: *mut Rect) -> i32;
    pub fn GetClientRect(hwnd: HWND, rect: *mut Rect) -> i32;
    pub fn GetMessageW(msg: *mut Msg, hwnd: HWND, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(msg: *const Msg) -> i32;
    pub fn DispatchMessageW(msg: *const Msg) -> isize;
    pub fn PostQuitMessage(code: i32);
    pub fn DefWindowProcW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> isize;
    pub fn PostMessageW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> i32;
    pub fn SendMessageW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> isize;
    pub fn BeginPaint(hwnd: HWND, paint: *mut PaintStruct) -> HDC;
    pub fn EndPaint(hwnd: HWND, paint: *const PaintStruct) -> i32;
    pub fn InvalidateRect(hwnd: HWND, rect: *const Rect, erase: i32) -> i32;
    pub fn UpdateWindow(hwnd: HWND) -> i32;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
    pub fn SetTimer(hwnd: HWND, id: usize, elapse: u32, proc: *mut c_void) -> usize;
    pub fn KillTimer(hwnd: HWND, id: usize) -> i32;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub fn GetDpiForSystem() -> u32;
    pub fn GetCursorPos(point: *mut Point) -> i32;
    pub fn SetCursorPos(x: i32, y: i32) -> i32;
    pub fn SetForegroundWindow(hwnd: HWND) -> i32;
    pub fn GetForegroundWindow() -> HWND;
    pub fn SetFocus(hwnd: HWND) -> HWND;
    pub fn SetCapture(hwnd: HWND) -> HWND;
    pub fn ReleaseCapture() -> i32;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(menu: HMENU, flags: u32, id: usize, text: *const u16) -> i32;
    pub fn TrackPopupMenuEx(menu: HMENU, flags: u32, x: i32, y: i32, hwnd: HWND, param: *mut c_void) -> i32;
    pub fn DestroyMenu(menu: HMENU) -> i32;
    pub fn LoadIconW(instance: HINSTANCE, icon: *const u16) -> *mut c_void;
    pub fn LoadImageW(instance: HINSTANCE, name: *const u16, kind: u32, cx: i32, cy: i32, flags: u32) -> *mut c_void;
    pub fn LoadCursorW(instance: HINSTANCE, cursor: *const u16) -> *mut c_void;
    pub fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, kind: u32) -> i32;
    pub fn SetWindowTextW(hwnd: HWND, text: *const u16) -> i32;
    pub fn GetWindowTextW(hwnd: HWND, buf: *mut u16, len: i32) -> i32;
    pub fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    pub fn EnableWindow(hwnd: HWND, enable: i32) -> i32;
    pub fn MonitorFromWindow(hwnd: HWND, flags: u32) -> *mut c_void;
    pub fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfo) -> i32;
    pub fn ClientToScreen(hwnd: HWND, point: *mut Point) -> i32;
    pub fn ScreenToClient(hwnd: HWND, point: *mut Point) -> i32;
    pub fn DrawTextW(hdc: HDC, text: *const u16, count: i32, rect: *mut Rect, flags: u32) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateSolidBrush(color: u32) -> HBRUSH;
    pub fn CreatePen(style: i32, width: i32, color: u32) -> HPEN;
    pub fn SelectObject(hdc: HDC, obj: *mut c_void) -> *mut c_void;
    pub fn DeleteObject(obj: *mut c_void) -> i32;
    pub fn RoundRect(hdc: HDC, l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> i32;
    pub fn Ellipse(hdc: HDC, l: i32, t: i32, r: i32, b: i32) -> i32;
    pub fn Rectangle(hdc: HDC, l: i32, t: i32, r: i32, b: i32) -> i32;
    pub fn SetTextColor(hdc: HDC, color: u32) -> u32;
    pub fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    pub fn SetBkColor(hdc: HDC, color: u32) -> u32;
    pub fn CreateFontW(
        height: i32, width: i32, escapement: i32, orientation: i32, weight: i32,
        italic: u32, underline: u32, strikeout: u32, charset: u32,
        out_precision: u32, clip_precision: u32, quality: u32, family: u32, face: *const u16,
    ) -> HFONT;
    pub fn GetTextExtentPoint32W(hdc: HDC, text: *const u16, count: i32, size: *mut Size) -> i32;
    pub fn GetStockObject(index: i32) -> *mut c_void;
}

#[repr(C)]
pub struct Size {
    pub cx: i32,
    pub cy: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct MinMaxInfo {
    pub reserved: Point,
    pub max_size: Point,
    pub max_position: Point,
    pub min_track_x: i32,
    pub min_track_y: i32,
    pub max_track_x: i32,
    pub max_track_y: i32,
}

#[repr(C)]
pub struct MonitorInfo {
    pub size: u32,
    pub monitor: Rect,
    pub work: Rect,
    pub flags: u32,
}

type DwmSetAttribute = unsafe extern "system" fn(HWND, u32, *const c_void, u32) -> i32;

/// 动态解析 dwmapi（GNU 工具链没有 dwmapi 导入库，避免硬链接）
fn dwm_set_attribute(hwnd: HWND, attr: u32, value: *const c_void, size: u32) -> i32 {
    static PROC: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let address = *PROC.get_or_init(|| unsafe {
        let library = LoadLibraryW(wide("dwmapi.dll").as_ptr());
        if library.is_null() {
            return 0;
        }
        GetProcAddress(library, b"DwmSetWindowAttribute\0".as_ptr() as *const i8) as usize
    });
    if address == 0 {
        return -1;
    }
    let function: DwmSetAttribute = unsafe { std::mem::transmute(address) };
    unsafe { function(hwnd, attr, value, size) }
}

#[link(name = "shell32")]
extern "system" {
    pub fn Shell_NotifyIconW(action: u32, data: *mut NotifyIconDataW) -> i32;
    pub fn ShellExecuteW(hwnd: HWND, op: *const u16, file: *const u16, params: *const u16, dir: *const u16, show: i32) -> HINSTANCE;
}

// ---- 常量 ----
pub const WM_PAINT: u32 = 0x000F;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_CTLCOLOREDIT: u32 = 0x0133;
pub const WM_SETFONT: u32 = 0x0030;
pub const WM_GETTEXT: u32 = 0x000D;
pub const WM_SETTEXT: u32 = 0x000C;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_NULL: u32 = 0x0000;

pub const WS_POPUP: u32 = 0x80000000;
pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_CHILD: u32 = 0x40000000;
pub const WS_VISIBLE: u32 = 0x10000000;
pub const WS_TABSTOP: u32 = 0x00010000;
pub const ES_AUTOHSCROLL: u32 = 0x0080;
pub const ES_MULTILINE: u32 = 0x0004;

pub const SW_HIDE: i32 = 0;
pub const SW_SHOW: i32 = 5;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_FRAMECHANGED: u32 = 0x0020;
pub const SWP_SHOWWINDOW: u32 = 0x0040;

pub const DT_LEFT: u32 = 0;
pub const DT_CENTER: u32 = 1;
pub const DT_RIGHT: u32 = 2;
pub const DT_VCENTER: u32 = 4;
pub const DT_WORDBREAK: u32 = 0x10;
pub const DT_SINGLELINE: u32 = 0x20;
pub const DT_END_ELLIPSIS: u32 = 0x8000;
pub const DT_NOPREFIX: u32 = 0x0800;
pub const DT_PATH_ELLIPSIS: u32 = 0x4000;

pub const TRANSPARENT: i32 = 1;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIF_MESSAGE: u32 = 1;
pub const NIF_ICON: u32 = 2;
pub const NIF_TIP: u32 = 4;
pub const NIF_INFO: u32 = 0x10;
pub const NIIF_INFO: u32 = 1;
pub const NIIF_NOSOUND: u32 = 0x10;

pub const INVALID_ATTR: u32 = 0xFFFFFFFF;
pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

pub fn wide(text: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

/// 调色板：浅色、低饱和、层级清楚。
pub mod theme {
    use super::rgb;
    pub const BG: u32 = rgb(247, 248, 250);
    pub const CARD: u32 = rgb(255, 255, 255);
    pub const CARD_ALT: u32 = rgb(249, 250, 252);
    pub const BORDER: u32 = rgb(232, 235, 241);
    pub const TEXT: u32 = rgb(24, 28, 36);
    pub const TEXT_MID: u32 = rgb(90, 98, 112);
    pub const TEXT_FAINT: u32 = rgb(145, 152, 165);
    pub const ACCENT: u32 = rgb(37, 99, 235);
    pub const ACCENT_SOFT: u32 = rgb(237, 243, 255);
    pub const OK: u32 = rgb(22, 138, 74);
    pub const OK_SOFT: u32 = rgb(236, 249, 241);
    pub const DANGER: u32 = rgb(196, 32, 63);
    pub const DANGER_SOFT: u32 = rgb(255, 240, 243);
    pub const WARN: u32 = rgb(180, 110, 8);
    pub const WARN_SOFT: u32 = rgb(255, 248, 232);
    pub const RAIL: u32 = rgb(248, 249, 251);
    pub const HOVER: u32 = rgb(242, 244, 248);
}

/// DPI 缩放：把 96dpi 下的逻辑像素换算成当前窗口的物理像素。
#[derive(Clone, Copy)]
pub struct Scale(pub i32);

impl Scale {
    pub fn new(dpi: u32) -> Self {
        Scale(((dpi as i32) * 100 + 48) / 96)
    }
    pub fn of(hwnd: HWND) -> Self {
        unsafe { Scale::new(GetDpiForWindow(hwnd).max(96)) }
    }
    pub fn p(&self, logical: i32) -> i32 {
        (logical * self.0 + 50) / 100
    }
}

pub fn init_dpi_awareness() -> i32 {
    unsafe {
        let user32 = LoadLibraryW(wide("user32.dll").as_ptr());
        if !user32.is_null() {
            let f = GetProcAddress(user32, b"SetProcessDpiAwarenessContext\0".as_ptr() as *const i8);
            if !f.is_null() {
                let call: unsafe extern "system" fn(isize) -> i32 = std::mem::transmute(f);
                if call(-4) != 0 {
                    return -4;
                }
            }
            let g = GetProcAddress(user32, b"SetProcessDPIAware\0".as_ptr() as *const i8);
            if !g.is_null() {
                let call: unsafe extern "system" fn() -> i32 = std::mem::transmute(g);
                if call() != 0 {
                    return -2;
                }
            }
        }
    }
    unsafe { GetLastError() as i32 }
}

/// 圆角 + 阴影：优先用 DWM（有抗锯齿），失败时退回区域裁剪。
pub fn apply_window_chrome(hwnd: HWND, radius_logical: i32) {
    unsafe {
        let preference: u32 = 2; // DWMWCP_ROUND
        let ok = dwm_set_attribute(hwnd, 33, &preference as *const u32 as *const c_void, 4);
        if ok != 0 {
            let scale = Scale::of(hwnd);
            let rect = {
                let mut r = Rect::default();
                GetWindowRect(hwnd, &mut r);
                r
            };
            let r = scale.p(radius_logical);
            let region = CreateRoundRectRgnFor(0, 0, rect.w(), rect.h(), r * 2, r * 2);
            if !region.is_null() {
                SetWindowRgnCompat(hwnd, region);
            }
        }
    }
}

extern "system" {
    fn SetWindowRgn(hwnd: HWND, region: *mut c_void, redraw: i32) -> i32;
    fn CreateRoundRectRgn(l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> *mut c_void;
}

fn CreateRoundRectRgnFor(l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> *mut c_void {
    unsafe { CreateRoundRectRgn(l, t, r, b, w, h) }
}

fn SetWindowRgnCompat(hwnd: HWND, region: *mut c_void) {
    unsafe {
        SetWindowRgn(hwnd, region, 1);
    }
}

pub struct Fonts {
    pub dpi: u32,
    pub title: HFONT,
    pub h2: HFONT,
    pub body: HFONT,
    pub small: HFONT,
    pub mono: HFONT,
    pub mono_small: HFONT,
    pub icon: HFONT,
}

impl Fonts {
    pub fn new(dpi: u32) -> Self {
        let s = Scale::new(dpi.max(96));
        let ui = wide("Microsoft YaHei UI");
        let mono = wide("Consolas");
        let mk = |px: i32, weight: i32, face: &[u16]| unsafe {
            CreateFontW(-s.p(px), 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr())
        };
        Fonts {
            dpi: dpi.max(96),
            title: mk(20, 700, &ui),
            h2: mk(15, 600, &ui),
            body: mk(14, 400, &ui),
            small: mk(13, 400, &ui),
            mono: mk(13, 400, &mono),
            mono_small: mk(12, 400, &mono),
            icon: mk(15, 400, &ui),
        }
    }

    pub fn free(&self) {
        unsafe {
            for f in [self.title, self.h2, self.body, self.small, self.mono, self.mono_small, self.icon] {
                if !f.is_null() {
                    DeleteObject(f as *mut c_void);
                }
            }
        }
    }
}

pub fn fill_round(hdc: HDC, rect: Rect, color: u32, radius: i32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let pen = CreatePen(0, 1, color);
        let ob = SelectObject(hdc, brush as *mut c_void);
        let op = SelectObject(hdc, pen as *mut c_void);
        RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, radius * 2, radius * 2);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(brush as *mut c_void);
        DeleteObject(pen as *mut c_void);
    }
}

/// 只描边不填充：必须换成 NULL_BRUSH，否则会把整块涂成边框色。
pub fn stroke_round(hdc: HDC, rect: Rect, color: u32, radius: i32, width: i32) {
    unsafe {
        let pen = CreatePen(0, width.max(1), color);
        let ob = SelectObject(hdc, GetStockObject(5 /* NULL_BRUSH */));
        let op = SelectObject(hdc, pen as *mut c_void);
        RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, radius * 2, radius * 2);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(pen as *mut c_void);
    }
}

pub fn fill_rect(hdc: HDC, rect: Rect, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        // GDI Rectangle 会用「当前画笔」描一圈边；不换掉默认 BLACK_PEN，
        // 每次填色都会带出一条黑边（1px 的分隔线会被整个涂黑）。
        let pen = CreatePen(0, 1, color);
        let ob = SelectObject(hdc, brush as *mut c_void);
        let op = SelectObject(hdc, pen as *mut c_void);
        Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(brush as *mut c_void);
        DeleteObject(pen as *mut c_void);
    }
}

pub fn dot(hdc: HDC, cx: i32, cy: i32, r: i32, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let pen = CreatePen(0, 1, color);
        let ob = SelectObject(hdc, brush as *mut c_void);
        let op = SelectObject(hdc, pen as *mut c_void);
        Ellipse(hdc, cx - r, cy - r, cx + r, cy + r);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(brush as *mut c_void);
        DeleteObject(pen as *mut c_void);
    }
}

pub fn text(hdc: HDC, value: &str, rect: Rect, font: HFONT, color: u32, flags: u32) {
    unsafe {
        let old = SelectObject(hdc, font as *mut c_void);
        SetTextColor(hdc, color);
        SetBkMode(hdc, TRANSPARENT);
        let mut r = rect;
        let w = wide(value);
        DrawTextW(hdc, w.as_ptr(), -1, &mut r, DT_NOPREFIX | flags);
        SelectObject(hdc, old);
    }
}

pub fn measure(hdc: HDC, value: &str, font: HFONT) -> i32 {
    unsafe {
        let old = SelectObject(hdc, font as *mut c_void);
        let w = wide(value);
        let mut size = Size { cx: 0, cy: 0 };
        GetTextExtentPoint32W(hdc, w.as_ptr(), (w.len() - 1) as i32, &mut size);
        SelectObject(hdc, old);
        size.cx
    }
}

/// 按宽度折行，返回行数与最大宽度，用于 argv 多行展示。
pub fn wrap_lines(hdc: HDC, value: &str, font: HFONT, max_width: i32, max_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        let mut probe = current.clone();
        probe.push(ch);
        if measure(hdc, &probe, font) > max_width && !current.is_empty() {
            lines.push(current.clone());
            current.clear();
            if lines.len() >= max_lines {
                break;
            }
        }
        current.push(ch);
    }
    if lines.len() < max_lines && !current.is_empty() {
        lines.push(current);
    }
    if lines.len() == max_lines && value.chars().count() > lines.iter().map(|l| l.chars().count()).sum::<usize>() {
        if let Some(last) = lines.last_mut() {
            last.push('…');
        }
    }
    lines
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: *mut c_void = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), 0x0008, &mut token) == 0 {
            return false;
        }
        let mut info = [0u8; 4];
        let mut ret = 0u32;
        let ok = GetTokenInformation(token, 20 /* TokenElevation */, info.as_mut_ptr() as *mut c_void, 4, &mut ret);
        CloseHandle(token);
        ok != 0 && u32::from_ne_bytes(info) != 0
    }
}

pub fn ensure_single_instance(name: &str) -> bool {
    static KEEP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    unsafe {
        let handle = CreateMutexW(std::ptr::null_mut(), 0, wide(name).as_ptr());
        if handle.is_null() {
            return false;
        }
        let already = GetLastError() == 183; // ERROR_ALREADY_EXISTS
        let _ = KEEP.set(handle as usize);
        !already
    }
}

pub fn resolve_executable(command: &str) -> Result<String, String> {
    let trimmed = command.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return Err("命令不能为空".into());
    }
    let name = wide(trimmed);
    let is_abs = std::path::Path::new(trimmed).is_absolute();
    let mut buf = vec![0u16; 1024];
    unsafe {
        let mut found: Option<String> = None;
        if is_abs {
            let n = GetFullPathNameW(name.as_ptr(), buf.len() as u32, buf.as_mut_ptr(), std::ptr::null_mut());
            if n > 0 && (n as usize) < buf.len() {
                found = Some(from_wide(&buf));
            } else {
                found = Some(trimmed.to_string());
            }
        } else {
            let ext = wide(".exe");
            let n = SearchPathW(std::ptr::null(), name.as_ptr(), ext.as_ptr(), buf.len() as u32, buf.as_mut_ptr(), std::ptr::null_mut());
            if n > 0 && (n as usize) < buf.len() {
                let raw = from_wide(&buf);
                let n2 = GetFullPathNameW(wide(&raw).as_ptr(), buf.len() as u32, buf.as_mut_ptr(), std::ptr::null_mut());
                found = Some(if n2 > 0 { from_wide(&buf) } else { raw });
            }
        }
        let path = match found {
            Some(p) => p,
            None => return Err(format!("找不到可执行文件: {}", trimmed)),
        };
        if GetFileAttributesW(wide(&path).as_ptr()) == INVALID_ATTR {
            return Err(format!("可执行文件不存在: {}", path));
        }
        Ok(path)
    }
}
