// ui/settings.rs — Fenêtre de réglages Win32 complète.
// Noms par chat, couleur, modèle Ollama, échelle (TrackBar), langue.
// Sauvegarde dans %APPDATA%\CATAI\config.json + PostMessage WM_CONFIG_CHANGED.

use std::sync::{Arc, Mutex};

use windows::{
    core::{Result, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{GetStockObject, HBRUSH, DEFAULT_GUI_FONT},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetDlgItem,
                GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
                PostMessageW, RegisterClassExW, SendMessageW, SetWindowLongPtrW,
                SetWindowTextW, ShowWindow, HMENU,
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
                GWLP_USERDATA, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
                WM_COMMAND, WM_CREATE, WM_DESTROY, WM_HSCROLL, WM_SETFONT,
                WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_SYSMENU,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
};

use crate::{
    app::{AppState, WM_CONFIG_CHANGED},
    config::{save_config, CatConfig, CAT_COLOR_DEFS},
};

pub const SETTINGS_CLASS: PCWSTR = windows::core::w!("CATAI_Settings");

// ── IDs des contrôles ─────────────────────────────────────────────────────────
const IDC_CAT_LIST:    u32 = 100;
const IDC_ADD_CAT:     u32 = 101;
const IDC_REMOVE_CAT:  u32 = 102;
const IDC_NAME_EDIT:   u32 = 103;
const IDC_COLOR_BASE:  u32 = 110; // 110-115 → 6 couleurs
const IDC_MODEL_EDIT:  u32 = 120;
const IDC_SCALE_TRACK: u32 = 130;
const IDC_SCALE_VAL:   u32 = 131;
const IDC_LANG_FR:     u32 = 140;
const IDC_LANG_EN:     u32 = 141;
const IDC_LANG_ES:     u32 = 142;
const IDC_SAVE:        u32 = 200;
const IDC_CANCEL:      u32 = 201;

// ── Messages Win32 (valeurs numériques directes) ──────────────────────────────
const LB_ADDSTRING:    u32 = 0x0180;
const LB_RESETCONTENT: u32 = 0x0184;
const LB_SETCURSEL:    u32 = 0x0186;
const LB_GETCURSEL:    u32 = 0x0188;
// TrackBar (WM_USER = 0x0400)
const TBM_GETPOS:   u32 = 0x0400;
const TBM_SETRANGE: u32 = 0x0406;
const TBM_SETPOS:   u32 = 0x0405;
// Notifications
const LBN_SELCHANGE: u32 = 1;
const EN_CHANGE:     u32 = 0x0300;
// Styles
const ES_AUTOHSCROLL: u32 = 0x0080;
const LBS_NOTIFY:     u32 = 0x0001;
const LBS_HASSTRINGS: u32 = 0x0040;
const TBS_AUTOTICKS:  u32 = 0x0001;
const WS_BORDER_RAW:  u32 = 0x00800000;

// ── État interne de la fenêtre ────────────────────────────────────────────────
struct SettingsData {
    shared:       Arc<Mutex<AppState>>,
    msg_hwnd_ptr: isize,
    cats:         Vec<CatConfig>,
    selected:     usize,
    model:        String,
    scale:        f64,
    lang:         String,
    updating:     bool,
}

struct CreateParam {
    shared:   Arc<Mutex<AppState>>,
    msg_hwnd: HWND,
}

// ── Enregistrement de la classe ───────────────────────────────────────────────
pub unsafe fn register_settings_class(hinstance: HINSTANCE) -> Result<()> {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(settings_wnd_proc),
        hInstance: hinstance,
        lpszClassName: SETTINGS_CLASS,
        // COLOR_BTNFACE = 15, +1 = 16 comme brush système
        hbrBackground: HBRUSH(16isize as *mut core::ffi::c_void),
        ..Default::default()
    };
    RegisterClassExW(&wc);
    Ok(())
}

// ── Ouverture ─────────────────────────────────────────────────────────────────
pub unsafe fn open_settings(
    parent: HWND,
    shared: Arc<Mutex<AppState>>,
    msg_hwnd: HWND,
) -> Result<()> {
    let hinstance = HINSTANCE(GetModuleHandleW(None)?.0);
    let param = Box::new(CreateParam { shared, msg_hwnd });
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        SETTINGS_CLASS,
        windows::core::w!("CATAI \u{2014} R\u{e9}glages"),
        WINDOW_STYLE(WS_CAPTION.0 | WS_SYSMENU.0),
        200, 200, 490, 460,
        Some(parent), None, Some(hinstance),
        Some(Box::into_raw(param) as *const _),
    )?;
    ShowWindow(hwnd, SW_SHOW);
    Ok(())
}

