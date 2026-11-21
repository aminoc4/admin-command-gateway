use std::process::Command;
use std::os::windows::process::CommandExt;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        multi_byte_str: *const u8,
        cb_multi_byte: i32,
        wide_char_str: *mut u16,
        cch_wide_char: i32,
    ) -> i32;
}

fn decode_with(code_page: u32, data: &[u8]) -> Option<String> {
    #[cfg(windows)]
    unsafe {
        let n = MultiByteToWideChar(code_page, 0, data.as_ptr(), data.len() as i32, std::ptr::null_mut(), 0);
        if n <= 0 {
            return None;
        }
        let mut buf = vec![0u16; n as usize];
        MultiByteToWideChar(code_page, 0, data.as_ptr(), data.len() as i32, buf.as_mut_ptr(), n);
        Some(String::from_utf16_lossy(&buf))
    }
    #[cfg(not(windows))]
    {
        let _ = code_page;
        String::from_utf8(data.to_vec()).ok()
    }
}

/// 依次尝试 UTF-8、OEM（控制台默认）、ACP（系统区域设置），取第一个合法结果。
pub fn decode_console(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    if let Ok(text) = std::str::from_utf8(data) {
        return text.to_string();
    }
    const CP_OEMCP: u32 = 1;
    const CP_ACP: u32 = 0;
    for code_page in [CP_OEMCP, CP_ACP] {
        if let Some(text) = decode_with(code_page, data) {
            if !text.contains('\u{FFFD}') {
                return text;
            }
            if code_page == CP_ACP {
                return text;
            }
        }
    }
    String::from_utf8_lossy(data).into_owned()
}

/// 终止整棵进程树（含 cmd /c 这类派生出来的子孙进程）。
pub fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW，别闪控制台
        .output();
}
