# CATAI — Instructions GitHub Copilot

## Projet
Réécriture de CATAI (desktop pet cats, originalement Swift/macOS) en **Rust + windows-rs** pour Windows.
Des chats pixel-art 68×68 px animés vivent sur la barre des tâches Windows et discutent via **Ollama** (LLM local, `http://localhost:11434`).

## Consignes générales
- Respecter à la lettre les règles et l'architecture définies dans ce document.
- Code style Rust standard, commentaires clairs, structure modulaire.
- Toujours suivre le fichier check-plan.md pour l'ordre d'implémentation des fonctionnalités **ET LE TENIR À JOUR**.
- Ne jamais faire de compromis sur les règles STRICTES listées à la fin du document.
- En cas de doute, se référer à la section "Architecture critique" pour les décisions techniques majeures.
- Ne jamais installer directement sur le système, toujours utiliser un environnement de développement isolé (ex: WSL, VM, conteneur) pour éviter les risques liés aux erreurs de code.
- Tester chaque fonctionnalité de manière exhaustive avant de passer à la suivante, en suivant les validations définies dans check-plan.md.
- Documenter tout code complexe ou non trivial avec des commentaires explicatifs, surtout pour les interactions avec les API Win32 et les conversions de formats d'image.


## Stack technique
- **Rust** edition 2021, stable toolchain, target `x86_64-pc-windows-msvc`
- **`windows = "0.62"`** pour toutes les API Win32 — PAS de winit, egui, tao, tauri, iced, ni aucun autre framework UI
- **`image = "0.25"`** pour décoder les PNG spritesb
- **`reqwest` + `tokio`** pour le streaming HTTP Ollama
- **`serde` + `serde_json`** pour la persistance JSON
- **`uuid = "1"`** pour les identifiants de chats

## Architecture critique — À RESPECTER ABSOLUMENT

### Threads
- **Thread UI = thread Win32 principal** : boucle `GetMessage` / `TranslateMessage` / `DispatchMessage` dans `main.rs`. Ne jamais bloquer ce thread.
- **Thread async = tokio runtime séparé** lancé au démarrage avec `tokio::runtime::Runtime::new()`.
- **Communication async → UI** : `tokio::mpsc` + `PostMessage(hwnd, WM_APP + N, wparam, lparam)` uniquement. Jamais d'accès direct aux HWNDs depuis tokio.

### Fenêtres chats (transparence pixel-par-pixel)
- Style obligatoire : `WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW` + `WS_POPUP`
- Rendu **uniquement** via `UpdateLayeredWindow` avec un DIB BGRA prémultiplié
- **Jamais de `WM_PAINT`** pour les fenêtres chats
- Pas de shadow (`WS_EX_SHADOW`), pas de bord, fond transparent
- Format bitmap GDI : **BGRA** (Blue, Green, Red, Alpha), alpha prémultiplié (`r_out = r * a / 255`)

### Timers Win32
```rust
const TIMER_RENDER: usize    = 1;  // 100ms  — rendu animation 10 FPS
const TIMER_BEHAVIOR: usize  = 2;  // 1000ms — transitions état + meows
const TIMER_TASKBAR: usize   = 3;  // 5000ms — poll position/auto-hide barre des tâches
const TIMER_MOUSE: usize     = 4;  // 33ms   — polling souris pendant drag
```

## Structure des modules

```
src/
  main.rs          — entry point Win32 + tokio runtime + boucle messages
  app.rs           — AppState global (Arc<Mutex<>>), liste CatInstance, échelle, langue
  cat/
    mod.rs         — CatInstance : HWND, position f32, direction, frame_idx, CatState
    state.rs       — enum CatState { Idle, Walking, Eating, Drinking, Angry, Sleeping, WakingUp }
    animation.rs   — parse metadata.json au démarrage → HashMap<(CatState,Direction), Vec<PathBuf>>
    sprite.rs      — PNG → RGBA (image crate) → HSB tinting → BGRA prémultiplié → HBITMAP via CreateDIBSection
  ui/
    mod.rs
    layered.rs     — fn create_layered_window(...) -> HWND + fn update_layered(hwnd, hbitmap, w, h, x, y)
    tray.rs        — Shell_NotifyIconW NIM_ADD/NIM_MODIFY/NIM_DELETE + TrackPopupMenu
    chat_bubble.rs — fenêtre WS_EX_LAYERED popup texte, tokens streamés, auto-resize hauteur
    settings.rs    — fenêtre Win32 réglages (EditBox noms, ComboBox modèles, Slider échelle, boutons couleurs/langues)
  system/
    mod.rs
    taskbar.rs     — SHAppBarMessage ABM_GETTASKBARPOS + ABM_GETAUTOHIDEBAR
    windows.rs     — EnumWindows + GetWindowRect → coordonnées fenêtre active (pour perching)
    mouse.rs       — SetWindowsHookEx WH_MOUSE_LL
  ollama/
    mod.rs
    client.rs      — reqwest streaming POST /api/chat, parsing JSON ligne par ligne, mpsc sender
  config/
    mod.rs         — CatConfig + AppConfig, serde_json, chemin %APPDATA%\CATAI\config.json
  l10n.rs          — strings FR/EN/ES + meows aléatoires par langue
```

