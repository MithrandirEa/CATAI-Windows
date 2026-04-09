#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// main.rs — Point d'entrée Win32 + runtime tokio.
// Thread UI = boucle GetMessage. Thread tokio = runtime séparé.
// Communication tokio → UI uniquement via PostMessage(WM_APP+N).

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use windows::{
    core::{w, Result, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW,
                GetCursorPos, GetMessageW, PostQuitMessage, RegisterClassExW,
                SetTimer, TranslateMessage,
                CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, MSG, WINDOW_EX_STYLE, WINDOW_STYLE,
                WNDCLASSEXW, PostMessageW, WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_TIMER,
            },
        },
    },
};

/// Pseudo-handle pour une fenêtre message-only (HWND_MESSAGE = -3).
const HWND_MESSAGE: HWND = HWND(-3isize as *mut core::ffi::c_void);

mod app;
mod cat;
mod config;
mod l10n;
mod ollama;
mod system;
mod ui;

use app::{AppState, SharedState, WM_CONFIG_CHANGED, WM_FRAMES_READY, WM_MODELS_READY, WM_MODELS_UPDATED, WM_OLLAMA_DONE, WM_OLLAMA_ERR, WM_OLLAMA_TOKEN, WM_TRAY};
use cat::{animation::AnimationTable, sprite::load_sprite_bgra, CatInstance};
use config::{load_config, color_def, TIMER_BEHAVIOR, TIMER_RENDER, TIMER_TASKBAR, WALK_SPEED};
use system::taskbar::get_taskbar_info;
use ui::{
    layered::{create_cat_window, update_layered, CAT_CLASS},
    tray::{add_tray_icon, remove_tray_icon, show_context_menu, MENU_QUIT, MENU_SETTINGS},
    settings::open_settings,
};

// ── Constantes locales ────────────────────────────────────────────────────────

const MSG_CLASS: PCWSTR = w!("CATAI_Msg");
const MSG_WND_TITLE: PCWSTR = w!("CATAI_MsgWindow");

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    // 1. DPI awareness — avant toute création de fenêtre
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // 2. Charger la config
    let cfg = load_config();

    // 3. Localiser les assets (exe_dir/cute_orange_cat/ ou CWD/cute_orange_cat/)
    let assets_dir = {
        let exe_relative = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("cute_orange_cat")));
        match exe_relative {
            Some(ref p) if p.exists() => p.clone(),
            _ => PathBuf::from("cute_orange_cat"),
        }
    };

    // 4. Parser metadata.json
    let anim_table = AnimationTable::load(&assets_dir)
        .unwrap_or_else(|e| panic!("Impossible de charger les animations: {e}"));

    // 5. État partagé
    let state = Arc::new(Mutex::new(AppState::new(cfg)));
    {
        let mut s = state.lock().unwrap();
        s.anim_table = Some(anim_table);
        s.taskbar = get_taskbar_info();
    }

    // 6. Démarrer le runtime tokio sur un thread séparé
    let tokio_rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _rt_guard = tokio_rt.enter();
    {
        let mut s = state.lock().unwrap();
        s.tokio_handle = Some(tokio_rt.handle().clone());
    }

    unsafe {
        let hinstance = HINSTANCE(GetModuleHandleW(None)?.0);

        // 7. Enregistrer les classes de fenêtres
        register_classes(hinstance)?;

        // 8. Fenêtre message-only (pour recevoir les PostMessage depuis tokio)
        let msg_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            MSG_CLASS,
            MSG_WND_TITLE,
            WINDOW_STYLE::default(),
            0, 0, 0, 0,
            Some(HWND_MESSAGE), None, Some(hinstance), None,
        )?;

        {
            let mut s = state.lock().unwrap();
            s.msg_hwnd = msg_hwnd;
        }

        // 9. Stocker le SharedState dans le user-data de la fenêtre message
        let state_ptr = Arc::into_raw(Arc::clone(&state)) as isize;
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
            msg_hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            state_ptr,
        );

        // 10. Créer les fenêtres chat + icône tray
        init_cats(&state, hinstance)?;
        add_tray_icon(msg_hwnd)?;

        // 11. Démarrer les timers sur la fenêtre message
        SetTimer(Some(msg_hwnd), TIMER_RENDER, 100, None);
        SetTimer(Some(msg_hwnd), TIMER_BEHAVIOR, 1000, None);
        SetTimer(Some(msg_hwnd), TIMER_TASKBAR, 5000, None);

        // 12. Boucle de messages Win32
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 13. Nettoyage
        remove_tray_icon(msg_hwnd);
        // Libérer l'Arc récupéré depuis le user-data
        let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
            msg_hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
        );
        if raw != 0 {
            drop(Arc::from_raw(raw as *const Mutex<AppState>));
        }
    }

    Ok(())
}

