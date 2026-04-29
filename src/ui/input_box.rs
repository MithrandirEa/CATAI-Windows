// ui/input_box.rs — Champ de saisie flottant au-dessus du chat actif.
// Apparaît au double-clic ; Entrée = envoyer à Ollama, Échap / perte de focus = fermer.

use std::sync::atomic::{AtomicI64, AtomicIsize, AtomicUsize, Ordering};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        System::Threading::AttachThreadInput,
        UI::{
            Input::KeyboardAndMouse::{keybd_event, SetFocus, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_ESCAPE, VK_MENU, VK_RETURN},
            WindowsAndMessaging::{
                CallWindowProcW, CreateWindowExW, DefWindowProcW, GetForegroundWindow,
                GetParent, GetWindow, GetWindowTextLengthW, GetWindowTextW,
                GetWindowThreadProcessId, GW_CHILD, GWLP_WNDPROC,
                HWND_TOPMOST, PostMessageW, SetForegroundWindow,
                SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
                SW_HIDE, SWP_SHOWWINDOW, WINDOW_EX_STYLE, WA_INACTIVE, WM_ACTIVATE, WM_KEYDOWN,
                WS_BORDER, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                WS_POPUP, WS_VISIBLE,
            },
        },
    },
    core::Result,
};

pub const INPUT_CLASS: windows::core::PCWSTR = windows::core::w!("CATAI_Input");

// Messages internes (plage WM_USER 0x0400–0x7FFF) — utilisés uniquement entre
// la subclass de l'EDIT et la WndProc de la fenêtre InputBox parente.
const WM_INPUT_SEND: u32 = 0x0500;
const WM_INPUT_CANCEL: u32 = 0x0501;

static ORIG_EDIT_PROC: AtomicIsize = AtomicIsize::new(0);
/// Index du chat actif (mis à jour dans show()).
static ACTIVE_CAT_IDX: AtomicUsize = AtomicUsize::new(0);
/// HWND du message window principal (cible du WM_USER_INPUT final).
static MSG_HWND_PTR: AtomicIsize = AtomicIsize::new(0);
/// Horodatage (ms Unix) du dernier appel à show() — protège contre la fermeture prématurée
/// par WM_ACTIVATE(WA_INACTIVE) lors de l'acquisition du focus.
static SHOW_TIMESTAMP_MS: AtomicI64 = AtomicI64::new(0);

const BOX_W: i32 = 260;
const BOX_H: i32 = 34;

pub struct InputBox {
    pub hwnd: HWND,
    edit_hwnd: HWND,
}

// SAFETY : InputBox n'est accédé que depuis le thread UI Win32.
unsafe impl Send for InputBox {}
unsafe impl Sync for InputBox {}

