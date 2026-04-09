# CATAI — Windows Port

> **Windows port of [CATAI](https://github.com/wil-pe/CATAI) — pixel art desktop cats that live on your taskbar and chat via Ollama.**
>
> The original project, concept, pixel art sprites, and animations are the work of **[wil-pe](https://github.com/wil-pe)**.
> This repository is a full rewrite in **Rust + windows-rs**, targeting Windows 10/11.

![Rust](https://img.shields.io/badge/Rust-stable-orange) ![Windows](https://img.shields.io/badge/Windows-10%2B-blue) ![Ollama](https://img.shields.io/badge/Ollama-LLM-green)

## What is CATAI?

Virtual desktop pet cats that walk along your Windows taskbar, sit on top of active windows, and chat with you through a local LLM ([Ollama](https://ollama.ai)).

## Features

- **Taskbar companion** — Cats walk along your taskbar (bottom, top, left or right side)
- **Window perching** — Cats sit on top of your active window when the taskbar auto-hides
- **Multi-cat** — Up to 6 cats with distinct colors and personalities
- **AI chat** — Click a cat to open a pixel-art chat bubble, powered by [Ollama](https://ollama.ai)
- **Random meows** — Cats spontaneously say "Miaou~", "Prrr...", "Mrrp!" in speech bubbles
- **System tray icon** — Quick access to settings and quit
- **Multilingual** — French, English, Spanish

## Cat Personalities

| Color | Default Name | Personality | Skill |
|-------|-------------|-------------|-------|
| 🟠 Orange | Citrouille | Playful & mischievous | Jokes & puns |
| ⚫ Black | Ombre | Mysterious & philosophical | Deep questions |
| ⚪ White | Neige | Elegant & poetic | Poetry & grace |
| 🔘 Grey | Einstein | Wise & scholarly | Science facts |
| 🟤 Brown | Indiana | Adventurous storyteller | Epic tales |
| 🟡 Cream | Caramel | Cuddly & comforting | Emotional support |

## Animations

Each cat has 368 AI-generated sprites across 8 directions:

- **Walking** — 8 frames per direction
- **Eating** — 11 frames per direction
- **Drinking** — 8 frames per direction
- **Angry** — 9 frames per direction
- **Waking up** — 9 frames per direction
- **Idle** — Static rotation sprites

## Requirements

- Windows 10 or 11 (x64)
- [Ollama](https://ollama.ai) running locally (optional, for chat feature)

## Build & Run

### Download pre-built release

Grab `CATAI-x.x.x.zip` from [Releases](https://github.com/wil-pe/CATAI/releases), unzip, then run `catai.exe`.

The `cute_orange_cat/` folder must stay **next to** `catai.exe`.

### Build from source

```powershell
# Requires Rust stable (x86_64-pc-windows-msvc)
cargo build --release
```

Output: `target\release\catai.exe`

### Package (exe + assets → ZIP)

```powershell
.\package.ps1 -Version "1.0.0"
# Produces: dist\CATAI-1.0.0.zip
```

## Settings

Click the 🐱 system tray icon → Settings:

- **Language** — FR / EN / ES buttons (labels update to the chosen language)
- **Cats** — Add (max 6) or remove cats from the list
- **Name** — Rename each cat (max 64 characters)
- **Color** — Choose from 6 colors shown with their full localized name
- **Size** — Slider to scale cats (0.5× – 3.0×)
- **Ollama model** — Type a model name directly, or click **Fetch** to retrieve the list of installed models from Ollama and select one from the listbox
- **Clear memory** — Erases the selected cat's conversation history (file + RAM)

## How It Works

- Native Win32 application, no UI framework
- `WS_EX_LAYERED` transparent windows rendered via `UpdateLayeredWindow`
- Per-pixel BGRA premultiplied bitmaps rendered with GDI `CreateDIBSection`
- HSB color tinting applied directly on sprite pixels at load time
- PNG sprites decoded asynchronously in a tokio thread-pool — no UI-thread freeze during animation transitions
- Ollama streaming chat via a persistent `reqwest::Client` + `tokio` (connection pool reused across all requests)
- Ollama base URL validated at call time — only `localhost` / `127.0.0.1` / `::1` are accepted
- Conversation memory (max 20 message pairs) persisted as JSON in `%APPDATA%\CATAI\`

## Project Structure

```
.
├── src/
│   ├── main.rs            # Win32 message loop + tokio runtime
│   ├── app.rs             # Global AppState
│   ├── cat/               # CatInstance, state machine, animation, sprite
│   ├── ui/                # Layered windows, tray, chat bubble, settings
│   ├── system/            # Taskbar detection, window perching, mouse hook
│   ├── ollama/            # Streaming HTTP client
│   └── config/            # Persistence (JSON) + l10n
├── cute_orange_cat/        # Sprite assets (from the original project)
│   ├── metadata.json
│   ├── rotations/
│   └── animations/
└── package.ps1             # Packaging script
```

## Credits

Sprites, animations, original concept and macOS implementation by **[wil-pe](https://github.com/wil-pe)** — [original CATAI project](https://github.com/wil-pe/CATAI).

## License

MIT

---
