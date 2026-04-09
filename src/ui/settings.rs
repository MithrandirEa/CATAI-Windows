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
    app::{AppState, WM_CONFIG_CHANGED, WM_MODELS_READY, WM_MODELS_UPDATED},
    config::{delete_memory, save_config, CatConfig, CAT_COLOR_DEFS, MAX_CAT_NAME_LEN},
    l10n::L10n,
};

pub const SETTINGS_CLASS: PCWSTR = windows::core::w!("CATAI_Settings");

// ── IDs des contrôles ─────────────────────────────────────────────────────────
const IDC_CAT_LIST:      u32 = 100;
const IDC_ADD_CAT:       u32 = 101;
const IDC_REMOVE_CAT:    u32 = 102;
const IDC_NAME_EDIT:     u32 = 103;
const IDC_COLOR_BASE:    u32 = 110; // 110-115 → 6 couleurs
const IDC_MODEL_EDIT:    u32 = 120;
const IDC_MODELS_LIST:   u32 = 121; // listbox des modèles disponibles
const IDC_FETCH_MODELS:  u32 = 122; // bouton "Récupérer"
const IDC_SCALE_TRACK:   u32 = 130;
const IDC_SCALE_VAL:     u32 = 131;
const IDC_LANG_FR:       u32 = 140;
const IDC_LANG_EN:       u32 = 141;
const IDC_LANG_ES:       u32 = 142;
const IDC_CLEAR_MEM:     u32 = 150; // bouton "Effacer mémoire" du chat sélectionné
const IDC_SAVE:          u32 = 200;
const IDC_CANCEL:        u32 = 201;