impl InputBox {
    /// Crée la fenêtre de saisie, cachée par défaut.
    pub unsafe fn new(msg_hwnd: HWND) -> Result<Self> {
        let hinstance = HINSTANCE(GetModuleHandleW(None)?.0);
        MSG_HWND_PTR.store(msg_hwnd.0 as isize, Ordering::Relaxed);

        // Fenêtre conteneur (sans titre, sans barre des tâches)
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0),
            INPUT_CLASS,
            windows::core::w!(""),
            WS_POPUP | WS_BORDER,
            0, 0, BOX_W, BOX_H,
            None, None, Some(hinstance), None,
        )?;

        // Contrôle EDIT enfant qui remplit la zone cliente
        let edit_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_CLIENTEDGE.0),
            windows::core::w!("EDIT"),
            windows::core::w!(""),
            WS_CHILD | WS_VISIBLE,
            2, 2, BOX_W - 4, BOX_H - 4,
            Some(hwnd), None, Some(hinstance), None,
        )?;

        // Sous-classer l'EDIT pour intercepter VK_RETURN, VK_ESCAPE et WM_KILLFOCUS
        let orig = SetWindowLongPtrW(edit_hwnd, GWLP_WNDPROC, edit_subclass_proc as isize);
        ORIG_EDIT_PROC.store(orig, Ordering::Relaxed);

        Ok(Self { hwnd, edit_hwnd })
    }

    /// Positionne et affiche la boîte au-dessus du chat, donne le focus à l'EDIT.
    pub unsafe fn show(&self, cat_x: i32, cat_y: i32, cat_size: i32, cat_idx: usize) {
        ACTIVE_CAT_IDX.store(cat_idx, Ordering::Relaxed);
        // Enregistrer l'horodatage AVANT tout appel Win32 pour protéger WM_ACTIVATE.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        SHOW_TIMESTAMP_MS.store(now_ms, Ordering::Relaxed);

        let x = cat_x - (BOX_W - cat_size) / 2;
        // Si la taskbar est en haut de l'écran, placer la boîte sous le chat.
        let y = {
            let above = cat_y - BOX_H - 6;
            if above < 0 { cat_y + cat_size + 6 } else { above }
        };
        // Montrer + repositionner + assurer le z-order topmost en une passe
        let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, BOX_W, BOX_H, SWP_SHOWWINDOW);
        let _ = SetWindowTextW(self.edit_hwnd, windows::core::w!(""));

        // ALT trick : simuler une pression/relâchement de Alt pour débloquer la politique
        // foreground de Windows (nécessaire quand l'appli courante ne cède pas le focus).
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);

        // Technique AttachThreadInput : voler le focus même si notre processus n'est
        // pas le foreground (les fenêtres WS_EX_TOOLWINDOW ne le deviennent pas seules).
        let fg_hwnd = GetForegroundWindow();
        let fg_tid = GetWindowThreadProcessId(fg_hwnd, None);
        let our_tid = GetWindowThreadProcessId(self.hwnd, None);
        if fg_tid != 0 && fg_tid != our_tid {
            // Attache NOTRE thread au thread foreground pour que SetForegroundWindow soit autorisé.
            // IdAttach=our_tid → idAttachTo=fg_tid : notre thread partage l'état d'entrée du foreground.
            let _ = AttachThreadInput(our_tid, fg_tid, true);
            SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.edit_hwnd));
            let _ = AttachThreadInput(our_tid, fg_tid, false);
            // Re-donner le focus après détachement pour éviter que l'app précédente le reprenne
            let _ = SetFocus(Some(self.edit_hwnd));
        } else {
            SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.edit_hwnd));
        }
    }

    pub unsafe fn hide(&self) {
        ShowWindow(self.hwnd, SW_HIDE);
    }
}

impl Drop for InputBox {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

// ── Subclass de l'EDIT ─────────────────────────────────────────────────────────

unsafe extern "system" fn edit_subclass_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    if msg == WM_KEYDOWN {
        let vk = wp.0 as u16;
        if vk == VK_RETURN.0 {
            if let Ok(parent) = GetParent(hwnd) {
                let _ = PostMessageW(Some(parent), WM_INPUT_SEND, WPARAM(0), LPARAM(0));
            }
            return LRESULT(0);
        }
        if vk == VK_ESCAPE.0 {
            if let Ok(parent) = GetParent(hwnd) {
                let _ = PostMessageW(Some(parent), WM_INPUT_CANCEL, WPARAM(0), LPARAM(0));
            }
            return LRESULT(0);
        }
    }

    // Note : on ne ferme PAS sur WM_KILLFOCUS — le focus peut être perdu brièvement
    // lors du changement de foreground, ce qui fermerait la boîte avant que l'utilisateur
    // puisse taper. La fermeture n'est déclenchée que par VK_ESCAPE ou WM_LBUTTONDBLCLK.

