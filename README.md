# FF

A fast, lightweight, GPU-rendered file manager for macOS and Linux.

Finder's job. Ghostty's feel. Honest I/O discipline. Zero chrome.

---

## Features

- **Miller 3-Column Navigation**: View parent context on the left, interactive active directory in the center, and live previews on the right.
- **Git Integration**: Monospace status indicators in the listing (`M`, `A`, `?`, `D`) and inline syntax-highlighted diffs in the preview pane.
- **Command Palette Launcher (`Cmd+K` / `Cmd+P`)**: Instant fuzzy file search, in-process content grep, and quick actions in a compact HUD.
- **In-Window Peek Pane (`Space`)**: Instant preview for source code (syntax highlighting), Markdown documents, image textures, and child directories.
- **HUD Theme Picker (`Cmd+Shift+P`)**: 14 curated high-contrast palettes (Braun Alabaster, Gruvbox, Tokyo Night, Catppuccin, Nord, Solarized, and more) with real-time preview.
- **Smart App Opener**: Configurable per-extension routing (`.java` to IntelliJ, `.cpp` to CLion, `.pdf` to Preview, text/code to `nvim` in Ghostty).
- **Vim Motions**: Modal-free navigation (`h`, `j`, `k`, `l`, counts, `gg`, `G`, `0`, `$`, `Ctrl+D`).

---

## Installation

### macOS

1. Download `FF.dmg` from the [Latest Release](https://github.com/cjs675/FF/releases/latest).
2. Open the DMG and drag `FF.app` to your `/Applications` folder.
3. On first launch, right-click `FF.app` and select **Open** (or run `xattr -cr /Applications/FF.app` in Terminal).

### Linux

#### Option 1: Build and Install from Source (via Cargo)

Requires the Rust toolchain (1.80+):

```bash
cargo install --git https://github.com/cjs675/FF.git
```

#### Option 2: Clone and Build Locally

1. Install build dependencies:
   - **Ubuntu / Debian**:
     ```bash
     sudo apt-get update
     sudo apt-get install -y build-essential libfontconfig1-dev libx11-dev libxkbcommon-dev libwayland-dev
     ```
   - **Fedora**:
     ```bash
     sudo dnf install -y fontconfig-devel libX11-devel libxkbcommon-devel wayland-devel
     ```
   - **Arch Linux**:
     ```bash
     sudo pacman -S --needed base-devel fontconfig libx11 libxkbcommon wayland
     ```

2. Build release binary:
   ```bash
   git clone https://github.com/cjs675/FF.git
   cd FF
   cargo build --release
   cp target/release/ff ~/.local/bin/
   ```

---

## Keybindings

### Navigation & Motions

| Key | Action |
|---|---|
| `j` / `Down` | Move down |
| `k` / `Up` | Move up |
| `h` / `Left` | Ascend to parent directory / back |
| `l` / `Right` | Descend into folder / enter |
| `Enter` | Smart open file (or descend folder) |
| `<count>j` / `<count>k` | Jump by numeric count |
| `gg` / `0` / `Home` | Jump to top |
| `G` / `$` / `End` | Jump to bottom |
| `Ctrl+D` / `PageDown` | Half-page down |
| `Backspace` | Navigate to parent directory |

### View & Previews

| Key | Action |
|---|---|
| `Space` | Toggle Peek preview pane |
| `Tab` | Toggle between file view and Git diff in preview pane |
| `m` | Toggle Miller 3-column spatial layout |
| `Cmd+Shift+P` | Open HUD theme picker with live preview |
| `Cmd+Shift+F` / `Cmd+U` | Open Frequents view |

### Actions & Search

| Key | Action |
|---|---|
| `Cmd+K` / `Cmd+P` | Open command palette launcher (Files, Content, Actions) |
| `/` | Focus inline directory filter |
| `e` | Open highlighted file in terminal editor (`nvim`) |
| `o` | Open highlighted file with OS default application |
| `Alt+Enter` / `Cmd+O` | Open "Open With..." application chooser modal |
| `a` / `Cmd+N` | Create new file |
| `A` / `Cmd+Shift+N` | Create new folder |
| `r` / `F2` | Rename highlighted file |
| `Cmd+Backspace` / `x` | Move highlighted item to Trash |
| `u` / `Cmd+Z` | Undo last file operation |
| `y` / `Cmd+C` | Copy file path to clipboard |

---

## Configuration

Configuration is located at `~/.config/ff/config` (or `%APPDATA%\ff\config` on Windows). All settings are optional; an empty file uses all defaults.

```ini
# Theme: braun | gruvbox-dark | gruvbox-light | tokyo-night | catppuccin-mocha | nord | solarized-dark | etc.
theme          = gruvbox-dark

# Layout view: miller | flat
view           = miller

# In-window preview pane: peek | off
preview        = peek
preview.syntax = true

# Search backend: auto | portable | fsearch
search         = auto
content        = auto

# Startup directory: session | frequents | home
launch         = session
show_hidden    = false

# Per-extension app opener overrides
open.java      = IntelliJ IDEA
open.cpp       = CLion
open.pdf       = Preview
open.docx      = Preview
open.sh        = nvim
```

---

## License

MIT License. Copyright (c) 2026 Chris Silva.
