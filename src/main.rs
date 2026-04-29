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
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
            Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW,
                GetCursorPos, GetMessageW, GetWindowRect, KillTimer, PostQuitMessage,
                RegisterClassExW, SetTimer, TranslateMessage,
                CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, MSG,
                WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW, PostMessageW,
                WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_TIMER,
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

use app::{AppState, SharedState, WM_CONFIG_CHANGED, WM_DEFERRED_INIT, WM_FRAMES_READY, WM_MODELS_READY, WM_MODELS_UPDATED, WM_OLLAMA_DONE, WM_OLLAMA_ERR, WM_OLLAMA_TOKEN, WM_TASKBAR_UPDATED, WM_TRAY, WM_USER_CANCEL_CHAT, WM_USER_INPUT};
use cat::{animation::AnimationTable, sprite::load_sprite_bgra, CatInstance};
use config::{load_config, color_def, TIMER_BEHAVIOR, TIMER_CLICK_PAUSE, TIMER_RENDER, TIMER_TASKBAR, WALK_SPEED};
use system::taskbar::get_taskbar_info;
use ui::{
    layered::{create_cat_window, resize_cat_dibs, update_layered, update_layered_fast, CAT_CLASS},
    tray::{add_tray_icon, remove_tray_icon, show_context_menu, MENU_QUIT, MENU_SETTINGS},
    settings::open_settings,
};

// ── Constantes locales ────────────────────────────────────────────────────────

const MSG_CLASS: PCWSTR = w!("CATAI_Msg");
const MSG_WND_TITLE: PCWSTR = w!("CATAI_MsgWindow");

/// Seuil en pixels au-delà duquel un déplacement souris est considéré comme un drag.
const DRAG_THRESHOLD: i32 = 10;

// Message posté au chat lié quand l'utilisateur clique la bulle de dialogue.
// Toujours traité comme "continuer la conversation" (InputBox directe, pas de meow).
const WM_BUBBLE_CLICKED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 14;
static INPUT_BOX: std::sync::OnceLock<ui::input_box::InputBox> = std::sync::OnceLock::new();

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
        // taskbar sera initialisée dans WM_DEFERRED_INIT, après démarrage de la boucle
    }

    // 6. Le runtime tokio est créé dans WM_DEFERRED_INIT (après la boucle de messages)
    //    pour éviter de spawner N threads workers AVANT que la fenêtre soit visible.

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

        // 10. Créer les fenêtres chat (position par défaut, taskbar non encore connue)
        init_cats(&state, hinstance)?;

        // 10b. Créer l'InputBox de manière synchrone (la classe INPUT_CLASS est déjà
        //      enregistrée et msg_hwnd est valide). Ne dépend pas de la taskbar.
        if INPUT_BOX.get().is_none() {
            match ui::input_box::InputBox::new(msg_hwnd) {
                Ok(ib) => { let _ = INPUT_BOX.set(ib); }
                Err(e) => {
                    eprintln!("CATAI: InputBox::new failed: {:?}", e);
                }
            }
        }

        // 11. Démarrer les timers sur la fenêtre message
        SetTimer(Some(msg_hwnd), TIMER_RENDER, 100, None);
        SetTimer(Some(msg_hwnd), TIMER_BEHAVIOR, 1000, None);
        SetTimer(Some(msg_hwnd), TIMER_TASKBAR, 5000, None);

        // 12. Initialisation différée : taskbar + icône tray seront gérés dans WM_DEFERRED_INIT
        // après le premier tour de boucle, évitant de bloquer sur SHAppBarMessage/Shell_NotifyIconW
        // avant que GetMessageW soit prêt à pomper.
        let _ = PostMessageW(Some(msg_hwnd), WM_DEFERRED_INIT, WPARAM(0), LPARAM(0));

        // 13. Boucle de messages Win32
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

