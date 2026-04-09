// l10n.rs — Localisation FR/EN/ES + meows aléatoires.

use std::collections::HashMap;

pub struct L10n;

impl L10n {
    pub fn s(key: &'static str, lang: &str) -> &'static str {
        STRINGS
            .get(key)
            .and_then(|m| m.get(lang).or_else(|| m.get("fr")))
            .copied()
            .unwrap_or(key)
    }

    pub fn random_meow(lang: &str) -> &'static str {
        use std::time::{SystemTime, UNIX_EPOCH};
        let meows = MEOWS.get(lang).or_else(|| MEOWS.get("fr")).unwrap();
        let idx = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0)
            % meows.len();
        meows[idx]
    }
}

// ── Tables statiques ──────────────────────────────────────────────────────────

static STRINGS: std::sync::LazyLock<HashMap<&'static str, HashMap<&'static str, &'static str>>> =
    std::sync::LazyLock::new(|| {
        let mut m: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
        macro_rules! entry {
            ($key:expr, $fr:expr, $en:expr, $es:expr) => {{
                let mut inner = HashMap::new();
                inner.insert("fr", $fr);
                inner.insert("en", $en);
                inner.insert("es", $es);
                m.insert($key, inner);
            }};
        }
        entry!("title", ":: RÉGLAGES ::", ":: SETTINGS ::", ":: AJUSTES ::");
        entry!("cats", "MES CHATS", "MY CATS", "MIS GATOS");
        entry!("name", "Nom :", "Name:", "Nombre:");
        entry!("size", "TAILLE", "SIZE", "TAMAÑO");
        entry!("model", "MODÈLE OLLAMA", "OLLAMA MODEL", "MODELO OLLAMA");
        entry!("quit", "Quitter", "Quit", "Salir");
        entry!("settings", "Réglages...", "Settings...", "Ajustes...");
        entry!(
            "talk",
            "Parle au chat...",
            "Talk to the cat...",
            "Habla al gato..."
        );
        entry!(
            "hi",
            "Miaou! ~(=^..^=)~",
            "Meow! ~(=^..^=)~",
            "¡Miau! ~(=^..^=)~"
        );
        entry!("loading", "Chargement...", "Loading...", "Cargando...");
        entry!(
            "no_ollama",
            "(Ollama indisponible)",
            "(Ollama unavailable)",
            "(Ollama no disponible)"
        );
        entry!(
            "err",
            "Mrrp... pas de connexion",
            "Mrrp... no connection",
            "Mrrp... sin conexión"
        );
        entry!("lang_label", "LANGUE", "LANGUAGE", "IDIOMA");
        entry!("max_cats", "Chats (max 6) :", "Cats (max 6):", "Gatos (máx 6):");
        entry!("color_label", "Couleur :", "Color:", "Color:");
        entry!("add_cat", "+ Ajouter", "+ Add", "+ Agregar");
        entry!("remove_cat", "- Supprimer", "- Remove", "- Eliminar");
        entry!("save", "Enregistrer", "Save", "Guardar");
        entry!("cancel", "Annuler", "Cancel", "Cancelar");
        entry!("fetch_models", "Récupérer", "Fetch", "Obtener");
        entry!("clear_mem", "Effacer mémoire", "Clear memory", "Borrar memoria");
        entry!("available_models", "Modèles disponibles :", "Available models:", "Modelos disponibles:");
        m
    });

static MEOWS: std::sync::LazyLock<HashMap<&'static str, Vec<&'static str>>> =
    std::sync::LazyLock::new(|| {
        let mut m: HashMap<&str, Vec<&str>> = HashMap::new();
        m.insert(
            "fr",
            vec![
                "Miaou~",
                "Mrrp!",
                "Prrrr...",
                "Miaou miaou!",
                "Nyaa~",
                "*ronron*",
                "Mew!",
                "Prrrt?",
            ],
        );
        m.insert(
            "en",
            vec![
                "Meow~", "Mrrp!", "Purrrr...", "Meow meow!", "Nyaa~", "*purr*", "Mew!", "Prrrt?",
            ],
        );
        m.insert(
            "es",
            vec![
                "¡Miau~!",
                "Mrrp!",
                "Purrrr...",
                "¡Miau miau!",
                "Nyaa~",
                "*ronroneo*",
                "Mew!",
                "Prrrt?",
            ],
        );
        m
    });