    let orig = ORIG_EDIT_PROC.load(Ordering::Relaxed);
    if orig != 0 {
        let orig_fn: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
            std::mem::transmute(orig as usize);
        CallWindowProcW(Some(orig_fn), hwnd, msg, wp, lp)
    } else {
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

// ── WndProc de la fenêtre InputBox ────────────────────────────────────────────

/// WndProc enregistrée pour la classe INPUT_CLASS.
pub unsafe extern "system" fn input_wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    match msg {
        WM_INPUT_SEND => {
            // Lire le texte de l'EDIT (seul enfant de la fenêtre)
            let edit = GetWindow(hwnd, GW_CHILD).unwrap_or_default();
            let len = GetWindowTextLengthW(edit);
            if len > 0 {
                let mut buf: Vec<u16> = vec![0u16; (len + 1) as usize];
                GetWindowTextW(edit, &mut buf);
                let text = String::from_utf16_lossy(&buf[..len as usize]).trim().to_owned();
                if !text.is_empty() {
                    let cat_idx = ACTIVE_CAT_IDX.load(Ordering::Relaxed);
                    let msg_hwnd_ptr = MSG_HWND_PTR.load(Ordering::Relaxed);
                    ShowWindow(hwnd, SW_HIDE);
                    // Envoyer le message utilisateur au msg_hwnd principal
                    let payload = Box::new((cat_idx, text));
                    let raw = Box::into_raw(payload) as isize;
                    let _ = PostMessageW(
                        Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                        crate::app::WM_USER_INPUT,
                        WPARAM(cat_idx),
                        LPARAM(raw),
                    );
                    return LRESULT(0);
                }
            }
            // Texte vide → fermer + signaler l'annulation pour reprendre le comportement du chat
            ShowWindow(hwnd, SW_HIDE);
            let cat_idx = ACTIVE_CAT_IDX.load(Ordering::Relaxed);
            let msg_hwnd_ptr = MSG_HWND_PTR.load(Ordering::Relaxed);
            let _ = PostMessageW(
                Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                crate::app::WM_USER_CANCEL_CHAT,
                WPARAM(cat_idx),
                LPARAM(0),
            );
            LRESULT(0)
        }
        WM_INPUT_CANCEL => {
            ShowWindow(hwnd, SW_HIDE);
            let cat_idx = ACTIVE_CAT_IDX.load(Ordering::Relaxed);
            let msg_hwnd_ptr = MSG_HWND_PTR.load(Ordering::Relaxed);
            let _ = PostMessageW(
                Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                crate::app::WM_USER_CANCEL_CHAT,
                WPARAM(cat_idx),
                LPARAM(0),
            );
            LRESULT(0)
        }
        // Fermer quand la fenêtre perd l'activation (clic sur une autre application).
        // Garde 500ms après show() pour éviter la fermeture prématurée lors de l'acquisition
        // du focus (Windows envoie WA_INACTIVE à la fenêtre précédente, qui peut rebondir).
        // WM_ACTIVATE arrive sur le CONTAINER, pas sur l'EDIT enfant.
        // On vérifie stored_ts > 0 pour éviter une fausse annulation si show() n'a jamais
        // été appelé (SHOW_TIMESTAMP_MS vaut 0 au démarrage → elapsed serait très grand).
        WM_ACTIVATE => {
            if wp.0 as u32 & 0xFFFF == WA_INACTIVE {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;
                let stored_ts = SHOW_TIMESTAMP_MS.load(Ordering::Relaxed);
                let elapsed = now_ms - stored_ts;
                if stored_ts > 0 && elapsed >= 500 {
                    ShowWindow(hwnd, SW_HIDE);
                    let cat_idx = ACTIVE_CAT_IDX.load(Ordering::Relaxed);
                    let msg_hwnd_ptr = MSG_HWND_PTR.load(Ordering::Relaxed);
                    if msg_hwnd_ptr != 0 {
                        let _ = PostMessageW(
                            Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                            crate::app::WM_USER_CANCEL_CHAT,
                            WPARAM(cat_idx),
                            LPARAM(0),
                        );
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