/// WndProc des fenêtres bulle de dialogue.
/// Détection de clic via WM_LBUTTONUP — fonctionne car tous les pixels ont alpha > 0
/// (garanti par la boucle alpha-fix dans render()). WM_NCHITTEST retourne HTCLIENT
/// pour que les clics soient bien transmis à la fenêtre WS_EX_LAYERED.
unsafe extern "system" fn bubble_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWLP_USERDATA};
    match msg {
        WM_NCHITTEST => LRESULT(1), // HTCLIENT — nécessaire pour recevoir les clics souris
        WM_LBUTTONUP => {
            // Poster WM_BUBBLE_CLICKED au chat associé (GWLP_USERDATA = cat HWND via link_cat).
            let cat_hwnd_raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if cat_hwnd_raw != 0 {
                let cat_hwnd = HWND(cat_hwnd_raw as *mut core::ffi::c_void);
                let _ = PostMessageW(Some(cat_hwnd), WM_BUBBLE_CLICKED, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
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

    // Classe fenêtre bulle — cliquable via bubble_wnd_proc
    wc.lpfnWndProc = Some(bubble_wnd_proc);
    wc.lpszClassName = ui::chat_bubble::BUBBLE_CLASS;
    wc.style = CS_HREDRAW | CS_VREDRAW;
    RegisterClassExW(&wc);

    // Classe fenêtre de saisie utilisateur
    wc.lpfnWndProc = Some(ui::input_box::input_wnd_proc);
    wc.lpszClassName = ui::input_box::INPUT_CLASS;
    wc.style = CS_HREDRAW | CS_VREDRAW;
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
        // Restaurer l'historique de conversation sauvegardé
        instance.messages = config::load_memory(&cat_cfg.id);
        // Cache du frame initial + état pour éviter un rebuild inutile au premier tick
        instance.cached_frames = vec![(bgra, sw, sh)];
        instance.cached_state = Some(cat::state::CatState::Idle);
        instance.cached_dir = Some(cat::state::Direction::South);
        // Créer la bulle de dialogue et stocker le HWND du chat dans le
        // GWLP_USERDATA de la bulle pour que son wndproc puisse émettre
        // WM_BUBBLE_CLICKED vers la bonne fenêtre chat.
        instance.bubble = ui::chat_bubble::ChatBubble::new().ok();
        if let Some(b) = &instance.bubble {
            windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                b.hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                hwnd.0 as isize,
            );
        }

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
    // Libérer les DIBs persistants (seront recréés au prochain rendu)
    resize_cat_dibs(0);
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
                        // Accumuler le token dans l'historique (la bulle ne se met
                        // à jour qu'une seule fois à la fin, dans WM_OLLAMA_DONE)
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

        WM_OLLAMA_DONE => {
            // WPARAM = index du chat — afficher le message assistant complet dans la bulle
            let idx = wparam.0;
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
                        let sz = cat.current_frame().map(|f| f.1 as i32).unwrap_or(68);
                        let full_text = cat
                            .messages
                            .last()
                            .filter(|m| {
                                m.get("role").and_then(|r| r.as_str()) == Some("assistant")
                            })
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_str())
                            .map(|s| s.to_owned());
                        if let (Some(text), Some(bubble)) = (full_text, &mut cat.bubble) {
                            bubble.hide_and_clear();
                            let _ = bubble.append(&text, cx, cy, sz);
                            cat.bubble_shown_at = Some(std::time::Instant::now());
                            cat.is_chatting = true;
                        }
                        // Persister l'historique hors UI thread pour éviter les freezes I/O.
                        let cat_id = cat.id.clone();
                        let msgs = cat.messages.clone();
                        std::thread::spawn(move || { config::save_memory(&cat_id, &msgs); });
                    }
                }
                std::mem::forget(state);
            }
            LRESULT(0)
        }

        // Message utilisateur saisi dans l'InputBox → construire le prompt et lancer Ollama.
        // WPARAM = cat index, LPARAM = Box<(usize, String)> raw pointer.
        WM_USER_INPUT => {
            if lparam.0 == 0 {
                return LRESULT(0);
            }
            let payload = Box::from_raw(lparam.0 as *mut (usize, String));
            let (idx, user_text) = *payload;

            let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if raw == 0 {
                return LRESULT(0);
            }
            let task_data = {
                let state = Arc::from_raw(raw as *const Mutex<AppState>);
                let result = {
                    let mut s = state.lock().unwrap();
                    let lang = s.config.lang.clone();
                    let model = s.config.model.clone();
                    let msg_hwnd_ptr = s.msg_hwnd.0 as isize;
                    let tokio_handle = s.tokio_handle.clone();
                    if let Some(cat) = s.cats.get_mut(idx) {
                        if cat.messages.is_empty() {
                            let color = config::color_def(&cat.color_id)
                                .unwrap_or(&config::CAT_COLOR_DEFS[0]);
                            let sp = color.prompt(&cat.name, &lang);
                            cat.messages.push(serde_json::json!({"role": "system", "content": sp}));
                        }
                        cat.messages.push(serde_json::json!({"role": "user", "content": user_text}));
                        while cat.messages.len() > config::MEM_MAX * 2 {
                            cat.messages.remove(1);
                        }
                        // Persister le message utilisateur hors UI thread.
                        let cat_id2 = cat.id.clone();
                        let msgs2 = cat.messages.clone();
                        std::thread::spawn(move || { config::save_memory(&cat_id2, &msgs2); });
                        let messages = cat.messages.clone();
                        tokio_handle.map(|h| (messages, model, msg_hwnd_ptr, h))
                    } else {
                        None
                    }
                };
                std::mem::forget(state);
                result
            };
            if let Some((messages, model, msg_hwnd_ptr, handle)) = task_data {
                handle.spawn(async move {
                    let (tx, mut rx) = tokio::sync::mpsc::channel::<ollama::client::OllamaMsg>(64);
                    let consumer = tokio::spawn(async move {
                        while let Some(ollama_msg) = rx.recv().await {
                            match ollama_msg {
                                ollama::client::OllamaMsg::Token(t) => {
                                    let payload_ptr = Box::into_raw(Box::new((idx, t))) as isize;
                                    unsafe {
                                        let _ = PostMessageW(
                                            Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                                            WM_OLLAMA_TOKEN, WPARAM(idx), LPARAM(payload_ptr),
                                        );
                                    }
                                }
                                ollama::client::OllamaMsg::Done => {
                                    unsafe {
                                        let _ = PostMessageW(
                                            Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                                            WM_OLLAMA_DONE, WPARAM(idx), LPARAM(0),
                                        );
                                    }
                                    break;
                                }
                                ollama::client::OllamaMsg::Error(e) => {
                                    let s_ptr = Box::into_raw(Box::new(e)) as isize;
                                    unsafe {
                                        let _ = PostMessageW(
                                            Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                                            WM_OLLAMA_ERR, WPARAM(idx), LPARAM(s_ptr),
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                    });
                    ollama::client::stream_chat(config::OLLAMA_URL, &model, messages, tx).await;
                    let _ = consumer.await;
                });
            }
            LRESULT(0)
        }

        WM_OLLAMA_ERR => {
            // Libérer le Box<String> de l'erreur
            if lparam.0 != 0 {
                drop(Box::from_raw(lparam.0 as *mut String));
            }
            LRESULT(0)
        }

        // InputBox fermée sans saisie → reprendre le comportement normal du chat.
        WM_USER_CANCEL_CHAT => {
            let idx = wparam.0;
            let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if raw != 0 {
                let state = Arc::from_raw(raw as *const Mutex<AppState>);
                {
                    let mut s = state.lock().unwrap();
                    if let Some(cat) = s.cats.get_mut(idx) {
                        // Ignorer si le timer est encore en attente (InputBox pas encore montrée).
                        // Evite les annulations parasites arrivant pendant le délai de 500ms.
                        if !cat.click_input_pending {
                            cat.is_chatting = false;
                            if let Some(b) = &mut cat.bubble {
                                b.hide_and_clear();
                            }
                            cat.bubble_shown_at = None;
                        }
                    }
                }
                std::mem::forget(state);
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
                                cat.cached_frames = frames.into_iter().map(std::sync::Arc::new).collect();
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

        // Résultat de get_taskbar_info() calculé dans un thread background.
        WM_TASKBAR_UPDATED => {
            if lparam.0 != 0 {
                let info = Box::from_raw(lparam.0 as *mut Option<system::taskbar::TaskbarInfo>);
                let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                );
                if raw != 0 {
                    let state = Arc::from_raw(raw as *const Mutex<AppState>);
                    state.lock().unwrap().taskbar = *info;
                    std::mem::forget(state);
                }
            }
            LRESULT(0)
        }

        // Initialisation différée : SHAppBarMessage + Shell_NotifyIconW sont des appels
        // synchrones vers Explorer.exe. Les exécuter ici (après GetMessageW) évite de
        // bloquer le thread UI pendant les premières secondes si Explorer est occupé.
        WM_DEFERRED_INIT => {
            let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            // Récupérer la taskbar et repositionner les chats
            let taskbar = get_taskbar_info();
            if raw != 0 {
                let state = Arc::from_raw(raw as *const Mutex<AppState>);
                {
                    let mut s = state.lock().unwrap();
                    s.taskbar = taskbar;
                    // Repositionner les chats sur la taskbar maintenant qu'on la connaît
                    let scale = s.config.scale as f32;
                    let cat_px = (68.0 * scale) as i32;
                    let n = s.cats.len();
                    if let Some(tb) = &s.taskbar.clone() {
                        for (i, cat) in s.cats.iter_mut().enumerate() {
                            let (xmin, xmax) = tb.walk_range_x(cat_px);
                            let new_x = xmin + ((xmax - xmin) / (n as i32 + 1)) * (i as i32 + 1);
                            let new_y = tb.cat_y_for_bottom(cat_px);
                            cat.x = new_x as f32;
                            cat.y = new_y as f32;
                        }
                    }
                    // Créer le runtime tokio ici — après la boucle de messages —
                    // pour que les 2 threads workers soient invisibles au démarrage.
                    // worker_threads(2) suffit pour le streaming Ollama.
                    if s.tokio_rt.is_none() {
                        if let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
                            .worker_threads(2)
                            .enable_all()
                            .build()
                        {
                            s.tokio_handle = Some(rt.handle().clone());
                            s.tokio_rt = Some(rt);
                        }
                    }
                }
                std::mem::forget(state);
            }
            // Créer la boîte de saisie (une seule instance globale)
            if INPUT_BOX.get().is_none() {
                let msg_hwnd_for_input = hwnd; // msg_wnd_proc reçoit hwnd = msg_hwnd
                if let Ok(ib) = ui::input_box::InputBox::new(msg_hwnd_for_input) {
                    let _ = INPUT_BOX.set(ib);
                }
            }
            // Ajouter l'icône tray maintenant que la boucle tourne
            let _ = add_tray_icon(hwnd);
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
        // ── Début potentiel du drag — seuil DRAG_THRESHOLD px ───────────────
        WM_LBUTTONDOWN => {
            if let Some(state) = borrow_state!(hwnd) {
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);
                let mut s = state.lock().unwrap();
                if let Some(cat) = s.cats.iter_mut().find(|c| c.hwnd == hwnd) {
                    cat.drag_offset_x = cursor.x - cat.x as i32;
                    cat.drag_offset_y = cursor.y - cat.y as i32;
                    cat.drag_start_x = cursor.x;
                    cat.drag_start_y = cursor.y;
                    // Reset du drag : un WM_MOUSEMOVE antérieur a pu laisser
                    // is_dragging = true — le remettre à false pour ce nouveau clic.
                    cat.is_dragging = false;
                    cat.state = cat::state::CatState::Idle;
                    cat.idle_ticks = 0;
                    // Sauvegarder is_chatting AVANT de le forcer à true.
                    // WM_LBUTTONUP utilisera pre_click_chatting pour distinguer
                    // "conversation déjà active" de "protection drag uniquement".
                    cat.pre_click_chatting = cat.is_chatting;
                    // Protection immédiate : bloquer on_timer_behavior entre DOWN et UP.
                    // Sera remis à false dans WM_LBUTTONUP si c'était un drag.
                    cat.is_chatting = true;
                    // Annuler un éventuel timer de délai InputBox en cours.
                    cat.click_input_pending = false;
                }
            }
            // Annuler le timer de délai InputBox s'il tournait depuis un clic précédent.
            let _ = KillTimer(Some(hwnd), TIMER_CLICK_PAUSE);
            SetCapture(hwnd);
            LRESULT(0)
        }

        // ── Déplacement pendant le drag (activé après DRAG_THRESHOLD px) ────
        WM_MOUSEMOVE => {
            if let Some(state) = borrow_state!(hwnd) {
                // Extraire données sous verrou, puis libérer avant d'appeler update_layered
                let frame_data = {
                    let mut s = state.lock().unwrap();
                    let scale = s.config.scale as f32;
                    let cat_px = (68.0 * scale) as i32;
                    if let Some(cat) = s.cats.iter_mut().find(|c| c.hwnd == hwnd) {
                        // Activer le drag une fois le seuil DRAG_THRESHOLD atteint.
                        if !cat.is_dragging && GetCapture() == hwnd {
                            let mut cursor = POINT::default();
                            let _ = GetCursorPos(&mut cursor);
                            let dx = (cursor.x - cat.drag_start_x).abs();
                            let dy = (cursor.y - cat.drag_start_y).abs();
                            if dx > DRAG_THRESHOLD || dy > DRAG_THRESHOLD {
                                cat.is_dragging = true;
                            }
                        }
                        if cat.is_dragging {
                            let mut cursor = POINT::default();
                            let _ = GetCursorPos(&mut cursor);
                            let new_x = cursor.x - cat.drag_offset_x;
                            let new_y = cursor.y - cat.drag_offset_y;
                            cat.x = new_x as f32;
                            cat.y = new_y as f32;
                            // Repositionner la bulle si elle est affichée pendant le drag
                            if cat.bubble_shown_at.is_some() {
                                if let Some(bubble) = &mut cat.bubble {
                                    bubble.reposition(new_x, new_y, cat_px);
                                }
                            }
                            cat.current_frame().map(|f| (Arc::clone(f), new_x, new_y))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some((frame, new_x, new_y)) = frame_data {
                    let _ = update_layered(hwnd, &frame.0, frame.1, frame.2, new_x, new_y);
                }
            }
            LRESULT(0)
        }

        // ── Fin du drag OU clic simple (arrêt + chat) ────────────────────
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            if let Some(state) = borrow_state!(hwnd) {
                let was_dragging;
                let click_params;
                let frame_data;
                let mut start_click_timer = false;
                {
                    let mut s = state.lock().unwrap();
                    let scale = s.config.scale as f32;
                    let cat_px = (68.0 * scale) as i32;
                    let taskbar = s.taskbar.clone();
                    let lang = s.config.lang.clone();
                    if let Some((idx, cat)) = s.cats.iter_mut().enumerate().find(|(_, c)| c.hwnd == hwnd) {
                        was_dragging = cat.is_dragging;
                        cat.is_dragging = false;

                        if was_dragging {
                            // Drag terminé — annuler la protection is_chatting
                            cat.is_chatting = false;
                            // Snap sur la barre des tâches après drag
                            if let Some(tb) = &taskbar {
                                let snap_y = tb.cat_y_for_bottom(cat_px);
                                let (xmin, xmax) = tb.walk_range_x(cat_px);
                                cat.y = snap_y as f32;
                                cat.x = (cat.x as i32).clamp(xmin, xmax) as f32;
                            }
                            let new_x = cat.x as i32;
                            let new_y = cat.y as i32;
                            if cat.bubble_shown_at.is_some() {
                                if let Some(bubble) = &mut cat.bubble {
                                    bubble.reposition(new_x, new_y, cat_px);
                                }
                            }
                            frame_data = cat.current_frame().map(|f| (Arc::clone(f), new_x, new_y));
                            click_params = None;
                        } else {
                            // Clic sur un chat actif ou inactif
                            cat.state = cat::state::CatState::Idle;
                            cat.idle_ticks = 0;
                            let cx = cat.x as i32;
                            let cy = cat.y as i32;
                            let sz = cat.current_frame().map(|f| f.1 as i32).unwrap_or(68);

                            if cat.pre_click_chatting {
                                // Conversation déjà active avant ce clic → rouvrir l'InputBox
                                // directement, sans meow ni délai. Réinitialiser bubble_shown_at
                                // pour éviter la race condition avec on_timer_behavior (~8s).
                                cat.bubble_shown_at = None;
                                cat.click_input_pending = false;
                                if let Some(b) = &mut cat.bubble {
                                    b.hide_and_clear();
                                }
                                frame_data = None;
                                click_params = Some((idx, cx, cy, sz));
                                start_click_timer = false;
                            } else {
                                // Nouvelle conversation → meow + délai 500ms avant InputBox
                                cat.is_chatting = true;
                                cat.click_input_pending = true;
                                if let Some(b) = &mut cat.bubble {
                                    b.hide_and_clear();
                                    let _ = b.append(l10n::L10n::random_meow(&lang), cx, cy, sz);
                                }
                                // bubble_shown_at initialisé dans TIMER_CLICK_PAUSE pour démarrer
                                // le compte à rebours 8s seulement quand l'InputBox est visible.
                                frame_data = None;
                                click_params = None;
                                start_click_timer = true;
                            }
                        }
                    } else {
                        was_dragging = false;
                        frame_data = None;
                        click_params = None;
                    }
                }
                // Mutex relâché — appels hors du verrou
                if let Some((frame, new_x, new_y)) = frame_data {
                    let _ = update_layered(hwnd, &frame.0, frame.1, frame.2, new_x, new_y);
                }
                if let Some((idx, cx, cy, sz)) = click_params {
                    if let Some(ib) = INPUT_BOX.get() {
                        ib.show(cx, cy, sz, idx);
                    }
                }
                if start_click_timer {
                    // Afficher l'InputBox après 500ms pour laisser le chat s'arrêter.
                    let _ = SetTimer(Some(hwnd), TIMER_CLICK_PAUSE, 500, None);
                }
                let _ = was_dragging; // suppress unused warning
            }
            LRESULT(0)
        }

        // ── Clic sur la bulle de dialogue → toujours InputBox directe ─────────
        // Envoyé par bubble_wnd_proc. La bulle n'existe que lorsqu'une conversation
        // est active, donc on n'a pas besoin de passer par meow + délai.
        x if x == WM_BUBBLE_CLICKED => {
            let _ = KillTimer(Some(hwnd), TIMER_CLICK_PAUSE);
            let show_params = if let Some(state) = borrow_state!(hwnd) {
                let mut s = state.lock().unwrap();
                let scale = s.config.scale as f32;
                let cat_px = (68.0 * scale) as i32;
                s.cats.iter_mut().enumerate().find(|(_, c)| c.hwnd == hwnd).map(|(idx, cat)| {
                    cat.is_chatting = true;
                    cat.click_input_pending = false;
                    cat.bubble_shown_at = None;
                    if let Some(b) = &mut cat.bubble {
                        b.hide_and_clear();
                    }
                    let cx = cat.x as i32;
                    let cy = cat.y as i32;
                    let sz = cat.current_frame().map(|f| f.1 as i32).unwrap_or(cat_px);
                    (idx, cx, cy, sz)
                })
            } else {
                None
            };
            if let Some((idx, cx, cy, sz)) = show_params {
                if let Some(ib) = INPUT_BOX.get() {
                    ib.show(cx, cy, sz, idx);
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONDBLCLK => {
            // Annuler le timer de délai InputBox si un double-clic intervient avant 2s.
            let _ = KillTimer(Some(hwnd), TIMER_CLICK_PAUSE);
            // Collecter les infos sous verrou, puis relâcher AVANT d'appeler show().
            // ShowWindow/SetForegroundWindow envoient des messages Win32 synchrones
            // qui pourraient tenter de re-verrouiller l'AppState → deadlock.
            let show_params = if let Some(state) = borrow_state!(hwnd) {
                let mut s = state.lock().unwrap();
                let lang = s.config.lang.clone();
                s.cats.iter_mut().enumerate().find(|(_, c)| c.hwnd == hwnd).map(|(idx, cat)| {
                    cat.state = cat::state::CatState::Idle;
                    cat.idle_ticks = 0;
                    cat.is_chatting = true;
                    cat.click_input_pending = false;
                    let cx = cat.x as i32;
                    let cy = cat.y as i32;
                    let sz = cat.current_frame().map(|f| f.1 as i32).unwrap_or(68);
                    if let Some(b) = &mut cat.bubble {
                        b.hide_and_clear();
                        let _ = b.append(l10n::L10n::random_meow(&lang), cx, cy, sz);
                    }
                    cat.bubble_shown_at = Some(std::time::Instant::now());
                    (idx, cx, cy, sz)
                })
            } else {
                None
            };
            // Mutex relâché — appel show() hors du verrou
            if let Some((idx, cx, cy, sz)) = show_params {
                if let Some(ib) = INPUT_BOX.get() {
                    ib.show(cx, cy, sz, idx);
                }
            }
            LRESULT(0)
        }

        // ── Timer de délai InputBox : afficher l'InputBox 2s après un clic simple ──
        WM_TIMER => {
            if wparam.0 == TIMER_CLICK_PAUSE {
                let _ = KillTimer(Some(hwnd), TIMER_CLICK_PAUSE);
                let show_params = if let Some(state) = borrow_state!(hwnd) {
                    let mut s = state.lock().unwrap();
                    let scale = s.config.scale as f32;
                    let cat_px = (68.0 * scale) as i32;
                    s.cats.iter_mut().enumerate()
                        .find(|(_, c)| c.hwnd == hwnd)
                        .and_then(|(idx, cat)| {
                            if cat.is_chatting && cat.click_input_pending {
                                cat.click_input_pending = false;
                                // Démarrer le compte à rebours 8s à partir de l'affichage
                                // de l'InputBox (et non du clic).
                                cat.bubble_shown_at = Some(std::time::Instant::now());
                                let cx = cat.x as i32;
                                let cy = cat.y as i32;
                                let sz = cat.current_frame()
                                    .map(|f| f.1 as i32)
                                    .unwrap_or(cat_px);
                                Some((idx, cx, cy, sz))
                            } else {
                                cat.click_input_pending = false;
                                None
                            }
                        })
                } else {
                    None
                };
                if let Some((idx, cx, cy, sz)) = show_params {
                    if let Some(ib) = INPUT_BOX.get() {
                        ib.show(cx, cy, sz, idx);
                    }
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

    // ── Phase 1 : mettre à jour l'état sous verrou, collecter les données à rendre ──
    let render_jobs: Vec<(usize, HWND, std::sync::Arc<(Vec<u8>, u32, u32)>, i32, i32)> = {
        let mut s = state.lock().unwrap();
        let scale = s.config.scale as f32;
        let cat_px = (68.0 * scale) as i32;
        let taskbar = s.taskbar.clone();
        let mut jobs = Vec::with_capacity(s.cats.len());

        for i in 0..s.cats.len() {
            // Avancer la position si le chat marche (10 FPS)
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
                            s.cats[i].dir = Direction::West;
                            s.cats[i].state = CatState::Idle;
                        }
                    } else {
                        s.cats[i].x -= speed;
                        if s.cats[i].x as i32 <= xmin {
                            s.cats[i].x = xmin as f32;
                            s.cats[i].dir = Direction::East;
                            s.cats[i].state = CatState::Idle;
                        }
                    }
                }
            }

            // Reconstruire le cache si l'état ou la direction a changé
            let needs_rebuild = {
                use cat::state::{CatState, Direction};
                let cat = &s.cats[i];
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

            // Repositionner la bulle à chaque tick — reposition() est un no-op
            // si le chat n'a pas bougé ou si la bulle n'a pas de contenu
            // (les gardes dans ChatBubble::reposition vérifient last_h, text et position).
            if let Some(bubble) = &mut cat.bubble {
                bubble.reposition(cat.x as i32, cat.y as i32, cat_px);
            }

            if cat.tick_frame() || moved {
                if let Some(frame) = cat.current_frame() {
                    // Arc::clone évite le clone des pixels (~20KB) — partage de la référence.
                    jobs.push((i, cat.hwnd, std::sync::Arc::clone(frame), cat.x as i32, cat.y as i32));
                }
            }
        }
        jobs
    }; // ← verrou libéré ici

    // ── Phase 2 : appels GDI hors verrou ─────────────────────────────────────
    for (cat_idx, hwnd, frame, x, y) in render_jobs {
        // Utilise le DIB persistant (pas de CreateDIBSection sur le chemin chaud)
        let _ = update_layered_fast(hwnd, &frame.0, frame.1, frame.2, x, y, cat_idx);
    }
}

unsafe fn on_timer_behavior(state: &SharedState) {
    use cat::state::{CatState, Direction};

    let mut s = state.lock().unwrap();

    for cat in &mut s.cats {
        // Auto-cacher la bulle après 8 secondes d'affichage
        if cat.bubble_shown_at.map_or(false, |t| t.elapsed().as_secs() >= 8) {
            if let Some(b) = &mut cat.bubble {
                b.hide_and_clear();
            }
            cat.bubble_shown_at = None;
            cat.is_chatting = false;
        }

        if cat.is_dragging || cat.is_chatting {
            continue;
        }

        match cat.state {
            CatState::Idle => {
                cat.idle_ticks += 1;

                // Attendre 2-4 ticks (secondes) en Idle avant de choisir
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                let threshold = 2 + (t % 3); // 2, 3 ou 4 secondes
                if cat.idle_ticks >= threshold {
                    cat.idle_ticks = 0;

                    // Utiliser la direction mémorisée ; South = premier démarrage → East
                    if cat.dir == Direction::South {
                        cat.dir = Direction::East;
                    }

                    // Walk 60% | Eat 20% | Drink 20%
                    let roll = t % 10;
                    cat.state = if roll < 6 {
                        CatState::Walking
                    } else if roll < 8 {
                        CatState::Eating
                    } else {
                        CatState::Drinking
                    };
                    cat.frame_idx = 0;
                }
            }
            _ => {} // Mouvement géré dans on_timer_render ; one-shot dans tick_frame
        }
    }
}

unsafe fn on_timer_taskbar(state: &SharedState) {
    let msg_hwnd_ptr = state.lock().unwrap().msg_hwnd.0 as isize;
    // SHAppBarMessage peut bloquer si Explorer est occupé → exécuter hors UI thread.
    std::thread::spawn(move || {
        let info = get_taskbar_info();
        let payload = Box::new(info);
        let raw = Box::into_raw(payload) as isize;
        unsafe {
            let _ = PostMessageW(
                Some(HWND(msg_hwnd_ptr as *mut core::ffi::c_void)),
                WM_TASKBAR_UPDATED,
                WPARAM(0),
                LPARAM(raw),
            );
        }
    });
}

/// Lance un chargement des frames PNG dans un thread std (pas de tokio).
/// Le résultat est PostMessageé via WM_FRAMES_READY → aucun blocage du thread UI.
fn schedule_rebuild_frames(s: &AppState, idx: usize, scale: f32) {
    use cat::state::Direction;
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

    // Thread std — pas de tokio nécessaire pour du I/O bloquant ponctuel.
    std::thread::spawn(move || {
        let frames: Vec<(Vec<u8>, u32, u32)> = paths
            .iter()
            .filter_map(|p| cat::sprite::load_sprite_bgra(p, color, scale).ok())
            .collect();

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