// ── Proc principale ───────────────────────────────────────────────────────────
unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            let param = Box::from_raw(cs.lpCreateParams as *mut CreateParam);
            let shared = param.shared.clone();
            let msg_hwnd = param.msg_hwnd;
            drop(param);

            let (cats, model, scale, lang) = {
                let s = shared.lock().unwrap();
                (s.config.cats.clone(), s.config.model.clone(), s.config.scale, s.config.lang.clone())
            };

            let data = Box::new(SettingsData {
                shared, msg_hwnd_ptr: msg_hwnd.0 as isize,
                cats, selected: 0, model, scale, lang, updating: false,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(data) as isize);

            create_controls(hwnd);
            init_controls(hwnd);
            LRESULT(0)
        }

        WM_COMMAND => {
            let id    = (wparam.0 & 0xFFFF) as u32;
            let notif = ((wparam.0 >> 16) & 0xFFFF) as u32;
            handle_command(hwnd, id, notif);
            LRESULT(0)
        }

        WM_HSCROLL => {
            update_scale_label(hwnd);
            LRESULT(0)
        }

        WM_DESTROY => {
            let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if raw != 0 {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(raw as *mut SettingsData));
            }
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ── Création des contrôles ────────────────────────────────────────────────────
unsafe fn create_controls(hwnd: HWND) {
    let h = HINSTANCE(GetModuleHandleW(None).unwrap().0);
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let font_wp = WPARAM(font.0 as usize);

    let mk = |class: PCWSTR, text: &str, style: u32, x: i32, y: i32, w: i32, ht: i32, id: u32| -> HWND {
        let tw: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let ctrl = CreateWindowExW(
            WINDOW_EX_STYLE::default(), class, PCWSTR(tw.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
            x, y, w, ht, Some(hwnd),
            Some(HMENU(id as isize as *mut core::ffi::c_void)),
            Some(h), None,
        ).unwrap_or_default();
        SendMessageW(ctrl, WM_SETFONT, Some(font_wp), Some(LPARAM(1)));
        ctrl
    };

    // Section « Chats »
    mk(windows::core::w!("STATIC"),  "Chats (max 6) :", 0, 10, 10, 150, 20, 0);
    mk(windows::core::w!("LISTBOX"), "", LBS_NOTIFY | LBS_HASSTRINGS | WS_VSCROLL.0 | WS_TABSTOP.0 | WS_BORDER_RAW, 10, 33, 200, 95, IDC_CAT_LIST);
    mk(windows::core::w!("BUTTON"),  "+ Ajouter",    WS_TABSTOP.0, 220, 33,  120, 28, IDC_ADD_CAT);
    mk(windows::core::w!("BUTTON"),  "- Supprimer",  WS_TABSTOP.0, 220, 68,  120, 28, IDC_REMOVE_CAT);

    // Nom
    mk(windows::core::w!("STATIC"), "Nom :", 0, 10, 143, 55, 20, 0);
    mk(windows::core::w!("EDIT"),   "", ES_AUTOHSCROLL | WS_BORDER_RAW | WS_TABSTOP.0, 70, 140, 300, 24, IDC_NAME_EDIT);

    // Couleur
    mk(windows::core::w!("STATIC"), "Couleur :", 0, 10, 183, 65, 20, 0);
    let color_labels = ["Org", "Noi", "Bla", "Gri", "Mar", "Crm"];
    for (i, lbl) in color_labels.iter().enumerate() {
        mk(windows::core::w!("BUTTON"), lbl, WS_TABSTOP.0, 80 + i as i32 * 63, 180, 58, 26, IDC_COLOR_BASE + i as u32);
    }

    // Modèle Ollama
    mk(windows::core::w!("STATIC"), "Mod\u{e8}le Ollama :", 0, 10, 223, 115, 20, 0);
    mk(windows::core::w!("EDIT"),   "", ES_AUTOHSCROLL | WS_BORDER_RAW | WS_TABSTOP.0, 130, 220, 320, 24, IDC_MODEL_EDIT);

    // Taille (TrackBar)
    mk(windows::core::w!("STATIC"), "Taille :", 0, 10, 263, 60, 20, 0);
    let track = mk(windows::core::w!("msctls_trackbar32"), "", TBS_AUTOTICKS | WS_TABSTOP.0, 75, 258, 280, 30, IDC_SCALE_TRACK);
    // Plage 5-30 → ×0.1 = 0.5×–3.0×
    let range_lparam = ((5u32 & 0xFFFF) | (30u32 << 16)) as isize;
    SendMessageW(track, TBM_SETRANGE, Some(WPARAM(1)), Some(LPARAM(range_lparam)));
    drop(track);
    mk(windows::core::w!("STATIC"), "1.0\u{d7}", 0, 360, 263, 70, 20, IDC_SCALE_VAL);

    // Langue
    mk(windows::core::w!("STATIC"), "Langue :", 0, 10, 308, 60, 20, 0);
    mk(windows::core::w!("BUTTON"), "FR", WS_TABSTOP.0, 75,  305, 55, 28, IDC_LANG_FR);
    mk(windows::core::w!("BUTTON"), "EN", WS_TABSTOP.0, 135, 305, 55, 28, IDC_LANG_EN);
    mk(windows::core::w!("BUTTON"), "ES", WS_TABSTOP.0, 195, 305, 55, 28, IDC_LANG_ES);

    // Actions
    mk(windows::core::w!("BUTTON"), "Enregistrer", WS_TABSTOP.0, 80,  385, 130, 32, IDC_SAVE);
    mk(windows::core::w!("BUTTON"), "Annuler",     WS_TABSTOP.0, 270, 385, 130, 32, IDC_CANCEL);
}

// ── Initialisaiton des contrôles depuis l'état ────────────────────────────────
unsafe fn init_controls(hwnd: HWND) {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if raw == 0 { return; }
    let d = &mut *(raw as *mut SettingsData);
    d.updating = true;

    refresh_cat_list(hwnd, d);
    set_text(hwnd, IDC_MODEL_EDIT, &d.model.clone());

    let pos = (d.scale * 10.0).round() as isize;
    let track = GetDlgItem(Some(hwnd), IDC_SCALE_TRACK as i32).unwrap_or_default();
    if !track.0.is_null() {
        SendMessageW(track, TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(pos)));
    }
    update_scale_label(hwnd);
    update_lang_labels(hwnd, d);

    d.updating = false;
}

