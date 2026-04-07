// cat/animation.rs — Parse metadata.json au démarrage → table d'animations.
// Accès par (CatState, Direction) → Vec<PathBuf> trié par frame index.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use super::state::{CatState, Direction};

// ── Structs JSON ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MetaRoot {
    frames: MetaFrames,
}

#[derive(Debug, Deserialize)]
struct MetaFrames {
    animations: HashMap<String, HashMap<String, Vec<String>>>,
    rotations: HashMap<String, String>,
}

// ── Type principal ────────────────────────────────────────────────────────────

pub struct AnimationTable {
    /// (CatState, Direction) → frames ordonnées
    anims: HashMap<(CatState, Direction), Vec<PathBuf>>,
    /// Direction → frame de rotation statique
    rotations: HashMap<Direction, PathBuf>,
    /// Répertoire de base (contient metadata.json et les sous-dossiers)
    base_dir: PathBuf,
}

impl AnimationTable {
    /// Charge metadata.json depuis le répertoire `assets_dir`.
    /// Retourne une erreur si le fichier est absent ou mal formé.
    pub fn load(assets_dir: &Path) -> Result<Self, String> {
        let meta_path = assets_dir.join("metadata.json");
        let data = std::fs::read(&meta_path)
            .map_err(|e| format!("Impossible de lire {}: {e}", meta_path.display()))?;

        let root: MetaRoot = serde_json::from_slice(&data)
            .map_err(|e| format!("Erreur metadata.json: {e}"))?;

        let mut anims: HashMap<(CatState, Direction), Vec<PathBuf>> = HashMap::new();
        let mut rotations: HashMap<Direction, PathBuf> = HashMap::new();

        // ── rotations ────────────────────────────────────────────────────────
        for (dir_str, rel_path) in &root.frames.rotations {
            if let Some(dir) = parse_direction(dir_str) {
                rotations.insert(dir, assets_dir.join(rel_path));
            }
        }

        // ── animations ───────────────────────────────────────────────────────
        for (anim_name, dir_map) in &root.frames.animations {
            let state = match anim_name.as_str() {
                "running-8-frames" => CatState::Walking,
                "eating" => CatState::Eating,
                "drinking" => CatState::Drinking,
                "angry" => CatState::Angry,
                "waking-getting-up" => CatState::WakingUp,
                _ => continue,
            };

            for (dir_str, frames) in dir_map {
                if let Some(dir) = parse_direction(dir_str) {
                    let mut paths: Vec<PathBuf> = frames
                        .iter()
                        .map(|f| assets_dir.join(f))
                        .collect();
                    // Les frames sont déjà ordonnées par l'ordre du JSON,
                    // mais on trie par nom de fichier pour être robuste.
                    paths.sort_by(|a, b| {
                        let na = a.file_name().unwrap_or_default();
                        let nb = b.file_name().unwrap_or_default();
                        na.cmp(nb)
                    });
                    anims.insert((state, dir), paths);
                }
            }
        }

        Ok(Self {
            anims,
            rotations,
            base_dir: assets_dir.to_path_buf(),
        })
    }

    /// Frames pour (état, direction). Retourne slice vide si inconnu.
    pub fn frames(&self, state: CatState, dir: Direction) -> &[PathBuf] {
        self.anims
            .get(&(state, dir))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Frame de rotation statique.
    pub fn rotation(&self, dir: Direction) -> Option<&Path> {
        self.rotations.get(&dir).map(PathBuf::as_path)
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }
}

fn parse_direction(s: &str) -> Option<Direction> {
    match s {
        "south" => Some(Direction::South),
        "south-east" => Some(Direction::SouthEast),
        "east" => Some(Direction::East),
        "north-east" => Some(Direction::NorthEast),
        "north" => Some(Direction::North),
        "north-west" => Some(Direction::NorthWest),
        "west" => Some(Direction::West),
        "south-west" => Some(Direction::SouthWest),
        _ => None,
    }
}
