# CATAI

Virtual desktop pet cats for macOS — pixel art cats that live on your dock and chat with you via Ollama LLM.

![Swift](https://img.shields.io/badge/Swift-native-orange) ![macOS](https://img.shields.io/badge/macOS-14%2B-blue) ![Ollama](https://img.shields.io/badge/Ollama-LLM-green)

![CATAI Screenshot](screenshot.jpeg)

## Features

- **Dock companion** — Cats walk along your dock with pixel-perfect animations
- **Window perching** — When dock auto-hides, cats teleport to sit on top of your active window
- **Multi-cat** — Up to 6 cats with distinct colors and personalities
- **AI chat** — Click a cat to open a pixel-art chat bubble, powered by [Ollama](https://ollama.ai)
- **Random meows** — Cats spontaneously say "Miaou~", "Prrr...", "Mrrp!" in cute speech bubbles
- **Pixel art UI** — Settings panel, chat bubbles, and controls all in retro pixel style
- **Menu bar icon** — 🐱 icon with quick access to settings and quit
- **Retina ready** — Nearest-neighbor scaling keeps pixel art crisp on HiDPI displays
- **Multilingual** — French, English, Spanish (switch with flag buttons)

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

Each cat has 368 hand-drawn sprites across 8 directions:

- **Walking** — 8 frames per direction
- **Eating** — 11 frames per direction
- **Drinking** — 8 frames per direction
- **Angry** — 9 frames per direction
- **Waking up** — 9 frames per direction
- **Idle / Sleeping** — Static rotation sprites

## Requirements

- macOS 14+ (Apple Silicon or Intel)
- [Ollama](https://ollama.ai) running locally (for chat feature, optional)

## Build & Run

```bash
# Compile (one command)
swiftc -O -o cat cat.swift -framework AppKit -framework Foundation

# Run
./cat
```

No Xcode project, no dependencies, no package manager — just one Swift file.

## Settings

Click the 🐱 menu bar icon → Settings:

- **Language** — 🇫🇷 🇬🇧 🇪🇸 click a flag to switch
- **Cats** — Click a color bubble to add a cat, click × to remove
- **Name** — Rename each cat
- **Size** — Pixel art slider to scale cats
- **Ollama model** — Select from your installed models

## How It Works

- Single native Swift file (~1500 lines), no external dependencies
- `NSWindow` with transparent background for overlay rendering
- `CGWindowListCopyWindowInfo` for detecting frontmost windows
- Dock auto-hide detection via mouse position polling at 30 FPS
- Color tinting via direct pixel manipulation in sRGB `CGContext`
- Ollama streaming chat via `URLSessionDataDelegate`
- Conversation memory persisted in `UserDefaults`

## Project Structure

```
.
├── cat.swift              # Entire application (single file)
├── cat                    # Compiled binary
└── cute_orange_cat/       # Sprite assets
    ├── metadata.json      # Animation & rotation definitions
    ├── rotations/         # 8 static direction sprites (68x68 PNG)
    └── animations/        # 5 animations × 8 directions × 8-11 frames
        ├── angry/
        ├── drinking/
        ├── eating/
        ├── running-8-frames/
        └── waking-getting-up/
```

## License

MIT

---