## Données des chats — 6 couleurs avec personnalités

```rust
pub struct CatColorDef {
    pub id: &'static str,
    pub hue_shift: f32,   // décalage teinte HSB
    pub sat_mul: f32,     // multiplicateur saturation
    pub bri_off: f32,     // offset luminosité
    pub traits: [&'static str; 3],   // [fr, en, es]
    pub default_names: [&'static str; 3],
    pub skills: [&'static str; 3],
}
```

Couleurs définies dans `config/mod.rs` comme `pub const CAT_COLOR_DEFS: &[CatColorDef]` :
- `orange` : hue_shift=0, sat_mul=1, bri_off=0 → **aucun tinting** (retourner sprite source)
- `black`  : hue_shift=0, sat_mul=0.1, bri_off=-0.45
- `white`  : hue_shift=0, sat_mul=0.05, bri_off=0.4
- `grey`   : hue_shift=0, sat_mul=0, bri_off=-0.05
- `brown`  : hue_shift=-0.03, sat_mul=0.7, bri_off=-0.2
- `cream`  : hue_shift=0.02, sat_mul=0.3, bri_off=0.15

## HSB Tinting — Port direct du code Swift (`sprite.rs`)

```rust
// Pour chaque pixel RGBA avec alpha > 0 :
// 1. Dépremultiplication alpha
// 2. RGB → HSB (conversion manuelle sans appel système)
// 3. h += hue_shift; s = (s * sat_mul).clamp(0,1); b = (b + bri_off).clamp(0,1)
// 4. HSB → RGB
// 5. Rémultiplication alpha
// 6. Convertir RGBA → BGRA prémultiplié pour GDI
```

`mx = max(r,g,b)`, `mn = min(r,g,b)`, `delta = mx - mn`
- Si delta > 0.001 : calcul teinte selon canal dominant
- Saturation : `delta / mx`
- Luminosité : `mx`

## Ollama API
- **Endpoint** : `http://localhost:11434`
- **List models** : `GET /api/tags` → `json["models"][*]["name"]`
- **Chat stream** : `POST /api/chat`, body `{"model":"...", "messages":[...], "stream":true}`
- **Parsing** : chaque ligne de réponse est un objet JSON, token dans `message.content`
- **Mémoire** : 20 messages max par chat (premier message = system prompt conservé, puis `suffix(40)`)

## Persistance
- **Chemin** : `std::env::var("APPDATA") + \CATAI\config.json`
- **Mémoire chat** : `%APPDATA%\CATAI\mem_<uuid>.json`
- **Jamais le registre Windows**
- Struct `AppConfig { cats: Vec<CatConfig>, scale: f64, model: String, lang: String }`
- Struct `CatConfig { id: String, color_id: String, name: String }`

## Assets
- Dossier `assets/cute_orange_cat/` **adjacent à l'exécutable** (chemin via `std::env::current_exe()`)
- `metadata.json` parsé une seule fois au démarrage → stocké dans `AppState`
- Format metadata : `frames.animations.<anim_name>.<direction> = Vec<chemin_relatif>`
- Format rotations : `frames.rotations.<direction> = chemin_relatif`
- **Ne jamais embarquer les PNGs dans le binaire** (`include_bytes!` interdit sur les sprites)

## Conventions de code

```rust
// unsafe : uniquement dans les modules ui/ et system/ pour les appels Win32
// Tous les HWND, HBITMAP, HDC wrappés dans structs newtype avec impl Drop
// Erreurs Win32 : windows::core::Result<T> + opérateur ?
// snake_case standard Rust, constantes SCREAMING_SNAKE_CASE
// Pas de .unwrap() sauf dans main() après vérification explicite
// Arc<Mutex<AppState>> pour l'état partagé entre callbacks Win32 et closures
```

## Règles STRICTES — Ne jamais faire

- ❌ PAS de `winapi` crate — utiliser `windows = "0.62"` uniquement
- ❌ PAS de `WM_PAINT` pour les fenêtres chats — `UpdateLayeredWindow` obligatoire
- ❌ PAS de blocage du thread UI (pas de `.await`, pas de `thread::sleep`, pas d'I/O synchrone)
- ❌ PAS de stockage dans le registre Windows
- ❌ PAS de framework UI (egui, iced, tauri, relm4, etc.)
- ❌ PAS de `include_bytes!` sur les sprites PNG (368 fichiers = trop lourd)
- ❌ PAS d'accès direct aux `HWND` depuis les threads tokio — passer par `PostMessage`
- ❌ PAS de `WS_EX_APPWINDOW` sur les fenêtres chats (elles ne doivent pas apparaître dans la barre des tâches)

## Barre des tâches Windows (≠ Dock macOS)

Contrairement au Dock macOS, la barre des tâches Windows peut être sur les 4 bords.
`SHAppBarMessage(ABM_GETTASKBARPOS)` retourne un `RECT` et un edge (`ABE_BOTTOM`, `ABE_TOP`, `ABE_LEFT`, `ABE_RIGHT`).
Les chats se déplacent **le long** de ce bord, pas forcément en bas.
