// system/windows.rs — Détection des fenêtres actives pour le perching.
// EnumWindows + GetWindowRect → liste des rectangles des fenêtres normales visibles.

use windows::{
    core::{Result, BOOL},
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowRect, IsWindowVisible, GetWindowLongW,
            GWL_STYLE, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_MINIMIZE,
        },
    },
};

/// Retourne les rectangles de toutes les fenêtres normales visibles.
pub fn visible_window_rects() -> Vec<RECT> {
    let mut rects: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut rects as *mut Vec<RECT> as isize),
        );
    }
    rects
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // Ignorer les fenêtres invisibles
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    // Ignorer les ToolWindows (dont nos propres fenêtres chat)
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return BOOL(1);
    }
    // Ignorer les fenêtres minimisées
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    if style & WS_MINIMIZE.0 != 0 {
        return BOOL(1);
    }

    let mut rc = RECT::default();
    if GetWindowRect(hwnd, &mut rc).is_ok() && rc.right > rc.left && rc.bottom > rc.top {
        let list = &mut *(lparam.0 as *mut Vec<RECT>);
        list.push(rc);
    }
    BOOL(1)
}