// ── Enregistrement des classes ────────────────────────────────────────────────

/// Wrapper ABI "system" autour de DefWindowProcW pour l'utiliser dans lpfnWndProc.
unsafe extern "system" fn def_wnd_proc_wrapper(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe fn register_classes(hinstance: HINSTANCE) -> Result<()> {
    // Classe fenêtre message-only
    let mut wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(msg_wnd_proc),
        hInstance: hinstance,
        lpszClassName: MSG_CLASS,
        ..Default::default()
    };
    RegisterClassExW(&wc);

    // Classe fenêtre chat
    wc.lpfnWndProc = Some(cat_wnd_proc);
    wc.lpszClassName = CAT_CLASS;
    wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
    RegisterClassExW(&wc);

    // Classe fenêtre bulle
    wc.lpfnWndProc = Some(def_wnd_proc_wrapper);
    wc.lpszClassName = ui::chat_bubble::BUBBLE_CLASS;
    RegisterClassExW(&wc);

    // Classe fenêtre réglages
    ui::settings::register_settings_class(hinstance)?;

    Ok(())
}

// ── Création des chats ────────────────────────────────────────────────────────

unsafe fn init_cats(state: &SharedState, hinstance: HINSTANCE) -> Result<()> {
    let mut s = state.lock().unwrap();

    let taskbar = s.taskbar.clone();
    let scale = s.config.scale as f32;
    let cat_px = (68.0 * scale) as i32;

    let cats_cfg = s.config.cats.clone();

    for (i, cat_cfg) in cats_cfg.iter().enumerate() {
        // Position initiale sur la barre des tâches
        let (x, y) = if let Some(tb) = &taskbar {
            let (xmin, xmax) = tb.walk_range_x(cat_px);
            let start_x = xmin + ((xmax - xmin) / (cats_cfg.len() as i32 + 1)) * (i as i32 + 1);
            (start_x, tb.cat_y_for_bottom(cat_px))
        } else {
            (100 + i as i32 * (cat_px + 10), 800)
        };

        // Charger le sprite initial (direction South, idle = rotation)
        let color = color_def(&cat_cfg.color_id).unwrap_or(&config::CAT_COLOR_DEFS[0]);
        let anim = s.anim_table.as_ref().unwrap();
        let sprite_path = anim
            .rotation(cat::state::Direction::South)
            .unwrap_or_else(|| std::path::Path::new(""));

        let (bgra, sw, sh) = if sprite_path.exists() {
            load_sprite_bgra(sprite_path, color, scale).unwrap_or_else(|_| {
                let w = cat_px as u32;
                let h = cat_px as u32;
                (vec![0u8; (w * h * 4) as usize], w, h)
            })
        } else {
            let w = cat_px as u32;
            (vec![128u8; (w * w * 4) as usize], w, w)
        };

        let hwnd = create_cat_window(x, y, sw as i32, sh as i32)?;
        update_layered(hwnd, &bgra, sw, sh, x, y)?;

        // Stocker le SharedState dans le GWLP_USERDATA de la fenêtre chat
        let state_ptr = Arc::into_raw(Arc::clone(state)) as isize;
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            state_ptr,
        );

        let mut instance = CatInstance::new(cat_cfg, hwnd, x as f32, y as f32);
        // Cache du frame initial + état pour éviter un rebuild inutile au premier tick
        instance.cached_frames = vec![(bgra, sw, sh)];
        instance.cached_state = Some(cat::state::CatState::Idle);
        instance.cached_dir = Some(cat::state::Direction::South);
        // Créer la bulle de dialogue
        instance.bubble = ui::chat_bubble::ChatBubble::new().ok();

        s.cats.push(instance);
    }

    Ok(())
}

