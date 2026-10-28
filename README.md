# nimblenote-rs

A native Windows take on [nimblenote](https://nimblenote.app): simple, keyboard-driven note taking, written in Rust (egui). One small `.exe`, no runtime, no installer.

## How it works

One input box does both search and create.

- **Create a note:** type a name that doesn't exist yet and press **Enter**. Focus jumps to the editor.
- **Open a note:** start typing; matches (by name, fuzzy name, or content) appear. **Up/Down** to pick, **Enter** to edit.
- **Esc** at any time returns to the search box (press again to clear it).
- Notes **autosave** as you type (and when the window loses focus or closes).

## Markdown

- Highlighting for headings, **bold**, *italic*, `code`, fenced code blocks, quotes, lists, checklists, links and `[[note links]]`.
- **Markdown actions** — **Ctrl+Enter** in the editor:
  - on a `- [ ]` line → toggles the checkbox
  - on a link / URL → opens it in your browser
  - on `[[Another note]]` (or a relative `[text](Another note.md)` link) → jumps to that note, creating it if needed
- **Enter** inside a list or checklist continues it; Enter on an empty item ends the list.

## Shortcuts

| Key | Action |
|---|---|
| Enter | Open selected note / create |
| Ctrl+Enter (in search) | Always create a note with the typed name |
| Up / Down, PgUp / PgDn | Move through results |
| Esc, Ctrl+L, Ctrl+K | Back to search |
| Ctrl+N | New note (clears search) |
| Ctrl+Enter (in editor) | Markdown action |
| F2 | Rename note |
| Ctrl+Shift+Delete | Delete note (to Recycle Bin) |
| Ctrl+Shift+T | Cycle theme: system → opposite of system → light → dark |
| Ctrl+Shift+O | Open the notes folder in Explorer |
| Ctrl + / Ctrl − | Zoom |
| Ctrl+, | Settings (notes folder, theme, font size, monospace editor) |
| F1 | Shortcut help |

## Storage

Each note is a plain `Name.md` file in `Documents\nimblenote` (changeable in Settings). Edit them with any other tool — the app picks up changes automatically. Settings live in `%APPDATA%\nimblenote-rs\config.json`.

## Build

1. Install Rust from <https://rustup.rs> (accept the Visual Studio C++ build tools it offers).
2. In this folder run `build-windows.bat` (or `cargo build --release`).
3. The app is `target\release\nimblenote.exe` — copy it anywhere and run it.

Or push the folder to GitHub: the included workflow (`.github/workflows/windows.yml`) builds the `.exe` and attaches it to the run as an artifact.

Run the tests with `cargo test`.

## Code layout

- `src/main.rs` – window setup, Windows fonts (Segoe UI / Cascadia), icon
- `src/app.rs` – UI, keyboard handling, autosave, dialogs
- `src/store.rs` – note files, ranked search, rename/delete
- `src/markdown.rs` – highlighter and markdown actions
- `src/config.rs` – settings
