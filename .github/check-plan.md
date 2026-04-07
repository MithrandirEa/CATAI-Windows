# CATAI — Plan de conversion Rust + windows-rs

## Pré-implémentation
- [x] Créer `.github/check-plan.md`
- [x] Créer `.github/copilot-instructions.md`

## Phase 1 — Squelette & fenêtre transparente
- [x] 1. `Cargo.toml` + `build.rs` (manifest DPI awareness, embed icône .ico)
- [x] 2. `src/main.rs` : boucle Win32 `GetMessage` / `TranslateMessage` / `DispatchMessage`
- [x] 3. `src/ui/layered.rs` : `CreateWindowEx(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW)` + helper `UpdateLayeredWindow`
- [x] 4. `src/cat/sprite.rs` : PNG decode (`image` crate) → HSB tinting → BGRA prémultiplié → DIB GDI
- [x] 5. ✅ Validation : `cargo run` → sprite chat visible dans fenêtre sans fond ni bord

## Phase 2 — Animation & déplacement
- [x] 6. `src/cat/animation.rs` : parse `metadata.json`, index `(CatState, direction) → Vec<chemin>`
- [x] 7. `src/cat/state.rs` : enum `CatState` + transitions aléatoires
- [x] 8. `src/system/taskbar.rs` : `SHAppBarMessage(ABM_GETTASKBARPOS)` — position + auto-hide
- [x] 9. `SetTimer` Win32 : ID 1 = render 100ms, ID 2 = behavior 1s, ID 3 = taskbar poll 5s
- [x] 10. ✅ Validation : chat qui marche le long de la barre des tâches à 10 FPS (4 px/frame)

## Phase 3 — System tray & interactions souris
- [x] 11. `src/ui/tray.rs` : `Shell_NotifyIconW(NIM_ADD)` + `TrackPopupMenu`
- [x] 12. `src/system/mouse.rs` : `SetWindowsHookEx(WH_MOUSE_LL)` — clic sur chat
- [x] 13. Drag-and-drop sur fenêtre chat : `WM_LBUTTONDOWN/MOUSEMOVE/LBUTTONUP` + `SetCapture`
- [x] 14. ✅ Validation : drag fonctionne, icône tray visible avec menu

## Phase 4 — Bulle de discussion Ollama
- [x] 15. `src/ollama/client.rs` : `reqwest` + `tokio`, `POST /api/chat` stream, parsing ligne par ligne
- [x] 16. Bridge async → UI : `tokio::mpsc` + `PostMessage(WM_APP + N)`
- [x] 17. `src/ui/chat_bubble.rs` : fenêtre pixel-art `DrawText` GDI, tokens streamés, auto-resize
- [x] 18. Gestion mémoire par chat : 20 messages max + pruning identique au Swift
- [x] 19. ✅ Validation : réponse Ollama streamée token par token dans la bulle

## Phase 5 — Réglages & persistance
- [x] 20. `src/config/mod.rs` : `CatConfig` + `AppConfig` serde_json → `%APPDATA%\CATAI\config.json`
- [x] 21. `src/ui/settings.rs` : EditBox (noms), ComboBox (modèles), Slider (0.5–3.0), boutons couleur/langue
- [x] 22. ✅ Validation : config persistée entre relances, noms/couleurs/modèle sauvegardés

## Phase 6 — Comportements avancés
- [x] 23. `src/system/windows.rs` : `EnumWindows` + `GetWindowRect` → perching fenêtre active
- [x] 24. Détection auto-hide barre des tâches : `ABM_GETAUTOHIDEBAR` (polling 5s, timer ID 3)
- [x] 25. Multi-chat : jusqu'à 6 instances `CatInstance` simultanées, couleurs distinctes
- [x] 26. ✅ Validation : comportements complets, 6 chats simultanés, perching sur fenêtre active

## Phase 7 — Packaging
- [x] 27. Profile release optimisé (`opt-level=3`, `lto=true`, `strip=true`)
- [x] 28. Script de packaging : exe + `assets/` → archive ZIP
- [x] 29. ✅ Validation : exe standalone ~5 MB, chats apparaissent au démarrage sans configuration