// ── Gestion WM_COMMAND ────────────────────────────────────────────────────────
unsafe fn handle_command(hwnd: HWND, id: u32, notif: u32) {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if raw == 0 { return; }
    let d = &mut *(raw as *mut SettingsData);

    match id {
        IDC_CAT_LIST if notif == LBN_SELCHANGE => {
            let sel = listbox_sel(hwnd, IDC_CAT_LIST);
            if sel < d.cats.len() {
                d.selected = sel;
                d.updating = true;
                set_text(hwnd, IDC_NAME_EDIT, &d.cats[sel].name.clone());
                update_color_labels(hwnd, d);
                d.updating = false;
            }
        }

        IDC_NAME_EDIT if notif == EN_CHANGE && !d.updating => {
            if d.selected < d.cats.len() {
                d.cats[d.selected].name = get_text(hwnd, IDC_NAME_EDIT);
                d.updating = true;
                refresh_cat_list(hwnd, d);
                d.updating = false;
            }
        }

        IDC_ADD_CAT => {
            if d.cats.len() < 6 {
                let color = &CAT_COLOR_DEFS[d.cats.len() % CAT_COLOR_DEFS.len()];
                d.cats.push(CatConfig {
                    id: uuid::Uuid::new_v4().to_string(),
                    color_id: color.id.to_string(),
                    name: color.default_names[0].to_string(),
                });
                d.selected = d.cats.len() - 1;
                d.updating = true;
                refresh_cat_list(hwnd, d);
                set_text(hwnd, IDC_NAME_EDIT, &d.cats[d.selected].name.clone());
                update_color_labels(hwnd, d);
                d.updating = false;
            }
        }

        IDC_REMOVE_CAT => {
            if d.cats.len() > 1 {
                d.cats.remove(d.selected);
                if d.selected >= d.cats.len() { d.selected = d.cats.len() - 1; }
                d.updating = true;
                refresh_cat_list(hwnd, d);
                set_text(hwnd, IDC_NAME_EDIT, &d.cats[d.selected].name.clone());
                update_color_labels(hwnd, d);
                d.updating = false;
            }
        }

        id if id >= IDC_COLOR_BASE && id < IDC_COLOR_BASE + 6 => {
            let ci = (id - IDC_COLOR_BASE) as usize;
            if ci < CAT_COLOR_DEFS.len() && d.selected < d.cats.len() {
                d.cats[d.selected].color_id = CAT_COLOR_DEFS[ci].id.to_string();
                update_color_labels(hwnd, d);
            }
        }

        IDC_LANG_FR => { d.lang = "fr".into(); update_lang_labels(hwnd, d); }
        IDC_LANG_EN => { d.lang = "en".into(); update_lang_labels(hwnd, d); }
        IDC_LANG_ES => { d.lang = "es".into(); update_lang_labels(hwnd, d); }

        IDC_SAVE   => save_and_close(hwnd),
        IDC_CANCEL => { let _ = DestroyWindow(hwnd); }
        _ => {}
    }
}

