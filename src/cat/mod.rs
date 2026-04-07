// cat/mod.rs — CatInstance : état complet d'un chat vivant.

pub mod animation;
pub mod sprite;
pub mod state;

use std::path::PathBuf;

use windows::Win32::Foundation::HWND;

use crate::config::CatConfig;
use state::{CatState, Direction};

// ── Structure principale ──────────────────────────────────────────────────────

pub struct CatInstance {
    // Identité
    pub id: String,
    pub color_id: String,
    pub name: String,

    // Win32
    pub hwnd: HWND,

    // Position et mouvement (coordonnées écran, px)
    pub x: f32,
    pub y: f32,
    pub dir: Direction,

    // Animation
    pub state: CatState,
    pub frame_idx: usize,

    // Drag
    pub is_dragging: bool,
    pub drag_offset_x: i32,
    pub drag_offset_y: i32,

    // Chat Ollama window
    pub bubble: Option<crate::ui::chat_bubble::ChatBubble>,
    pub messages: Vec<serde_json::Value>,

    // Frames précalculées (BGRA premul) pour l'état courant
    pub cached_frames: Vec<(Vec<u8>, u32, u32)>,
    pub cached_state: Option<CatState>,
    pub cached_dir: Option<Direction>,
    /// Chargement async de frames en cours (évite les spawns dupliqués)
    pub frames_loading: bool,
}

impl CatInstance {
    pub fn new(cfg: &CatConfig, hwnd: HWND, x: f32, y: f32) -> Self {
        Self {
            id: cfg.id.clone(),
            color_id: cfg.color_id.clone(),
            name: cfg.name.clone(),
            hwnd,
            x,
            y,
            dir: Direction::South,
            state: CatState::Idle,
            frame_idx: 0,
            is_dragging: false,
            drag_offset_x: 0,
            drag_offset_y: 0,
            bubble: None,
            messages: vec![],
            cached_frames: vec![],
            cached_state: None,
            cached_dir: None,
            frames_loading: false,
        }
    }

    /// Avance d'un frame. Si l'animation est terminée et était one-shot,
    /// repasse en Idle. Retourne `true` si le frame a changé.
    pub fn tick_frame(&mut self) -> bool {
        let n = self.cached_frames.len();
        if n == 0 {
            return false;
        }
        let prev = self.frame_idx;
        self.frame_idx = (self.frame_idx + 1) % n;

        // One-shot : retour à Idle après le dernier frame
        if self.state.is_one_shot() && self.frame_idx == 0 {
            self.state = CatState::Idle;
        }
        self.frame_idx != prev || n == 1
    }

    /// Frame courant (BGRA premul, largeur, hauteur).
    pub fn current_frame(&self) -> Option<&(Vec<u8>, u32, u32)> {
        self.cached_frames.get(self.frame_idx)
    }
}