// ── Messages Win32 (valeurs numériques directes) ──────────────────────────────
const LB_ADDSTRING:    u32 = 0x0180;
const LB_RESETCONTENT: u32 = 0x0184;
const LB_SETCURSEL:    u32 = 0x0186;
const LB_GETCURSEL:    u32 = 0x0188;
const LB_GETTEXT:      u32 = 0x0189;
const LB_GETTEXTLEN:   u32 = 0x018A;
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
        200, 200, 510, 545,
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
                shared: shared.clone(), msg_hwnd_ptr: msg_hwnd.0 as isize,
                cats, selected: 0, model, scale, lang, updating: false,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(data) as isize);

            // Enregistrer ce HWND dans l'AppState pour les notifications WM_MODELS_UPDATED
            shared.lock().unwrap().settings_hwnd = hwnd;

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

        // Rafraîchir la liste des modèles après récupération async
        WM_MODELS_UPDATED => {
            let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if raw != 0 {
                let d = &*(raw as *const SettingsData);
                let models = d.shared.lock().unwrap().available_models.clone();
                refresh_models_list(hwnd, &models);
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if raw != 0 {
                let d = &*(raw as *mut SettingsData);
                // Effacer le HWND settings dans l'AppState
                d.shared.lock().unwrap().settings_hwnd = HWND::default();
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(raw as *mut SettingsData));
            }
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ── Noms de couleurs localisés ────────────────────────────────────────────────
fn localized_color_names(lang: &str) -> [&'static str; 6] {
    match lang {
        "en" => ["Orange", "Black", "White", "Grey",   "Brown",  "Cream"],
        "es" => ["Naranja","Negro", "Blanco","Gris",   "Marrón", "Crema"],
        _    => ["Orange", "Noir",  "Blanc", "Gris",   "Marron", "Crème"],
    }
}

// ── Création des contrôles ────────────────────────────────────────────────────
unsafe fn create_controls(hwnd: HWND) {
    let h = HINSTANCE(GetModuleHandleW(None).unwrap().0);
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let font_wp = WPARAM(font.0 as usize);

    // Récupérer la langue depuis SettingsData déjà initialisée
    let lang: String = {
        let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        if raw != 0 {
            let d = &*(raw as *const SettingsData);
            d.lang.clone()
        } else {
            "fr".into()
        }
    };
    let lang = lang.as_str();

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
    mk(windows::core::w!("STATIC"),  L10n::s("max_cats", lang), 0, 10, 10, 200, 20, 0);
    mk(windows::core::w!("LISTBOX"), "", LBS_NOTIFY | LBS_HASSTRINGS | WS_VSCROLL.0 | WS_TABSTOP.0 | WS_BORDER_RAW, 10, 33, 200, 95, IDC_CAT_LIST);
    mk(windows::core::w!("BUTTON"),  L10n::s("add_cat", lang),    WS_TABSTOP.0, 220, 33,  130, 28, IDC_ADD_CAT);
    mk(windows::core::w!("BUTTON"),  L10n::s("remove_cat", lang), WS_TABSTOP.0, 220, 68,  130, 28, IDC_REMOVE_CAT);

    // Nom
    mk(windows::core::w!("STATIC"), L10n::s("name", lang), 0, 10, 143, 55, 20, 0);
    mk(windows::core::w!("EDIT"),   "", ES_AUTOHSCROLL | WS_BORDER_RAW | WS_TABSTOP.0, 70, 140, 300, 24, IDC_NAME_EDIT);

    // Couleur — boutons avec noms localisés (texte mis à jour dans update_color_labels)
    mk(windows::core::w!("STATIC"), L10n::s("color_label", lang), 0, 10, 183, 65, 20, 0);
    for i in 0..6usize {
        mk(windows::core::w!("BUTTON"), "", WS_TABSTOP.0, 80 + i as i32 * 67, 180, 65, 26, IDC_COLOR_BASE + i as u32);
    }

    // Modèle Ollama
    mk(windows::core::w!("STATIC"), L10n::s("model", lang), 0, 10, 223, 130, 20, 0);
    mk(windows::core::w!("EDIT"),   "", ES_AUTOHSCROLL | WS_BORDER_RAW | WS_TABSTOP.0, 145, 220, 240, 24, IDC_MODEL_EDIT);
    mk(windows::core::w!("BUTTON"), L10n::s("fetch_models", lang), WS_TABSTOP.0, 390, 220, 90, 24, IDC_FETCH_MODELS);

    // Liste des modèles disponibles
    mk(windows::core::w!("STATIC"), L10n::s("available_models", lang), 0, 10, 253, 200, 20, 0);
    mk(windows::core::w!("LISTBOX"), "", LBS_NOTIFY | LBS_HASSTRINGS | WS_VSCROLL.0 | WS_TABSTOP.0 | WS_BORDER_RAW, 10, 273, 470, 65, IDC_MODELS_LIST);

    // Taille (TrackBar)
    mk(windows::core::w!("STATIC"), L10n::s("size", lang), 0, 10, 350, 60, 20, 0);
    let track = mk(windows::core::w!("msctls_trackbar32"), "", TBS_AUTOTICKS | WS_TABSTOP.0, 75, 345, 280, 30, IDC_SCALE_TRACK);
    // Plage 5-30 → ×0.1 = 0.5×–3.0×
    let range_lparam = ((5u32 & 0xFFFF) | (30u32 << 16)) as isize;
    SendMessageW(track, TBM_SETRANGE, Some(WPARAM(1)), Some(LPARAM(range_lparam)));
    drop(track);
    mk(windows::core::w!("STATIC"), "1.0\u{d7}", 0, 360, 350, 70, 20, IDC_SCALE_VAL);

    // Langue
    mk(windows::core::w!("STATIC"), L10n::s("lang_label", lang), 0, 10, 393, 70, 20, 0);
    mk(windows::core::w!("BUTTON"), "FR", WS_TABSTOP.0, 85,  390, 55, 28, IDC_LANG_FR);
    mk(windows::core::w!("BUTTON"), "EN", WS_TABSTOP.0, 145, 390, 55, 28, IDC_LANG_EN);
    mk(windows::core::w!("BUTTON"), "ES", WS_TABSTOP.0, 205, 390, 55, 28, IDC_LANG_ES);

    // Effacer la mémoire du chat sélectionné
    mk(windows::core::w!("BUTTON"), L10n::s("clear_mem", lang), WS_TABSTOP.0, 300, 390, 175, 28, IDC_CLEAR_MEM);

    // Actions
    mk(windows::core::w!("BUTTON"), L10n::s("save", lang),   WS_TABSTOP.0, 80,  470, 150, 32, IDC_SAVE);
    mk(windows::core::w!("BUTTON"), L10n::s("cancel", lang), WS_TABSTOP.0, 270, 470, 150, 32, IDC_CANCEL);
}

// ── Initialisation des contrôles depuis l'état ────────────────────────────────
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
    update_color_labels(hwnd, d);

    // Peupler la liste des modèles déjà connus
    let models = d.shared.lock().unwrap().available_models.clone();
    refresh_models_list(hwnd, &models);

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
                d.cats[d.selected].name = truncate_name(&get_text(hwnd, IDC_NAME_EDIT));
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

        // Copier le modèle sélectionné dans la listbox vers l'edit
        IDC_MODELS_LIST if notif == LBN_SELCHANGE => {
            let sel = listbox_sel(hwnd, IDC_MODELS_LIST);
            let model_name = get_listbox_item(hwnd, IDC_MODELS_LIST, sel);
            if !model_name.is_empty() {
                set_text(hwnd, IDC_MODEL_EDIT, &model_name);
            }
        }

        // Lancer la récupération async des modèles Ollama
        IDC_FETCH_MODELS => {
            let msg_hwnd_ptr = d.msg_hwnd_ptr;
            let handle = d.shared.lock().unwrap().tokio_handle.clone();
            if let Some(handle) = handle {
                handle.spawn(async move {
                    match crate::ollama::client::list_models(crate::config::OLLAMA_URL).await {
                        Ok(models) => {
                            let payload = Box::into_raw(Box::new(models)) as isize;
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                                    WM_MODELS_READY,
                                    WPARAM(0),
                                    LPARAM(payload),
                                );
                            }
                        }
                        Err(_) => {} // Ollama indisponible, on ignore silencieusement
                    }
                });
            }
        }

        // Effacer la mémoire de conversation du chat sélectionné
        IDC_CLEAR_MEM => {
            if d.selected < d.cats.len() {
                let cat_id = d.cats[d.selected].id.clone();
                delete_memory(&cat_id);
                // Réinitialiser aussi la mémoire en RAM si le chat est actif
                let mut s = d.shared.lock().unwrap();
                if let Some(cat) = s.cats.iter_mut().find(|c| c.id == cat_id) {
                    cat.messages.clear();
                }
            }
        }

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

