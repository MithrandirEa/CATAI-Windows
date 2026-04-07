// ui/tray.rs — Icône dans la zone de notification (system tray).
// Shell_NotifyIconW + menu contextuel TrackPopupMenu.

use windows::{
    core::Result,
    Win32::{
        Foundation::{HWND, LPARAM, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NIM_MODIFY, NOTIFYICONDATAW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow,
                TrackPopupMenu, HMENU, LoadIconW, IDI_APPLICATION,
                MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
            },
        },
    },
};

use crate::app::WM_TRAY;

pub const MENU_SETTINGS: u32 = 1001;
pub const MENU_QUIT: u32 = 1002;

/// Ajoute l'icône dans la zone de notification.
pub unsafe fn add_tray_icon(hwnd: HWND) -> Result<()> {
    let hinstance = GetModuleHandleW(None)?;
    let hicon = LoadIconW(None, IDI_APPLICATION)?;

    let mut tip = [0u16; 128];
    let name = "CATAI\0";
    for (i, c) in name.encode_utf16().enumerate() {
        tip[i] = c;
    }

    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: hicon,
        szTip: tip,
        ..Default::default()
    };

    Shell_NotifyIconW(NIM_ADD, &mut nid).ok()
}

/// Met à jour le tooltip.
pub unsafe fn update_tray_tip(hwnd: HWND, tip: &str) -> Result<()> {
    let mut tip_buf = [0u16; 128];
    for (i, c) in tip.encode_utf16().take(127).enumerate() {
        tip_buf[i] = c;
    }
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_TIP,
        szTip: tip_buf,
        ..Default::default()
    };
    Shell_NotifyIconW(NIM_MODIFY, &mut nid).ok()
}

/// Supprime l'icône tray.
pub unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
}

/// Affiche le menu contextuel au curseur.
pub unsafe fn show_context_menu(hwnd: HWND, lang: &str) {
    let hmenu = CreatePopupMenu().unwrap();

    let settings = wstr("Réglages...");
    let quit = wstr("Quitter");
    let (s, q) = match lang {
        "en" => (wstr("Settings..."), wstr("Quit")),
        "es" => (wstr("Ajustes..."), wstr("Salir")),
        _ => (settings, quit),
    };

    let _ = AppendMenuW(hmenu, MF_STRING, MENU_SETTINGS as usize, windows::core::PCWSTR(s.as_ptr()));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(hmenu, MF_STRING, MENU_QUIT as usize, windows::core::PCWSTR(q.as_ptr()));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    // Requis pour que le menu se ferme correctement
    SetForegroundWindow(hwnd);

    TrackPopupMenu(
        hmenu,
        TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        None,
        hwnd,
        None,
    );

    DestroyMenu(hmenu);
}

fn wstr(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