// ── Helpers UI ────────────────────────────────────────────────────────────────
unsafe fn refresh_cat_list(hwnd: HWND, d: &SettingsData) {
    let lb = GetDlgItem(Some(hwnd), IDC_CAT_LIST as i32).unwrap_or_default();
    if lb.0.is_null() { return; }
    SendMessageW(lb, LB_RESETCONTENT, Some(WPARAM(0)), Some(LPARAM(0)));
    for cat in &d.cats {
        let tw: Vec<u16> = cat.name.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(lb, LB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(tw.as_ptr() as isize)));
    }
    SendMessageW(lb, LB_SETCURSEL, Some(WPARAM(d.selected)), Some(LPARAM(0)));
}

unsafe fn update_color_labels(hwnd: HWND, d: &SettingsData) {
    let cur = d.cats.get(d.selected).map(|c| c.color_id.as_str()).unwrap_or("");
    let labels = ["Org", "Noi", "Bla", "Gri", "Mar", "Crm"];
    for (i, lbl) in labels.iter().enumerate() {
        if let Some(def) = CAT_COLOR_DEFS.get(i) {
            let text = if def.id == cur { format!("[{lbl}]") } else { lbl.to_string() };
            set_text(hwnd, IDC_COLOR_BASE + i as u32, &text);
        }
    }
}

unsafe fn update_lang_labels(hwnd: HWND, d: &SettingsData) {
    for (code, id, lbl) in [("fr", IDC_LANG_FR, "FR"), ("en", IDC_LANG_EN, "EN"), ("es", IDC_LANG_ES, "ES")] {
        let text = if d.lang == code { format!("[{lbl}]") } else { lbl.to_string() };
        set_text(hwnd, id, &text);
    }
}

unsafe fn update_scale_label(hwnd: HWND) {
    let track = GetDlgItem(Some(hwnd), IDC_SCALE_TRACK as i32).unwrap_or_default();
    if track.0.is_null() { return; }
    let pos = SendMessageW(track, TBM_GETPOS, Some(WPARAM(0)), Some(LPARAM(0))).0;
    let scale = pos as f64 * 0.1;
    set_text(hwnd, IDC_SCALE_VAL, &format!("{:.1}\u{d7}", scale));
}

unsafe fn save_and_close(hwnd: HWND) {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if raw == 0 { return; }
    let d = &mut *(raw as *mut SettingsData);

    d.model = get_text(hwnd, IDC_MODEL_EDIT);
    let track = GetDlgItem(Some(hwnd), IDC_SCALE_TRACK as i32).unwrap_or_default();
    if !track.0.is_null() {
        let pos = SendMessageW(track, TBM_GETPOS, Some(WPARAM(0)), Some(LPARAM(0))).0;
        d.scale = (pos as f64 * 0.1).clamp(0.5, 3.0);
    }

    {
        let mut s = d.shared.lock().unwrap();
        s.config.cats  = d.cats.clone();
        s.config.model = d.model.clone();
        s.config.scale = d.scale;
        s.config.lang  = d.lang.clone();
        save_config(&s.config);
    }

    let _ = PostMessageW(
        Some(HWND(d.msg_hwnd_ptr as *mut _)),
        WM_CONFIG_CHANGED,
        WPARAM(0), LPARAM(0),
    );
    let _ = DestroyWindow(hwnd);
}

// ── Utilitaires ───────────────────────────────────────────────────────────────
unsafe fn set_text(hwnd: HWND, id: u32, text: &str) {
    let ctrl = GetDlgItem(Some(hwnd), id as i32).unwrap_or_default();
    if !ctrl.0.is_null() {
        let tw: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        SetWindowTextW(ctrl, PCWSTR(tw.as_ptr()));
    }
}

unsafe fn get_text(hwnd: HWND, id: u32) -> String {
    let ctrl = GetDlgItem(Some(hwnd), id as i32).unwrap_or_default();
    if ctrl.0.is_null() { return String::new(); }
    let len = GetWindowTextLengthW(ctrl);
    if len <= 0 { return String::new(); }
    let mut buf = vec![0u16; len as usize + 1];
    GetWindowTextW(ctrl, &mut buf);
    String::from_utf16_lossy(&buf[..len as usize])
}

unsafe fn listbox_sel(hwnd: HWND, id: u32) -> usize {
    let lb = GetDlgItem(Some(hwnd), id as i32).unwrap_or_default();
    if lb.0.is_null() { return 0; }
    let sel = SendMessageW(lb, LB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0;
    if sel >= 0 { sel as usize } else { 0 }
}

