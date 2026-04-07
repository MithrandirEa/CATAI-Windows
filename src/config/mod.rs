// config/mod.rs — Persistance JSON dans %APPDATA%\CATAI\config.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Constantes d'application ─────────────────────────────────────────────────

pub const WALK_SPEED: f32 = 4.0; // pixels par frame (10 FPS)
pub const MIN_SCALE: f32 = 0.5;
pub const MAX_SCALE: f32 = 3.0;
pub const DEFAULT_SCALE: f32 = 1.0;
pub const MEM_MAX: usize = 20;
pub const OLLAMA_URL: &str = "http://localhost:11434";

pub const TIMER_RENDER: usize = 1; // 100ms — 10 FPS
pub const TIMER_BEHAVIOR: usize = 2; // 1000ms
pub const TIMER_TASKBAR: usize = 3; // 5000ms
pub const TIMER_MOUSE: usize = 4; // 33ms — drag

// ── Définitions des couleurs/personnalités ────────────────────────────────────

pub struct CatColorDef {
    pub id: &'static str,
    pub hue_shift: f32,
    pub sat_mul: f32,
    pub bri_off: f32,
    /// [fr, en, es]
    pub traits: [&'static str; 3],
    pub default_names: [&'static str; 3],
    pub skills: [&'static str; 3],
}

impl CatColorDef {
    pub fn prompt(&self, name: &str, lang: &str) -> String {
        let (t, s) = match lang {
            "en" => (self.traits[1], self.skills[1]),
            "es" => (self.traits[2], self.skills[2]),
            _ => (self.traits[0], self.skills[0]),
        };
        match lang {
            "en" => format!(
                "You are a little {t} cat named {name}. {s} Respond briefly with cat sounds (meow, purr, mrrp). Max 2-3 sentences."
            ),
            "es" => format!(
                "Eres un gatito {t} llamado {name}. {s} Responde brevemente con sonidos de gato (miau, purr, mrrp). Máximo 2-3 frases."
            ),
            _ => format!(
                "Tu es un petit chat {t} nommé {name}. {s} Réponds brièvement avec des sons de chat (miaou, purr, mrrp). Max 2-3 phrases."
            ),
        }
    }
}

pub const CAT_COLOR_DEFS: &[CatColorDef] = &[
    CatColorDef {
        id: "orange",
        hue_shift: 0.0,
        sat_mul: 1.0,
        bri_off: 0.0,
        traits: ["joueur et espiègle", "playful and mischievous", "juguetón y travieso"],
        default_names: ["Citrouille", "Pumpkin", "Calabaza"],
        skills: [
            "Tu adores les blagues et jeux de mots.",
            "You love jokes and puns.",
            "Adoras los chistes y juegos de palabras.",
        ],
    },
    CatColorDef {
        id: "black",
        hue_shift: 0.0,
        sat_mul: 0.1,
        bri_off: -0.45,
        traits: ["mystérieux et philosophe", "mysterious and philosophical", "misterioso y filósofo"],
        default_names: ["Ombre", "Shadow", "Sombra"],
        skills: [
            "Tu poses des questions profondes et aimes réfléchir.",
            "You ask deep questions and love to reflect.",
            "Haces preguntas profundas y te encanta reflexionar.",
        ],
    },
    CatColorDef {
        id: "white",
        hue_shift: 0.0,
        sat_mul: 0.05,
        bri_off: 0.4,
        traits: ["élégant et poétique", "elegant and poetic", "elegante y poético"],
        default_names: ["Neige", "Snow", "Nieve"],
        skills: [
            "Tu t'exprimes avec grâce et tu adores la poésie.",
            "You speak gracefully and love poetry.",
            "Te expresas con gracia y adoras la poesía.",
        ],
    },
    CatColorDef {
        id: "grey",
        hue_shift: 0.0,
        sat_mul: 0.0,
        bri_off: -0.05,
        traits: ["sage et savant", "wise and scholarly", "sabio y erudito"],
        default_names: ["Einstein", "Einstein", "Einstein"],
        skills: [
            "Tu expliques des faits scientifiques fascinants.",
            "You explain fascinating scientific facts.",
            "Explicas datos científicos fascinantes.",
        ],
    },
    CatColorDef {
        id: "brown",
        hue_shift: -0.03,
        sat_mul: 0.7,
        bri_off: -0.2,
        traits: ["aventurier et conteur", "adventurous storyteller", "aventurero y cuentacuentos"],
        default_names: ["Indiana", "Indiana", "Indiana"],
        skills: [
            "Tu racontes des aventures extraordinaires.",
            "You tell extraordinary adventures.",
            "Cuentas aventuras extraordinarias.",
        ],
    },
    CatColorDef {
        id: "cream",
        hue_shift: 0.02,
        sat_mul: 0.3,
        bri_off: 0.15,
        traits: ["câlin et réconfortant", "cuddly and comforting", "cariñoso y reconfortante"],
        default_names: ["Caramel", "Caramel", "Caramelo"],
        skills: [
            "Tu remontes le moral avec tendresse.",
            "You comfort with tenderness.",
            "Animas con ternura.",
        ],
    },
];

pub fn color_def(id: &str) -> Option<&'static CatColorDef> {
    CAT_COLOR_DEFS.iter().find(|c| c.id == id)
}

// ── Structures persistées ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatConfig {
    pub id: String,
    pub color_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub cats: Vec<CatConfig>,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_lang")]
    pub lang: String,
}

fn default_scale() -> f64 {
    DEFAULT_SCALE as f64
}
fn default_lang() -> String {
    "fr".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            cats: vec![CatConfig {
                id: uuid::Uuid::new_v4().to_string(),
                color_id: "orange".into(),
                name: "Citrouille".into(),
            }],
            scale: DEFAULT_SCALE as f64,
            model: String::new(),
            lang: "fr".into(),
        }
    }
}

// ── Chemin de stockage ────────────────────────────────────────────────────────

pub fn config_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata).join("CATAI")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn mem_path(cat_id: &str) -> PathBuf {
    config_dir().join(format!("mem_{cat_id}.json"))
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(cfg) = serde_json::from_slice::<AppConfig>(&bytes) {
            return cfg;
        }
    }
    AppConfig::default()
}

pub fn save_config(cfg: &AppConfig) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_vec_pretty(cfg) {
        let _ = std::fs::write(config_path(), json);
    }
}

pub fn load_memory(cat_id: &str) -> Vec<serde_json::Value> {
    if let Ok(bytes) = std::fs::read(mem_path(cat_id)) {
        if let Ok(v) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) {
            return v;
        }
    }
    vec![]
}

pub fn save_memory(cat_id: &str, msgs: &[serde_json::Value]) {
    let mut s = msgs.to_vec();
    if s.len() > MEM_MAX * 2 + 1 {
        let tail: Vec<_> = s.iter().rev().take(MEM_MAX * 2).rev().cloned().collect();
        s = std::iter::once(s[0].clone()).chain(tail).collect();
    }
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_vec_pretty(&s) {
        let _ = std::fs::write(mem_path(cat_id), json);
    }
}

pub fn delete_memory(cat_id: &str) {
    let _ = std::fs::remove_file(mem_path(cat_id));
}
