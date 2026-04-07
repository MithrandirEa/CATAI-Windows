// system/mouse.rs — Hook souris bas-niveau WH_MOUSE_LL pour le drag des chats.
// Le hook est installé sur le thread UI, pas sur tokio.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::{
    core::Result,
    Win32::{
        Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK,
            MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
        },
    },
};

// HHOOK global partagé via AtomicIsize (HHOOK est un pointeur opaque).
static GLOBAL_HOOK: AtomicIsize = AtomicIsize::new(0);

/// Callback appelé par Windows pour chaque événement souris.
/// Défini par l'application — à adapter selon les besoins.
pub type MouseHookFn = fn(msg: u32, pt_x: i32, pt_y: i32) -> bool;

// Pointeur vers le callback applicatif.
static mut HOOK_CB: Option<MouseHookFn> = None;

/// Installe le hook souris bas-niveau.
/// `callback` reçoit (WM_*, x, y) et retourne `true` pour bloquer l'event.
pub unsafe fn install_mouse_hook(callback: MouseHookFn) -> Result<()> {
    HOOK_CB = Some(callback);
    let hinstance = HINSTANCE(GetModuleHandleW(None)?.0);
    let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), Some(hinstance), 0)?;;
    GLOBAL_HOOK.store(hook.0 as isize, Ordering::Relaxed);
    Ok(())
}

/// Désinstalle le hook.
pub unsafe fn remove_mouse_hook() {
    let raw = GLOBAL_HOOK.swap(0, Ordering::Relaxed);
    if raw != 0 {
        let _ = UnhookWindowsHookEx(HHOOK(raw as *mut _));
    }
}

unsafe extern "system" fn low_level_mouse_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        if let Some(cb) = HOOK_CB {
            let info = &*(l_param.0 as *const MSLLHOOKSTRUCT);
            let block = cb(w_param.0 as u32, info.pt.x, info.pt.y);
            if block {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(Some(HHOOK(GLOBAL_HOOK.load(Ordering::Relaxed) as *mut _)), n_code, w_param, l_param)
}