/// Détruit toutes les fenêtres chat existantes et recrée depuis la config courante.
unsafe fn reinit_cats(state: &SharedState, hinstance: HINSTANCE) {
    {
        let mut s = state.lock().unwrap();
        // Détruire les HWNDs + déposer les bulles (impl Drop appelle DestroyWindow)
        for cat in s.cats.drain(..) {
            drop(cat.bubble);
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(cat.hwnd);
        }
    }
    // Réinitialiser avec la nouvelle config
    let _ = init_cats(state, hinstance);
}

// ── Window procedures ─────────────────────────────────────────────────────────

/// Proc de la fenêtre message-only — gère les timers et les WM_APP.
unsafe extern "system" fn msg_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER => {
            let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if raw == 0 {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            // Cloner l'Arc sans en prendre possession
            let state = Arc::from_raw(raw as *const Mutex<AppState>);
            let state2 = Arc::clone(&state);
            std::mem::forget(state);

            match wparam.0 {
                TIMER_RENDER => on_timer_render(&state2),
                TIMER_BEHAVIOR => on_timer_behavior(&state2),
                TIMER_TASKBAR => on_timer_taskbar(&state2),
                _ => {}
            }
            LRESULT(0)
        }

        WM_TRAY => {
            let notif = (lparam.0 & 0xFFFF) as u32;
            if notif == windows::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP
                || notif == windows::Win32::UI::WindowsAndMessaging::WM_CONTEXTMENU
            {
                let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                );
                let lang = if raw != 0 {
                    let state = Arc::from_raw(raw as *const Mutex<AppState>);
                    let l = state.lock().unwrap().config.lang.clone();
                    std::mem::forget(state);
                    l
                } else {
                    "fr".into()
                };
                show_context_menu(hwnd, &lang);
            }
            LRESULT(0)
        }

        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as u32;
            match id {
                MENU_QUIT => PostQuitMessage(0),
                MENU_SETTINGS => {
                    let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                        hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                    );
                    if raw != 0 {
                        let state = Arc::from_raw(raw as *const Mutex<AppState>);
                        let state2 = Arc::clone(&state);
                        let msg_hwnd: HWND = state.lock().unwrap().msg_hwnd;
                        std::mem::forget(state);
                        let _ = open_settings(hwnd, state2, msg_hwnd);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }

        // Tokens Ollama reçus depuis tokio via PostMessage
        WM_OLLAMA_TOKEN => {
            // LPARAM = Box<(usize, String)> transformé en raw pointer
            if lparam.0 == 0 {
                return LRESULT(0);
            }
            let payload = Box::from_raw(lparam.0 as *mut (usize, String));
            let (idx, token) = *payload;
            let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if raw != 0 {
                let state = Arc::from_raw(raw as *const Mutex<AppState>);
                {
                    let mut s = state.lock().unwrap();
                    if let Some(cat) = s.cats.get_mut(idx) {
                        let cx = cat.x as i32;
                        let cy = cat.y as i32;
                        let frame = cat.current_frame().map(|(_, w, _)| *w as i32).unwrap_or(68);
                        if let Some(bubble) = &mut cat.bubble {
                            let _ = bubble.append(&token, cx, cy, frame);
                        }
                        // Accumuler le token dans l'historique
                        let last_is_assistant = cat
                            .messages
                            .last()
                            .and_then(|m| m.get("role"))
                            .and_then(|r| r.as_str())
                            == Some("assistant");
                        if last_is_assistant {
                            if let Some(last) = cat.messages.last_mut() {
                                let prev = last["content"].as_str().unwrap_or("").to_string();
                                *last = serde_json::json!({"role": "assistant", "content": prev + &token});
                            }
                        } else {
                            cat.messages.push(serde_json::json!({"role": "assistant", "content": token}));
                        }
                    }
                }
                std::mem::forget(state);
            }
            LRESULT(0)
        }

        WM_CONFIG_CHANGED => {
            // Réinitialiser tous les chats avec la nouvelle config
            let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if raw != 0 {
                let state = Arc::from_raw(raw as *const Mutex<AppState>);
                let state2 = Arc::clone(&state);
                std::mem::forget(state);
                let hinstance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
                reinit_cats(&state2, hinstance);
            }
            LRESULT(0)
        }

        WM_OLLAMA_DONE => LRESULT(0),
        WM_OLLAMA_ERR => {
            // Libérer le Box<String> de l'erreur
            if lparam.0 != 0 {
                drop(Box::from_raw(lparam.0 as *mut String));
            }
            LRESULT(0)
        }

        // Modèles Ollama récupérés par tokio → stocker + notifier la fenêtre settings
        WM_MODELS_READY => {
            if lparam.0 != 0 {
                let models = Box::from_raw(lparam.0 as *mut Vec<String>);
                let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                );
                if raw != 0 {
                    let state = Arc::from_raw(raw as *const Mutex<AppState>);
                    let settings_hwnd = {
                        let mut s = state.lock().unwrap();
                        s.available_models = *models;
                        s.settings_hwnd
                    };
                    std::mem::forget(state);
                    // Notifier la fenêtre settings si elle est ouverte
                    if !settings_hwnd.0.is_null() {
                        let _ = PostMessageW(
                            Some(settings_hwnd),
                            WM_MODELS_UPDATED,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            }
            LRESULT(0)
        }

        // Frames PNG chargées en arrière-plan via tokio, prêtes à être appliquées
        WM_FRAMES_READY => {
            use cat::state::{CatState, Direction};
            if lparam.0 != 0 {
                let payload = Box::from_raw(
                    lparam.0 as *mut (usize, CatState, Direction, Vec<(Vec<u8>, u32, u32)>),
                );
                let (idx, new_state, new_dir, frames) = *payload;
                let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                );
                if raw != 0 {
                    let state = Arc::from_raw(raw as *const Mutex<AppState>);
                    {
                        let mut s = state.lock().unwrap();
                        if let Some(cat) = s.cats.get_mut(idx) {
                            cat.frames_loading = false;
                            // Appliquer seulement si le chat est encore dans le même état
                            if cat.state == new_state && !frames.is_empty() {
                                let effective_dir = match new_state {
                                    CatState::Idle | CatState::Sleeping => Direction::South,
                                    _ => new_dir,
                                };
                                cat.cached_frames = frames;
                                cat.frame_idx = 0;
                                cat.cached_state = Some(new_state);
                                cat.cached_dir = Some(effective_dir);
                            }
                        }
                    }
                    std::mem::forget(state);
                }
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Helper : récupère un Arc<Mutex<AppState>> depuis le GWLP_USERDATA sans en prendre possession.
macro_rules! borrow_state {
    ($hwnd:expr) => {{
        let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
            $hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
        );
        if raw == 0 {
            None
        } else {
            let arc = Arc::from_raw(raw as *const Mutex<AppState>);
            let clone = Arc::clone(&arc);
            std::mem::forget(arc);
            Some(clone)
        }
    }};
}

/// Proc des fenêtres chat — gère les clics souris.
unsafe extern "system" fn cat_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // ── Début du drag ────────────────────────────────────────────────────
        WM_LBUTTONDOWN => {
            if let Some(state) = borrow_state!(hwnd) {
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);
                let mut s = state.lock().unwrap();
                if let Some(cat) = s.cats.iter_mut().find(|c| c.hwnd == hwnd) {
                    cat.drag_offset_x = cursor.x - cat.x as i32;
                    cat.drag_offset_y = cursor.y - cat.y as i32;
                    cat.is_dragging = true;
                    cat.state = cat::state::CatState::Idle;
                }
            }
            SetCapture(hwnd);
            LRESULT(0)
        }

        // ── Déplacement pendant le drag ───────────────────────────────────
        WM_MOUSEMOVE => {
            if let Some(state) = borrow_state!(hwnd) {
                // Extraire données sous verrou, puis libérer avant d'appeler update_layered
                let frame_data = {
                    let mut s = state.lock().unwrap();
                    if let Some(cat) = s.cats.iter_mut().find(|c| c.hwnd == hwnd) {
                        if cat.is_dragging {
                            let mut cursor = POINT::default();
                            let _ = GetCursorPos(&mut cursor);
                            let new_x = cursor.x - cat.drag_offset_x;
                            let new_y = cursor.y - cat.drag_offset_y;
                            cat.x = new_x as f32;
                            cat.y = new_y as f32;
                            cat.current_frame().map(|(bgra, w, h)| (bgra.clone(), *w, *h, new_x, new_y))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some((bgra, w, h, new_x, new_y)) = frame_data {
                    let _ = update_layered(hwnd, &bgra, w, h, new_x, new_y);
                }
            }
            LRESULT(0)
        }

        // ── Fin du drag — snap sur la barre des tâches ───────────────────
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            if let Some(state) = borrow_state!(hwnd) {
                let frame_data = {
                    let mut s = state.lock().unwrap();
                    let scale = s.config.scale as f32;
                    let cat_px = (68.0 * scale) as i32;
                    let taskbar = s.taskbar.clone();
                    if let Some(cat) = s.cats.iter_mut().find(|c| c.hwnd == hwnd) {
                        cat.is_dragging = false;
                        // Snap sur la barre des tâches
                        if let Some(tb) = &taskbar {
                            let snap_y = tb.cat_y_for_bottom(cat_px);
                            let (xmin, xmax) = tb.walk_range_x(cat_px);
                            cat.y = snap_y as f32;
                            cat.x = (cat.x as i32).clamp(xmin, xmax) as f32;
                        }
                        let new_x = cat.x as i32;
                        let new_y = cat.y as i32;
                        cat.current_frame().map(|(bgra, w, h)| (bgra.clone(), *w, *h, new_x, new_y))
                    } else {
                        None
                    }
                };
                if let Some((bgra, w, h, new_x, new_y)) = frame_data {
                    let _ = update_layered(hwnd, &bgra, w, h, new_x, new_y);
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONDBLCLK => {
            if let Some(state) = borrow_state!(hwnd) {
                let task_data = {
                    let mut s = state.lock().unwrap();
                    let lang = s.config.lang.clone();
                    let model = s.config.model.clone();
                    let msg_hwnd = s.msg_hwnd;
                    let tokio_handle = s.tokio_handle.clone();
                    if let Some((idx, cat)) = s.cats.iter_mut().enumerate().find(|(_, c)| c.hwnd == hwnd) {
                        let cx = cat.x as i32;
                        let cy = cat.y as i32;
                        let sz = cat.current_frame().map(|(_, w, _)| *w as i32).unwrap_or(68);
                        // Afficher le miaou immédiatement
                        if let Some(b) = &mut cat.bubble {
                            b.hide_and_clear();
                            let greeting = l10n::L10n::random_meow(&lang);
                            let _ = b.append(greeting, cx, cy, sz);
                        }
                        // Construire le system prompt si première conversation
                        if cat.messages.is_empty() {
                            let color = config::color_def(&cat.color_id)
                                .unwrap_or(&config::CAT_COLOR_DEFS[0]);
                            let sp = color.prompt(&cat.name, &lang);
                            cat.messages.push(serde_json::json!({"role": "system", "content": sp}));
                        }
                        let hi = l10n::L10n::s("hi", &lang).to_string();
                        cat.messages.push(serde_json::json!({"role": "user", "content": hi}));
                        // Garder max MEM_MAX messages (+ system)
                        while cat.messages.len() > config::MEM_MAX * 2 {
                            cat.messages.remove(1);
                        }
                        let messages = cat.messages.clone();
                        tokio_handle.map(|h| (idx, messages, model, msg_hwnd.0 as isize, h))
                    } else {
                        None
                    }
                };
                if let Some((idx, messages, model, msg_hwnd_ptr, handle)) = task_data {
                    handle.spawn(async move {
                        let (tx, mut rx) =
                            tokio::sync::mpsc::channel::<ollama::client::OllamaMsg>(64);
                        // Tâche consommatrice : PostMessage pour chaque token
                        let consumer = tokio::spawn(async move {
                            while let Some(msg) = rx.recv().await {
                                // HWND reconstruit en temporaire (pas vivant à travers await)
                                match msg {
                                    ollama::client::OllamaMsg::Token(t) => {
                                        let payload =
                                            Box::into_raw(Box::new((idx, t))) as isize;
                                        unsafe {
                                            let _ = PostMessageW(
                                                Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                                                WM_OLLAMA_TOKEN,
                                                WPARAM(idx),
                                                LPARAM(payload),
                                            );
                                        }
                                    }
                                    ollama::client::OllamaMsg::Done => {
                                        unsafe {
                                            let _ = PostMessageW(
                                                Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                                                WM_OLLAMA_DONE,
                                                WPARAM(idx),
                                                LPARAM(0),
                                            );
                                        }
                                        break;
                                    }
                                    ollama::client::OllamaMsg::Error(e) => {
                                        let s_ptr =
                                            Box::into_raw(Box::new(e)) as isize;
                                        unsafe {
                                            let _ = PostMessageW(
                                                Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                                                WM_OLLAMA_ERR,
                                                WPARAM(idx),
                                                LPARAM(s_ptr),
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                        });
                        // Producteur : stream les tokens (drop tx à la fin → ferme rx)
                        ollama::client::stream_chat(
                            config::OLLAMA_URL,
                            &model,
                            messages,
                            tx,
                        )
                        .await;
                        let _ = consumer.await;
                    });
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // Libérer l'Arc si présent
            let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if raw != 0 {
                drop(Arc::from_raw(raw as *const Mutex<AppState>));
                windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                    0,
                );
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ── Callbacks timers ──────────────────────────────────────────────────────────

unsafe fn on_timer_render(state: &SharedState) {
    use cat::state::CatState;
    let mut s = state.lock().unwrap();
    let scale = s.config.scale as f32;
    let cat_px = (68.0 * scale) as i32;
    let taskbar = s.taskbar.clone();

    for i in 0..s.cats.len() {
        // Avancer la position si le chat marche (10 FPS, 4 px/frame)
        if s.cats[i].state == CatState::Walking && !s.cats[i].is_dragging {
            let speed = WALK_SPEED * scale;
            let dir = s.cats[i].dir;
            if let Some(tb) = &taskbar {
                let (xmin, xmax) = tb.walk_range_x(cat_px);
                use cat::state::Direction;
                if dir == Direction::East {
                    s.cats[i].x += speed;
                    if s.cats[i].x as i32 >= xmax {
                        s.cats[i].x = xmax as f32;
                        s.cats[i].dir = Direction::West; // repartira à gauche
                        s.cats[i].state = CatState::Idle;
                    }
                } else {
                    s.cats[i].x -= speed;
                    if s.cats[i].x as i32 <= xmin {
                        s.cats[i].x = xmin as f32;
                        s.cats[i].dir = Direction::East; // repartira à droite
                        s.cats[i].state = CatState::Idle;
                    }
                }
            }
        }

        // Reconstruire le cache si l'état ou la direction a changé
        let needs_rebuild = {
            use cat::state::{CatState, Direction};
            let cat = &s.cats[i];
            // Pour Idle/Sleeping, le sprite est toujours South → comparer avec South
            let effective_dir = match cat.state {
                CatState::Idle | CatState::Sleeping => Direction::South,
                _ => cat.dir,
            };
            cat.cached_state != Some(cat.state) || cat.cached_dir != Some(effective_dir)
        };
        if needs_rebuild && !s.cats[i].frames_loading {
            s.cats[i].frames_loading = true;
            schedule_rebuild_frames(&s, i, scale);
        }

        let cat = &mut s.cats[i];
        let moved = cat.state == CatState::Walking;
        if cat.tick_frame() || moved {
            if let Some((bgra, w, h)) = cat.current_frame() {
                let (bgra, w, h) = (bgra.clone(), *w, *h);
                let _ = update_layered(cat.hwnd, &bgra, w, h, cat.x as i32, cat.y as i32);
            }
        }
    }
}

unsafe fn on_timer_behavior(state: &SharedState) {
    use cat::state::{CatState, Direction};

    let mut s = state.lock().unwrap();

    for cat in &mut s.cats {
        if cat.is_dragging {
            continue;
        }
        match cat.state {
            CatState::Idle => {
                // Décision aléatoire simple sans rand crate pour l'instant
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                // ~30% de chance de commencer à marcher
                if t % 10 < 3 {
                    // Utiliser la direction mémorisée ; South = premier démarrage → East
                    if cat.dir == Direction::South {
                        cat.dir = Direction::East;
                    }
                    cat.state = CatState::Walking;
                }
            }
            _ => {} // Mouvement géré dans on_timer_render ; one-shot dans tick_frame
        }
    }
}

unsafe fn on_timer_taskbar(state: &SharedState) {
    let mut s = state.lock().unwrap();
    s.taskbar = get_taskbar_info();
}

/// Lance un chargement asynchrone des frames pour un chat.
/// Les PNGs sont décodés dans le thread-pool tokio ; le résultat est PostMessageé
/// via WM_FRAMES_READY → aucun blocage du thread UI.
fn schedule_rebuild_frames(s: &AppState, idx: usize, scale: f32) {
    use cat::state::Direction;
    let handle = match s.tokio_handle.as_ref() {
        Some(h) => h.clone(),
        None => return,
    };
    let msg_hwnd_ptr = s.msg_hwnd.0 as isize;
    let cat = &s.cats[idx];
    let cat_state = cat.state;
    let cat_dir = cat.dir;
    let color: &'static config::CatColorDef =
        config::color_def(&cat.color_id).unwrap_or(&config::CAT_COLOR_DEFS[0]);

    // Collecter les chemins de fichier AVANT de lâcher le verrou
    let paths: Vec<std::path::PathBuf> = match s.anim_table.as_ref() {
        Some(anim) => {
            if cat_state.anim_key().is_some() {
                anim.frames(cat_state, cat_dir).to_vec()
            } else {
                anim.rotation(Direction::South)
                    .map(|p| vec![p.to_path_buf()])
                    .unwrap_or_default()
            }
        }
        None => return,
    };

    handle.spawn(async move {
        // Décodage PNG dans le thread-pool (opération bloquante)
        let frames: Vec<(Vec<u8>, u32, u32)> = tokio::task::spawn_blocking(move || {
            paths
                .iter()
                .filter_map(|p| cat::sprite::load_sprite_bgra(p, color, scale).ok())
                .collect()
        })
        .await
        .unwrap_or_default();

        let payload = Box::new((idx, cat_state, cat_dir, frames));
        let raw = Box::into_raw(payload) as isize;
        unsafe {
            let _ = PostMessageW(
                Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                WM_FRAMES_READY,
                WPARAM(idx),
                LPARAM(raw),
            );
        }
    });
}
