// app.rs — AppState global (Arc<Mutex<>>) partagé entre le thread UI et tokio.

use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::HWND;

use crate::{
    cat::{animation::AnimationTable, CatInstance},
    config::AppConfig,
    system::taskbar::TaskbarInfo,
};

// ── Messages WM_APP ───────────────────────────────────────────────────────────

/// WM_APP + 1 : token Ollama reçu. WPARAM = cat index, LPARAM = Box<String> leak.
pub const WM_OLLAMA_TOKEN: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;
/// WM_APP + 2 : streaming Ollama terminé. WPARAM = cat index.
pub const WM_OLLAMA_DONE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;
/// WM_APP + 3 : erreur Ollama. WPARAM = cat index, LPARAM = Box<String> leak.
pub const WM_OLLAMA_ERR: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;
/// WM_APP + 4 : modèles disponibles reçus depuis tokio.
/// WPARAM = 0, LPARAM = Box<Vec<String>> leak.
pub const WM_MODELS_READY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 4;
pub const WM_CONFIG_CHANGED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 5;
/// WM_APP + 6 : frames précalculées prêtes (chargées depuis tokio).
/// WPARAM = cat index, LPARAM = Box<(usize, CatState, Direction, Vec<(Vec<u8>,u32,u32)>)> leak.
pub const WM_FRAMES_READY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 6;
/// WM_APP + 7 : envoyé à la fenêtre settings pour rafraîchir la liste des modèles.
pub const WM_MODELS_UPDATED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 7;

/// Message tray (uCallbackMessage dans NOTIFYICONDATAW).
pub const WM_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;

// ── AppState ──────────────────────────────────────────────────────────────────

pub struct AppState {
    pub config: AppConfig,
    pub cats: Vec<CatInstance>,
    pub anim_table: Option<AnimationTable>,
    pub taskbar: Option<TaskbarInfo>,
    pub available_models: Vec<String>,
    /// HWND de la fenêtre message-only (pour PostMessage depuis tokio).
    pub msg_hwnd: HWND,
    /// HWND de la fenêtre de réglages ouverte (HWND::default() si fermée).
    pub settings_hwnd: HWND,
    /// Handle tokio pour spawner des tâches depuis les callbacks Win32.
    pub tokio_handle: Option<tokio::runtime::Handle>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            cats: vec![],
            anim_table: None,
            taskbar: None,
            available_models: vec![],
            msg_hwnd: HWND::default(),
            settings_hwnd: HWND::default(),
            tokio_handle: None,
        }
    }

    pub fn lang(&self) -> &str {
        &self.config.lang
    }
}

pub type SharedState = Arc<Mutex<AppState>>;