unsafe fn refresh_models_list(hwnd: HWND, models: &[String]) {
    let lb = GetDlgItem(Some(hwnd), IDC_MODELS_LIST as i32).unwrap_or_default();
    if lb.0.is_null() { return; }
    SendMessageW(lb, LB_RESETCONTENT, Some(WPARAM(0)), Some(LPARAM(0)));
    for m in models {
        let tw: Vec<u16> = m.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(lb, LB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(tw.as_ptr() as isize)));
    }
}

unsafe fn update_color_labels(hwnd: HWND, d: &SettingsData) {
    let cur = d.cats.get(d.selected).map(|c| c.color_id.as_str()).unwrap_or("");
    let color_names = localized_color_names(&d.lang);
    for (i, name) in color_names.iter().enumerate() {
        if let Some(def) = CAT_COLOR_DEFS.get(i) {
            let text = if def.id == cur { format!("[{name}]") } else { name.to_string() };
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

    // Tronquer les noms trop longs avant de sauvegarder
    for cat in &mut d.cats {
        cat.name = truncate_name(&cat.name);
    }

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

/// Tronque un nom de chat à `MAX_CAT_NAME_LEN` caractères Unicode.
fn truncate_name(name: &str) -> String {
    if name.chars().count() <= MAX_CAT_NAME_LEN {
        name.to_string()
    } else {
        name.chars().take(MAX_CAT_NAME_LEN).collect()
    }
}

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

/// Récupère le texte d'un item de listbox à l'index `idx`.
unsafe fn get_listbox_item(hwnd: HWND, id: u32, idx: usize) -> String {
    let lb = GetDlgItem(Some(hwnd), id as i32).unwrap_or_default();
    if lb.0.is_null() { return String::new(); }
    let len = SendMessageW(lb, LB_GETTEXTLEN, Some(WPARAM(idx)), Some(LPARAM(0))).0;
    if len <= 0 { return String::new(); }
    let mut buf = vec![0u16; len as usize + 1];
    SendMessageW(lb, LB_GETTEXT, Some(WPARAM(idx)), Some(LPARAM(buf.as_mut_ptr() as isize)));
    // Trouver le terminateur nul réel pour éviter tout désaccord de longueur
    let text_len = buf.iter().position(|&c| c == 0).unwrap_or(len as usize);
    String::from_utf16_lossy(&buf[..text_len])
}

