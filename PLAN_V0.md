# FF — Plan v0 Completion

> Codename retired (it8): project is named **FF**. Repo dir `FF`; binary, crate and config identifier lowercase `ff`; display identity `FF`.
> Status: **v0 in progress** (engine `c1b317f`; watcher `53259b2`; session pending). Plan rev: v0-completion (2026-11-07).
> This document specifies the immediate next milestone (it12) and all remaining milestones through v0 closure.
> Non-interference: this is a standalone planning artifact; existing PLAN.md remains unmodified.

A cross-platform, chromeless, GPU-rendered file browser.
Finder's job. Ghostty's feel. Honest I/O discipline.
The window earns its keep.

---

## 1. Non-negotiables

1. **Cross-platform from day one:** macOS, Windows, Linux.
2. **Zero chrome:** no header, no tabs, no traffic lights — frameless window with custom drag region and per-OS resize handling.
3. **Measured snappiness:**
   - cold launch to first frame **< 50 ms**
   - directory switch **<= 1 frame** (~16 ms)
   - every interaction **<= 16 ms**
   - **Invariant: the UI thread never blocks on disk.** All filesystem I/O lives in the core, async.

---

## 2. Locked decisions

| Dimension | Decision |
|---|---|
| Hull | Rust + egui (eframe). One GPU-rendered codebase, three OSes. |
| Architecture | `crates/core` (pure, no UI deps) + `crates/app` (eframe shell). Boundary = ordinary Rust module API; no FFI anywhere. |
| Directory engine | `readdir`-only listing (no stat), viewport-first population, neighbor-dir prefetch, `notify` watcher, selection-anchored refresh (pin scroll/selection by name). |
| Theming | Token palettes (5-8 colors). Braun Alabaster Linen (`#F5F4EF`) and `gruvbox-dark` (`#282828`) first-class token sets. Zero cards, 1px stone hairlines (`#E2DFD7` / `#3c3836`). |
| Typography & Icons | Embedded `Inter` and `JetBrains Mono` (SIL OFL 1.1) with system fallback font chain. Curated `Lucide` 1.5px stroke vector icons (ISC). |
| Virtualization | `ScrollArea::show_rows` virtualized list rendering; zero per-frame string allocations or clones; container-level selection scrolling. |
| Stat pipeline | Off-thread lazy stat worker (`Work::Stat`) with monotonic ticket fencing and queue draining; shallow directory file size aggregation. No sync `metadata()` calls on UI thread. |
| Open-with | In-window fuzzy modal launcher (`Cmd+O` / `Alt+Enter`) with LaunchServices app enumeration, empty-state fallback, and launch failure toasts; `Enter` opens default OS application. |
| Status hairline | 24px bottom bar with 1px stone hairline divider, 3-tier flex priority layout, shallow directory file size, git status (including detached HEAD and submodules), and tactile watcher health indicator with coalescing amber state. |
| Launch | Last-session directory restored (`cwd` + selection + scroll-to-me). Fallback to `$HOME` on missing or corrupt state. |
| Config | `~/.config/ff/config`, flat `key = value`. Empty file = fully usable app. Unknown keys warn, never fail. Built-in defaults applied; keybind syntax validated. |
| Core extras | Undo journal, selection anchors, mount guardrails (never sync-stat network volumes), keymap config section, frequents view. |

**Permanent rejects (for now):** our own search index (delegated — see §3), tabs, grid view, bookmarks, webview shells (Tauri/QuickGUI family excluded from the hull).

---

## 3. Search: providers behind one `Find` trait

1. **`fsearch` (macOS, vendored dependency)** — third-party MIT OSS engine at `../fsearch` (NOT user-owned; classified it7). Integration shape: **in-process Rust crate** (`fsearch::Engine`) where available — but it is never a hard dependency: both name search and content search degrade gracefully to the portable providers. README reports vs fff on Chromium 509k files: name lookup 0.21 ms vs 15.4 ms, content grep 2.0 ms vs 64 ms.
   **Preference rule (user, it7):** fsearch beats out fff-family and portable providers only where *our own integrated benchmark* confirms those margins on our scenarios — vendor README numbers are a claim, not a gate.
2. **Portable name filter (all OSes)** — incremental substring/fuzzy filter over the loaded directory, so Windows/Linux are never second-class while fsearch keeps its macOS-only fast paths (`getattrlistbulk`, FSEvents).

**Content search is tiered ("keep rg" decision, it7):**

| Tier | Engine | All OSes? | Nature |
|---|---|---|---|
| instant | fsearch indexed grep (mac) | no | trigram index + FSEvents; millisecond-class |
| always-correct | **ripgrep-family libraries** (`ignore`, `grep-searcher`, `grep-regex`) | yes | unindexed, in-process — the ripgrep repo's own library crates, not the `rg` binary; no external install required |

`content = auto | rg | fsearch | off` — `auto` resolves to fsearch only where it is verified and sound, ripgrep-family libs everywhere else.

---

## 4. Architecture

```
FF/
├── PLAN.md              # original project plan (v3)
├── PLAN_V0.md           # standalone v0 completion plan (it12 through it17)
├── Cargo.toml           # workspace: crates/core, crates/app (edition 2024, resolver 2)
├── crates/
│   ├── core/            # pure headless Rust, zero egui/eframe deps, testable
│   │   ├── scan/        # DirSource trait, FsEngine background worker
│   │   │   ├── mod.rs   # DirSource, Listing, MemoryScanner, sort_entries
│   │   │   └── fs.rs    # worker readdir, notify v8 watcher, quiet window, ticket fencing, queue drain
│   │   ├── model.rs     # EntryKind (Dir, File, Symlink), Entry, EntryStats
│   │   ├── find.rs      # FileFinder trait, PortableFilter
│   │   ├── ops/         # file launching, open-with picker backends, trash, undo
│   │   │   ├── mod.rs   # ops public API contracts
│   │   │   └── open.rs  # OS launch dispatch (open/xdg-open/start) + LaunchServices, error toasts
│   │   ├── cfg.rs       # flat key=value parser, show_hidden, forgiving (warn != fail)
│   │   └── session.rs   # SessionState { cwd, selection }, atomic .tmp writes
│   └── app/             # eframe/egui shell: frameless, drag region, token styling
│       ├── main.rs      # binary entry point, frameless ViewportBuilder (560x320 min)
│       ├── shell.rs     # FfShell: event polling, virtualized list, keybinds, flex status hairline
│       ├── theme.rs     # Dieter Rams Alabaster & Gruvbox token sets
│       └── assets.rs    # embedded Inter/JetBrains Mono fonts, fallback chain, Lucide icons
```

---

## 5. Current State Survey

### 5.1 Baseline Commits

- **Commit `53259b2` (Watcher milestone, it11):**
  Added `notify = "8.2.1"` native backend (FSEvents on macOS, inotify on Linux, ReadDirectoryChangesW on Windows) entirely inside the `ff-scan` worker thread. Worker canonicalizes watch targets, maps event paths back to UI scan keys, coalesces file event bursts over a 150ms quiet window (`WATCH_QUIET`), and flushes `ScanEvent::Dirty { dir }`. The UI drops the cache entry and polls fresh data while keeping old entries painted to eliminate visual flicker. Selection remains pinned by `anchor: Option<String>`.
- **Commit `b6d30ae` (HEAD):**
  Updated `PLAN.md` status line: `> Status: **v0 in progress** (engine c1b317f; watcher 53259b2). Plan rev: v3 (2026-11-07).`

### 5.2 Working Tree Modifications (Uncommitted)

The working directory contains functional, uncommitted session persistence code:
- `crates/core/src/session.rs` (untracked, 213 lines): `SessionState { cwd, selection }`, escaping for backslashes/newlines, platform path detection (`~/Library/Application Support/ff/session`, `%APPDATA%\ff\session`, `~/.local/state/ff/session`), atomic write via `.tmp` file and rename.
- `crates/core/src/scan/fs.rs`: Worker thread initializes session file on launch, emits `ScanEvent::Restored { state }`, debounces write requests via `Work::Persist { state }` over a 300ms quiet window (`PERSIST_QUIET`).
- `crates/app/src/shell.rs`: Drains `take_restored()`, sets `anchor`, requests one-shot `scroll_to_me`, debounces persistence changes, and provides exit save handling. Contains temporary debug logging statements (`eprintln!`) requiring removal.
- `crates/core/src/lib.rs`: Exports `pub mod session;`.

### 5.3 Test Suite Status

`cargo test --workspace` passes 18/18 tests in 0.40s:
- `cfg::tests`: 4 passed (defaults, enum warnings, repeatable keybinds, unknown keys).
- `find::tests`: 2 passed (empty query, case-insensitive substring).
- `scan::tests`: 2 passed (directory-first sorting, missing directory handling).
- `session::tests`: 4 passed (platform paths, fallback on empty, special characters, atomic save/load).
- `scan::fs::tests`: 6 passed (missing dir error, restore default, tree listing, hidden files, watcher refresh, worker persistence).

---

## 6. Milestone it12 — Session Formalization, Config Loading & Event Waker

Immediate next milestone to land before remaining feature work.

### 6.1 Architectural Defects Addressed

1. **Event Loop Wakeup Stall:**
   `FsEngine` delivers events to `FfShell` across a standard `std::sync::mpsc::channel`. In `eframe` / `winit`, an immediate-mode application enters low-power sleep when idle. Because `FsEngine` currently holds no waker handle, events arriving from the worker thread (initial directory read, cache warming, session restore, or watcher file notifications) do NOT wake the event loop. The UI stalls until the user moves the mouse or presses a key.
2. **Uncommitted Session Code:**
   Session persistence logic in `session.rs`, `fs.rs`, and `shell.rs` is functional and tested, but contains active debug output (`eprintln!("ff-frame-...")`, `eprintln!("ff-worker...")`) and must be cleanly landed.
3. **Unwired Config Parser & Hidden Files Setting:**
   `crates/core/src/cfg.rs` parses flat `key = value` files, but `ff-app` never loads or applies `~/.config/ff/config` on startup. Furthermore, `Config` lacks a `show_hidden` field, triggering unknown key warnings when users specify hidden file preferences.

### 6.2 Implementation Specification

#### Task 12.1: Event Loop Waker (`request_repaint` Callback)
- In `crates/core/src/scan/fs.rs`:
  Add a thread-safe waker callback to `FsEngine`:
  ```rust
  pub type RepaintWaker = Arc<dyn Fn() + Send + Sync>;
  ```
  Provide `FsEngine::with_waker(session_path: Option<PathBuf>, waker: RepaintWaker) -> Self`.
  Store `waker` in the engine worker loop. Whenever any `ScanEvent` (`Batch`, `Ready`, `Failed`, `Dirty`, `Restored`, `Stats`, `Coalescing`, `DirSize`, `Openers`) is pushed to `event_tx`, invoke `(waker)()`.
- In `crates/app/src/main.rs` & `crates/app/src/shell.rs`:
  Initialize `FsEngine` in `eframe::App` creation (`cc.egui_ctx.clone()`):
  ```rust
  let ctx = cc.egui_ctx.clone();
  let waker = Arc::new(move || ctx.request_repaint());
  let scanner = FsEngine::with_waker(session_path, waker);
  ```
- *Contract Verification:* `crates/core` retains zero dependencies on `egui` or `eframe`. All UI wake notifications occur over the abstract trait callback.

#### Task 12.2: Session Persistence Formalization
- Clean up all diagnostic logging statements in `crates/app/src/shell.rs` and `crates/core/src/scan/fs.rs`.
- Verify atomic file writing: state is written to `<session_path>.tmp` with immediate flush, followed by `std::fs::rename`.
- Verify `$HOME` fallback: if the session file does not exist, cannot be read, or contains an unparseable working directory, the engine safely defaults to `session::default_state()`.
- Wire `on_exit` frame persistence: hook `eframe::App::save` or terminal teardown so that closing the window synchronously triggers the atomic session save outside the 60fps render loop.

#### Task 12.3: Configuration Loading & Keybind Scope Reconciliation
- In `crates/core/src/cfg.rs`:
  Add `pub show_hidden: bool` (default `false`) to `Config`, parsing `show_hidden = true | false`.
- Implement configuration file discovery in `crates/app`:
  1. Check `$XDG_CONFIG_HOME/ff/config` or `~/.config/ff/config` on macOS/Linux.
  2. Check `%APPDATA%\ff\config` on Windows.
- On startup, read file contents if present (if absent, proceed with `Config::default()`).
- Call `ff_core::cfg::parse(&contents)`.
- If parsing returns warnings, store warnings in `FfShell::config_warnings` for non-intrusive display in the status hairline.
- Apply parsed parameters:
  - `theme`: Select initial token palette (`braun` or `gruvbox-dark`).
  - `search`: Set `FileFinder` provider mode (`auto`, `portable`, `fsearch`).
  - `show_hidden`: Initialize directory scan filter visibility.
  - `keybind`: Parse and validate keybind syntax into `Config.keybinds`.
- **Keybind Scope Boundary:** it12 strictly validates and stores keybinding definitions from the config file into `Config.keybinds`. The dynamic user-reconfigurable keymap dispatch engine, action execution mapping, and conflict resolution remain scoped to the v1 Keymap Configuration milestone (§12.2). This avoids premature architectural complexity while ensuring user configs are parsed and validated without error.

### 6.3 Verification Criteria
- `cargo test --workspace` passes all tests with zero warnings.
- Background directory reads and watcher change notifications repaint immediately without pointer movement.
- Quitting and relaunching `ff` restores the previous working directory and selection cursor.
- Creating `~/.config/ff/config` with `theme = braun` launches with the Braun Alabaster Linen palette.
- Setting `show_hidden = true` in config displays hidden dotfiles without parsing warnings.

---

## 7. Milestone it13 — List View Virtualization

Guarantee the strict interaction invariant (every frame <= 16 ms) regardless of directory size.

### 7.1 Bottleneck Analysis

Currently, `crates/app/src/shell.rs` contains an un-virtualized render loop:
```rust
let rows: Vec<(String, bool)> = self.display.iter().map(|e| (e.name.clone(), e.is_dir())).collect();
for (i, (name, is_dir)) in rows.iter().enumerate() {
    let resp = ui.selectable_label(selected, text);
    ...
}
```
In directories with 5,000+ files:
1. Allocates a temporary `Vec<(String, bool)>` on every frame, cloning every filename string.
2. Allocates and tessellates egui shapes for all 5,000 labels, causing frame times to exceed 50ms.

### 7.2 Implementation Specification

#### Task 13.1: Zero-Allocation Virtualization
- Replace the allocation and linear iteration with `egui::ScrollArea::vertical().show_rows`:
  ```rust
  let total_rows = self.display.len();
  let row_height = 22.0; // Fixed physical row height in points

  ScrollArea::vertical()
      .auto_shrink([false, false])
      .show_rows(ui, row_height, total_rows, |ui, row_range| {
          for i in row_range {
              let entry = &self.display[i];
              let is_selected = i == self.selection;
              ...
          }
      });
  ```
- Eliminate the per-frame `rows` vector: index directly into `self.display` by row index.
- Zero string clones: borrow `&entry.name` directly for text rendering.

#### Task 13.2: Viewport Alignment & Container Selection Scroll
- Maintain keyboard navigation responsiveness:
  When ArrowUp / ArrowDown moves `self.selection`, calculate whether the selected row index is outside the visible `row_range`.
- **Eliminate `resp.scroll_to_me`:** In egui's `ScrollArea::show_rows`, rows outside `row_range` are uninstantiated virtual rows. Calling `resp.scroll_to_me` on an off-screen row is impossible because no `Response` exists.
- Directly compute the row's target bounding rectangle from its index and invoke `ui.scroll_to_rect` on the container `Ui`:
  ```rust
  let top_y = index as f32 * row_height;
  let target_rect = egui::Rect::from_min_size(
      egui::pos2(ui.min_rect().min.x, ui.min_rect().min.y + top_y),
      egui::vec2(ui.available_width(), row_height),
  );
  ui.scroll_to_rect(target_rect, None);
  ```
- Consume one-shot `pending_scroll` on initial session restoration using this exact container-level `ui.scroll_to_rect` mechanism so that restored selections deep in large directories (e.g. index 850 in a 2,000-file directory) scroll reliably into view on frame 1 without needing rendered widget responses.

### 7.3 Verification Criteria
- Frame times in a directory with 20,000 generated files remain strictly under 8 ms during rapid scrolling.
- Zero heap allocations occurring during the steady-state rendering loop.
- Restoring a session targeting an off-screen item scrolls that item into view on frame 1 without UI glitches.

---

## 8. Milestone it14 — Lazy Stat Columns (Off-Thread Pipeline)

Add File Size and Modification Date columns without violating the zero-stat listing invariant.

### 8.1 Invariant Protection

PLAN.md §1 and §2 dictate that directory enumeration must remain `readdir`-only:
- Calling `fs::metadata()` during directory enumeration is strictly prohibited.
- Stat syscalls must occur strictly on a background worker thread.
- Only visible viewport rows may be queried for stat metadata.
- **Queue Starvation & Latency Protection:** The single background worker thread must NEVER let pending stat batches starve directory navigation. Monotonic tickets fence stat queries; incoming directory navigation immediately invalidates and drains all pending stat requests.

### 8.2 Implementation Specification

#### Task 14.1: Stat Model & Messages
- In `crates/core/src/model.rs`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct EntryStats {
      pub size: u64,
      pub mtime_secs: i64,
  }
  ```
- In `crates/core/src/scan/fs.rs`:
  Add worker requests and events with monotonic navigation tickets:
  ```rust
  pub enum Work {
      ...
      Stat { ticket: u64, paths: Vec<PathBuf> },
  }

  pub enum ScanEvent {
      ...
      Stats { ticket: u64, batch: Vec<(PathBuf, EntryStats)> },
  }
  ```

#### Task 14.2: Worker Stat Execution & Queue Starvation Defense
- Monotonic Ticket Fencing: Every `Work::Stat` carries the navigation `ticket` of the currently viewed directory.
- Worker Queue Drain & Preemption: Before processing any `Work` item from `work_rx`, the worker inspects pending channel messages. If a newer `Work::Scan` arrives:
  1. Any queued `Work::Stat` bearing an older ticket is discarded immediately without executing syscalls.
  2. Any in-flight directory size operation (`Work::StatDir`) is aborted immediately.
  3. `Work::Scan` takes priority to satisfy the `<= 16 ms` directory switch latency invariant.
- For valid, non-stale `Work::Stat` requests: worker calls `std::fs::symlink_metadata(&path)` (avoiding following symlinks), extracts byte size and unix timestamp, transmits `ScanEvent::Stats { ticket, batch }` to `event_tx`, and triggers `RepaintWaker`.
- Error tolerance: Stat errors (permission denied, vanished file) produce default/zero stats rather than failing the scan.

#### Task 14.3: Viewport Stat Caching & Invalidation in Shell
- `FfShell` maintains an in-memory stat cache with LRU eviction:
  ```rust
  stat_cache: HashMap<PathBuf, EntryStats>
  ```
  Bounded at a fixed limit (e.g., 10,000 entries) to prevent unbounded memory growth over long browsing sessions.
- Invalidation on File Modifications: Upon receiving `ScanEvent::Dirty { dir }`, `FfShell` evicts cached entries where `path.parent() == Some(&dir)` so updated file sizes and timestamps refresh cleanly.
- During `ScrollArea::show_rows`, collect paths for visible rows currently missing from `stat_cache`. Enqueue missing paths to worker via `Work::Stat { ticket: self.nav_ticket, paths }` throttled to visible row batches (~30 files).
- Tabular row layout:
  - Column 1: Name (flexible width, left-aligned).
  - Column 2: Size (e.g. `4.2 KB`, `120 MB`, right-aligned, monospaced `JetBrains Mono`).
  - Column 3: Modified (e.g. `2026-11-07 16:43`, fixed width, muted text).

### 8.3 Verification Criteria
- Navigating into a directory paints names instantly in frame 1.
- Size and date columns populate smoothly in frame 2-3 without UI stutter.
- Rapid scrolling followed by immediate directory change switches directories in <= 16 ms without waiting for queued stat batches.
- Grepping for `metadata(` in `crates/core/src/scan/fs.rs` confirms zero stat calls inside `read_entries`.

---

## 9. Milestone it15 — Open-With Picker & Default File Opener

Elevate file execution into v0: default application launch, in-window application picker, error handling, and robust cross-platform execution.

### 9.1 Background & Motivation

A file browser must be capable of launching files. The temporary comment in `shell.rs:300` (`// file open lands with the opener work (v0.5)`) leaves Enter on a file as a no-op. Bringing file launching, candidate application enumeration, and the open-with picker into v0 makes FF immediately usable as an everyday tool.

### 9.2 Implementation Specification

#### Task 15.1: Core Launcher Subsystem (`crates/core/src/ops/open.rs`)
- Create `crates/core/src/ops/open.rs` with cross-platform launch primitives:
  - Default opener:
    - macOS: `std::process::Command::new("open").arg(path).spawn()`
    - Linux: check available launcher in order: `gio launch`, `xdg-open`, with fallback to parsing `.desktop` `Exec` entries:
      `std::process::Command::new("xdg-open").arg(path).spawn()`
    - Windows: execute via `ShellExecuteW` (or `std::process::Command::new("cmd").args(["/C", "start", "", path])` passing `CREATE_NO_WINDOW` (`0x08000000`) flag via Windows `CommandExt` to eliminate black console window flashing).
  - Specific application opener:
    - macOS: `std::process::Command::new("open").args(["-a", app_name]).arg(path).spawn()`
    - Linux: `std::process::Command::new("gtk-launch").arg(app_desktop_id).arg(path).spawn()` with fallback to desktop entry binary execution.
    - Windows: `std::process::Command::new(app_path).arg(path).spawn()` with `CREATE_NO_WINDOW`.
  - Execution runs detached via `spawn()`; the UI thread never waits on child process termination.
  - Error Capture & Status Toast: Capture `spawn()` errors (e.g. missing executable, permission denied) and exit failures. When a launch fails, do not drop the error silently: transmit a notification or set a temporary error toast in `FfShell` displayed in signature Braun orange (`#E65100`) in the status hairline (e.g. `cannot open: no application registered`).

#### Task 15.2: Application Enumeration (Off-Thread Query)
- Define worker query and response types in `crates/core/src/scan/fs.rs`:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct AppOpener {
      pub name: String,
      pub id: String,
      pub is_default: bool,
  }

  pub enum Work {
      ...
      EnumerateOpeners { ticket: u64, path: PathBuf },
  }

  pub enum ScanEvent {
      ...
      Openers { ticket: u64, path: PathBuf, apps: Vec<AppOpener> },
  }
  ```
- Platform candidate querying:
  - macOS: Query CoreServices / LaunchServices in `crates/core/src/ops/open.rs`. Add `[target.'cfg(target_os = "macos")'.dependencies]` in `crates/core/Cargo.toml` linking `CoreServices` and `CoreFoundation`. Safely call `LSCopyApplicationURLsForURL` and `LSCopyDefaultApplicationURLForURL`. Enforce strict RAII memory management: wrap `CFArrayRef` and `CFURLRef` in safe drop guards (or call `CFRelease`) to prevent CoreFoundation handle leaks. Extract human-readable bundle names via `CFBundleDisplayName` or `CFBundleName`.
  - Linux: inspect MIME associations via `xdg-mime query default <mime>`, scan `/usr/share/applications` and `~/.local/share/applications` for matching `MimeType` desktop files.
  - Windows: inspect registered file extension ProgIDs in `HKEY_CLASSES_ROOT` and OpenWithList keys.
- Isolate LaunchServices enumeration behind worker query `Work::EnumerateOpeners` to prevent IPC latency from blocking frames.

#### Task 15.3: In-Window Open-With Modal Picker & Empty State
- Reject OS native file dialogs to preserve the zero-chrome aesthetic.
- Keybinds:
  - `Enter` on File: Spawns default application.
  - `Cmd+O` / `Alt+Enter` on File: Emits `Work::EnumerateOpeners` and opens in-window modal picker.
- Modal Palette UI:
  - Frameless floating overlay centered in window with 1px stone border (`#E2DFD7` / `#3c3836`) and tactile drop shadow.
  - Substring search filter at the top with auto-focus.
  - List of candidate applications with `(default)` indicator.
  - Keyboard Arrow keys navigate, Enter launches selected application and dismisses picker, Escape dismisses.
  - **Empty-State Handling:** When 0 candidate applications are found (empty `apps` vector):
    Display an explicit empty-state message inside the palette: `"No application registered for this file type"`.
    Provide a fallback option: `"Open with system chooser..."` (triggering macOS `open -R` / OS chooser dialog) so the user is never stranded with an unresponsive modal.

### 9.3 Verification Criteria
- Pressing Enter on `README.md` launches the default editor immediately.
- Pressing `Cmd+O` summons the in-window picker within 16 ms.
- Launching an unassociated binary file displays a clear error toast in the status hairline rather than failing silently.
- Opening a file with no registered applications displays `"No application registered for this file type"` with system chooser fallback.

---

## 10. Milestone it16 — Status Hairline

Replace interim debug panel with an understated 24px status hairline adhering to the Dieter Rams & Braun Minimalist System.

### 10.1 Design Standards & Principles

- **Zero Container Boxes:** No heavy card wrappers or dark glassmorphism.
- **1px Stone Hairline:** Exact 1px horizontal stone border (`#E2DFD7` in Braun Alabaster Linen; `#3c3836` in Gruvbox dark) cleanly separating CentralPanel from the status hairline.
- **Height & Spacing:** Pinned height at 24px, vertical centering, understated typography (`JetBrains Mono` 11px).
- **Responsive Flex Priorities:** Predictable degradation on compact windows down to minimum size (560px width).

### 10.2 Implementation Specification

#### Task 16.1: Asynchronous Directory Size Calculation & Bounding
- **Zero-Stat Listing Invariant:** Directory total size must NEVER be computed synchronously during navigation.
- Implement background directory size aggregation in `ff-scan` worker:
  `Work::StatDir { ticket: u64, dir: PathBuf }`
- **Shallow-Only Scoping & Bounding:**
  To prevent infinite loops on recursive symlinks and multi-second disk thrashing on deep directory trees (`node_modules`, cache directories, network mounts), `Work::StatDir` in v0 is **strictly shallow**:
  1. Worker reads direct child entries in `dir` only.
  2. For each direct child file, calls `std::fs::symlink_metadata` (never recursing into child directories, never following symlinks).
  3. Bounded iteration: worker checks navigation `ticket` every 256 entries. If a newer navigation ticket has arrived, `StatDir` immediately aborts. If entry count exceeds 10,000 files, iteration stops and reports the bounded total.
- Worker emits `ScanEvent::DirSize { ticket: u64, total_bytes: u64 }`.
- UI behavior & label formatting:
  - When switching directories, immediately paint item count (e.g. `42 items`) and size placeholder (`--`).
  - When `DirSize` arrives for the active ticket, update display to shallow byte total: e.g. `124.5 MB in files`.

#### Task 16.2: Git Status Integration & Edge Cases
- Non-blocking git discovery executed off-thread:
  - Detect git repository root: check for `.git` in current or ancestor directories.
  - **Submodule & Worktree Support (`.git` File):** If `.git` is a file rather than a directory, read its content. If it starts with `gitdir: <path>`, resolve `<path>` relative to the file's parent directory to locate the authoritative git metadata directory.
  - **Branch Name & Detached HEAD Resolution:** Read the resolved `.git/HEAD`:
    - Normal branch: If `HEAD` contains `ref: refs/heads/<branch>`, extract `<branch>` (e.g. `main` or `feat/v0`).
    - Detached HEAD: If `HEAD` does not start with `ref:`, it contains a raw 40-character commit hash (e.g. during git bisect, rebase, or detached checkout). Extract the first 7 characters and render as `HEAD (e4a3b1c)`.
  - **Fast Dirty Indicator:** Avoid running expensive full-tree git status scans. Evaluate repository status by comparing index file timestamp against working tree metadata or checking `.git/index` state. Render clean state in muted stone grey (`#8E8C96` / `#928374`) and dirty state `*` in signature Braun orange (`#E65100`) or Gruvbox yellow (`#fabd2f`).

#### Task 16.3: Watcher Health Indicator & Coalescing Signal
- Surfaced as a physical 6px tactile status dot in the bottom right corner of the status hairline.
- **Coalescing Event Signal:**
  In `crates/core/src/scan/fs.rs`, when the worker receives filesystem change notifications and adds the first event for a directory to `dirty_since` (starting the 150ms quiet window), worker immediately transmits:
  ```rust
  ScanEvent::Coalescing { dir: PathBuf }
  ```
  and triggers `RepaintWaker`.
- Three Distinct Visual States:
  - Olive green (`#16A34A` in Braun / `#b8bb26` in Gruvbox): Active watching (native OS backend listening, idle state).
  - Amber (`#D97706` in Braun / `#d79921` in Gruvbox): Coalescing changes during 150ms quiet window (`ScanEvent::Coalescing`). Transitions back to olive green as soon as the fresh listing arrives (`ScanEvent::Ready` / `Batch`).
  - Orange (`#E65100` in Braun / `#fb4934` in Gruvbox): Degraded / unwatched / permission error or watcher initialization failure.

#### Task 16.4: Hairline Segment Layout & Flex Priorities
- Pinned 24px bar with 1px stone hairline divider (`#E2DFD7` in Braun Alabaster / `#3c3836` in Gruvbox Dark).
- Window minimum inner size is set to `560.0 x 320.0` points (`crates/app/src/main.rs:13`). At 560px width, packing four full text segments risks text collision or expanding panel height beyond 24px.
- **3-Tier Flex Layout Priority Rules:**
  - **Priority 1 (Left Segment — Primary Container Stats):** Item count and shallow file size (`42 items  |  124.5 MB in files`). Always visible; fixed left alignment.
  - **Priority 2 (Right Segment — System State):** Git branch / dirty indicator and 6px watcher status dot (`main *  [dot]`). Always visible; fixed right alignment.
  - **Priority 3 (Center Segment — Selected Item Inspection):** Selected item details (`cargo.lock  ·  14.2 KB  ·  16:43`). Flexible width with middle-truncation for long filenames (`very_long_component_nam...spec.tsx`). Responsive degradation: if available window width is < 600px, drop timestamp and file size, showing filename only; if available width is < 500px, hide center segment entirely so the hairline never wraps or exceeds 24px.
```
[ 1px stone divider #E2DFD7 / #3c3836 ]
42 items | 124.5 MB in files    cargo.lock · 14.2 KB · 16:43    main *  [green dot]
(Priority 1: left stats)         (Priority 3: center inspect)    (Priority 2: right state)
```

### 10.3 Verification Criteria
- Bottom panel renders at exactly 24px with 1px stone divider line across all window dimensions down to 560px.
- Changing directories shows instant item count; directory shallow size populates asynchronously without frame drops.
- Modifying a file in a watched directory immediately turns the status dot amber for 150ms before returning to green on refresh.
- Navigating into a git repository in detached HEAD cleanly renders `HEAD (short-sha)` without panel overflow.
- Navigating into a git submodule or worktree resolves the branch correctly from the `.git` file pointer.

---

## 11. Milestone it17 — Design Tokens, Embedded Assets & v0 Completion Gate

Embed typography and icon assets, reconcile theme tokens, and verify v0 performance gates.

### 11.1 Design Tokens & Theme Reconciliation

Support two first-class token sets selectable via `theme = braun | gruvbox-dark` in `~/.config/ff/config`:

| Token | Braun Minimalist Alabaster | Gruvbox Dark | Purpose |
|---|---|---|---|
| `BG` | `#F5F4EF` (Alabaster Linen) | `#282828` (Dark 0) | Window canvas / root background |
| `SURFACE` | `#FFFFFF` (Crisp White) | `#3c3836` (Dark 1) | Floating picker surfaces, text edit |
| `BORDER` | `#E2DFD7` (Stone Hairline) | `#3c3836` (Hairline) | 1px dividers, zero heavy borders |
| `HOVER` | `#EAE8E1` (Muted Warm Wash) | `#3c3836` (Hover Wash) | Row hover state |
| `SEL` | `#E0DCD3` (Tactile Depth) | `#504945` (Dark 2) | Active selection row |
| `FG` | `#1A1A1E` (Deep Charcoal) | `#ebdbb2` (Light 1) | Primary text, filenames |
| `MUTED` | `#5C5B64` (Warm Grey) | `#928374` (Muted Grey) | Secondary text, sizes, timestamps |
| `LABEL` | `#8E8C96` (Subtle Grey) | `#7c6f64` (Dark 4) | Inactive labels, path breadcrumbs |
| `ACCENT` | `#E65100` (Braun Warm Orange) | `#fabd2f` (Gruvbox Yellow) | Directory differentiation, focus |
| `GREEN` | `#16A34A` (Tactile Olive Green) | `#b8bb26` (Gruvbox Green) | Watcher active, git clean |
| `AMBER` | `#D97706` (Tactile Amber) | `#d79921` (Gruvbox Amber) | Debouncing quiet window |

Tactile physical feedback on interactive buttons and rows:
- 6px corner radius (`rounded-md`).
- Controlled light drop shadow: `0 1px 3px rgba(0, 0, 0, 0.06), 0 1px 2px rgba(0, 0, 0, 0.04)`.
- Active press depth: `transform: translateY(1px)`.

### 11.2 Embedded Assets & Font Fallback Chain (Zero System Dependencies)

1. **Embedded Typography & CJK Fallbacks:**
   Bundle fonts via `include_bytes!` into the binary:
   - `Inter-Regular.ttf` & `Inter-SemiBold.ttf` (SIL Open Font License 1.1) for body text and navigation.
   - `JetBrainsMono-Regular.ttf` (SIL Open Font License 1.1) for tabular sizes, dates, and status hairlines.
   - Register into `egui::FontDefinitions` on startup. Zero OS font installation dependencies.
   - **System Fallback Font Chain:** Configure `egui::FontDefinitions` so that `Inter` and `JetBrains Mono` head the font families, while preserving egui's default fallback font chain and registering platform system fallback font paths (e.g. PingFang, Hiragino Sans, Meiryo, Noto Sans). This ensures Japanese, Chinese, Cyrillic, or Arabic filenames render cleanly without displaying missing glyph boxes (`□` / tofu).
2. **Curated Vector Icons:**
   Bundle curated vector icons from `Lucide` (ISC License, 1.5px stroke weight):
   - `folder`, `file`, `file-text`, `file-code`, `git-branch`, `external-link`, `alert-circle`, `check`.
   - Render directly via egui painter paths or cached alpha glyphs.

### 11.3 v0 Acceptance & Performance Gate Verification

Before declaring v0 complete, execute formal validation:

| Verification Metric | Target Threshold | Measurement Method | Gate Status |
|---|---|---|---|
| Cold launch to first frame | < 50 ms | `ff` launch benchmark on release binary | Mandatory |
| Directory switch latency | <= 16 ms (1 frame) | Frame timer during directory switch on 10,000 files | Mandatory |
| List scroll frame budget | <= 16 ms (60+ fps) | Continuous virtualized scroll frame timer | Mandatory |
| UI thread disk syscalls | 0 blocking calls | Code audit & static verification of `crates/app` | Mandatory |
| Memory footprint | < 80 MB RSS | Peak memory under 256 cached directory listings | Mandatory |
| Test suite | 100% green | `cargo test --workspace` | Mandatory |

---

## 12. Forward Roadmap (v0.5, v1, and v2)

### 12.1 v0.5 — Mac Native Acceleration & Frequents
- **`fsearch` Provider for macOS:** In-process Rust crate (`fsearch::Engine`) integrated behind the `FileFinder` trait once local benchmarks substantiate README margins.
- **Frequents View:** Frecency (frequency + recency) ranking for directories, accessible via dedicated shortcut (`launch = frequents`).
- **Advanced Open-With:** File association defaults management and custom app arguments.

### 12.2 v1 — Full Visual Workspace & Safety
- **Peek Preview Pane:** In-window right-hand preview panel toggled via `Space`. Syntax highlighting for code via `syntect` lazily loaded with matching `.tmTheme`. Image previews via `image` crate uploaded to GPU texture. Head-of-file streaming, visible-window-only highlighting, and large-file safety guard.
- **Undo Journal:** Operation journal in `crates/core/src/ops/` tracking renames, moves, and deletions with per-OS correct trash handling (macOS `trash` / NSFileManager, Windows Recycle Bin, Linux FreeDesktop Trash spec).
- **Keymap Configuration:** Dynamic user-defined keymap execution engine in `~/.config/ff/config` (`keybind = combo=action`). Custom action dispatch, key sequence remapping, and key conflict resolution. (it12 handles config parsing and syntax validation only).
- **Surfaced Mount Guardrails:** Automatic detection of network filesystems (NFS, SMB, AFP, FUSE) to prevent aggressive prefetching or sync-stat stalls.
- **Theme Auto:** Dynamic theme switching following OS dark/light mode appearance.

### 12.3 v2 — Content Search & Multi-Surface
- **Content Grep Mode:** Tiered content search using `fsearch` query syntax (`ext:`, `grep:`, `sym:`) on macOS; portable in-process ripgrep library crates (`ignore`, `grep-searcher`, `grep-regex`) across all platforms.
- **Headless / CLI / TUI Sibling:** Terminal interface sharing the identical `crates/core` engine.
- **Tabs:** Permitted only if user requirement overrides current permanent rejection.

---

## 13. Open Architectural Decisions & User Questions

The following open architectural trade-offs and questions are surfaced for user resolution:

1. **`fsearch` Benchmark Validation Gate (PLAN §7):**
   *Question:* What scenario and directory scale should define the authoritative benchmark gate before enabling `fsearch` on macOS?
   *Recommendation:* Replicate `fsearch/demo/vs_fff.py` against a local repository containing >= 500,000 files (e.g. local Chromium or Linux checkout). Require `fsearch` to demonstrate >= 5x speedup over `PortableFilter` before promoting to default. Until then, `PortableFilter` remains the default floor.
2. **Font & Icon Sourcing Strategy:**
   *Question:* Should FF embed vector icons (Lucide 1.5px stroke), rely on system Nerd Fonts, or remain strictly text-only?
   *Recommendation:* Embed a minimal curated set of 8 vector icons from Lucide (ISC license, matching GoRex prior art) and bundle `Inter` + `JetBrains Mono` (SIL OFL 1.1) with system fallback chains. This guarantees identical cross-platform visual precision without relying on host fonts.
3. **Theme Reconciliation (Dieter Rams Alabaster vs Gruvbox):**
   *Question:* Should the default shipped theme be Dieter Rams Alabaster Linen (`#F5F4EF`) or Gruvbox Dark (`#282828`)?
   *Recommendation:* Provide both as first-class token sets in `theme.rs`. Ship `theme = braun` as the default light experience and `theme = gruvbox-dark` as the default dark experience, supporting `theme = auto` to track the host OS dark/light setting.
4. **Stat Pipeline Architecture & Queue Starvation Defense:**
   *Question:* Should lazy stat queries share the single `ff-scan` worker thread, or should a secondary thread pool handle stats?
   *Recommendation:* Retain a single worker thread for v0, but mandate request ticket invalidation and worker queue draining. All `Work::Stat` and `Work::StatDir` requests carry the active navigation ticket. When a user navigates to a new directory, the new `Work::Scan` carries an incremented ticket. The worker loop drains pending channel items before executing work, discarding any queued `Work::Stat` with an older ticket and immediately aborting in-flight `Work::StatDir` iterations. This guarantees that directory switch latency remains strictly `<= 16 ms` without thread synchronization bugs, while keeping off-thread stat queries completely non-blocking.
5. **Open-With Presentation & Failure Handling:**
   *Question:* Should the open-with picker use an in-window modal palette or summon the OS-native application picker dialog?
   *Recommendation:* Use an in-window modal palette layer. An in-window palette conforms to the zero-chrome philosophy, operates with instant keyboard filter responsiveness, and avoids OS dialog visual disruption. Surface an explicit empty state (`"No application registered for this file type"`) when 0 candidate applications exist, provide an OS chooser fallback action, and report process launch errors via non-intrusive status hairline toasts.
6. **Config Live Reload:**
   *Question:* Should changes to `~/.config/ff/config` be monitored live via the watcher, or read strictly at launch?
   *Recommendation:* For v0, load configuration at startup. In v0.5, register `~/.config/ff/config` with `notify` to support seamless runtime reloading of themes and keybinds.

---

## 14. Milestone Progression History

- **it1 — Idea phase.** TUI vs native shell debate; ghostty love (load speed, no-chrome window, minimal config) as the north-star feel. Core/front-shell split established.
- **it2 — Verification round 1.** quickgui.dev is egoist's pre-alpha GPU-renderer framework (Go/TS/Rust SDKs, macOS only today, Win/Linux marked in progress). libghostty: exposed APIs are terminal-only, C+Zig surface, "the current libghostty API is not stable" per its docs. Steal the architecture (portable core + thin shell), never libghostty itself.
- **it3 — Portability required -> route pivot.** Per-platform native shells eliminated (Ghostty's own shells are per-platform: Swift/AppKit, GTK4). Portable custom-GPU-shell chosen. Toolkit fork resolved: egui (immediate mode, fastest iteration, effortless under-styling) over iced / GPUI / QuickGUI. FFI problem dissolved (single language).
- **it4 — Spec additions.** GruvBox + theme toggling; fff-style search (verified: engine escaped nvim as the standalone `fff-search` crate); peek with syntax highlighting; core extras list (undo journal, anchors, mounts, keymap, frequents, open-with, status hairline).
- **it5 — Forks resolved.** syntect, right-pane peek, last-session launch. v1 roadmap set.
- **it6 — Local ground truth.** Repo scan discovered `../fsearch` (macOS whole-disk engine, MIT; self-authored hypothesis corrected in it7), `../gorex` (GoRex: Go + MyGo frameless cross-platform terminal replica — prior art for frameless/tabs/sessions/appearance), `../quickgui/my-app` (recent QuickGUI template — evaluation, not adoption). The `Find` trait became dual-provider: fsearch on mac + portable filter everywhere. §Gates added.
- **it7 — Workflow bound + gates resolved.** Working agreement encoded in §6; plan landed as root commit `f21f4fd`. User confirmed **Rust + egui stays** (GoRex/MyGo = prior art, not the route; hybrid rejected). User classified `fsearch`: **third-party MIT OSS** — vendored-dep treatment behind `Find`, with an added rule: prefer fsearch over alternatives only where *our own* benchmark confirms README margins (new performance gate). **Ripgrep is kept** — as the ripgrep repo's *library crates* (`ignore`, `grep-searcher`, `grep-regex`), not the binary: portable, always-correct, zero-install content search that outranks nothing but also never lies. Content search became tiered (`content = auto | rg | fsearch | off`).
- **it8 — Named: FF; de-emojified.** Project renamed per user: repo dir `filefinder` -> `FF`; binary/crate/config identifier lowercase `ff`; display identity `FF`. Directory rename executed after this same commit (git travels with the dir). All emoji removed from this plan as a standing style rule — plain-words cells in any test matrix; no icons, matching the product's no-chrome aesthetic.
- **it9 — First code landed.** v0 scaffold committed (`06c2c8c`): `ff-core` (std-only engine types, 8 tests) + `ff-app` (egui/eframe 0.37 frameless shell, gruvbox tokens, demo tree via in-memory scanner). Plan status flipped pre-code -> v0 in progress; roadmap chain unchanged. Independent read-only reporter confirmed zero deviations.
- **it10 — Engine milestone.** Sync `DirScanner` replaced by `DirSource::poll -> Listing{entries, complete}`: a non-blocking contract where no syscall can occur on the frame thread. `FsEngine`: single worker doing `read_dir` + `FileType` off dirents (per-entry `metadata()` never called), first ~96 entries paint raw immediately, full sorted list arrives as a second event, tickets fence fast navigation, subdirectory warm cache (cap 256 listings / 16 dirs per warm) makes descending instant, failures surface in the status line. `EntryKind::Symlink` added; listing never follows symlinks. Shell: starts in `$HOME`, selection anchored by NAME across the partial-to-sorted swap (shared machinery for the watcher milestone), display decoupled from entries so filter + enter can't disagree. Tests caught a real design bug before it shipped: "first frame before scan starts" was indistinguishable from "complete empty listing" — `Listing.complete` now carries the authoritative-listing state explicitly. `notify` watcher = next milestone.
- **it11 — Watcher milestone.** `notify` (v8, native backends: FSEvents/inotify/ReadDirectoryChangesW) added; the entire native backend lives on the worker thread (watcher creation, canonicalization, un/watch, event consumption — zero new syscalls for the frame). Design: per-switch `Work::Watch { dir }`; the worker canonicalizes targets (macOS /var <-> /private/var reality) and maps event-path parents back to the scan key the UI knows; dirtiness coalesces into a per-dir map flushing after a 150ms quiet window, so event storms cannot become scan storms; a watch relies on a setup-validation flush to close the switch-gap. `ScanEvent::Dirty { dir }` drops only that cache entry and forces a fresh authoritative listing on the next poll — the old list stays visible, incomplete-but-not-empty, and the NAME anchor (it10 machinery) keeps selection and scroll stable through the swap. Watch-refresh measured end-to-end sub-second in tests (12/12 green). Warm-cache staleness of non-live subdirs remains an accepted v0 simplification (they re-warm after refresh).
- **it12 — Planned: Session, Config & Event Waker.** Formalize and land session persistence (remove debug logging, atomic `.tmp` rename, `$HOME` fallback, exit frame persistence). Wire runtime configuration file loading (`~/.config/ff/config`) via `cfg::parse`, add `show_hidden` field, and validate keybind syntax (dynamic remapping engine deferred to v1). Eliminate UI repaint stall bug by providing an `Arc<dyn Fn() + Send + Sync>` callback to `FsEngine` calling `ctx.request_repaint()` on event arrival.
- **it13 — Planned: List Virtualization.** Implement `egui::ScrollArea::show_rows` to enforce strict <= 16ms frame budget across 20,000+ item directories. Eliminate per-frame string cloning and heap allocations. Enforce container-level `ui.scroll_to_rect` selection scrolling using row index and height, eliminating invalid `resp.scroll_to_me` on virtualized off-screen rows.
- **it14 — Planned: Lazy Stat Columns.** Add off-thread `Work::Stat` worker pipeline to lazily query file size and modification time for visible viewport rows. Maintain zero-stat directory listing invariant. Defend against worker queue starvation via monotonic ticket fencing and queue draining, discarding obsolete stat requests upon directory navigation. Manage in-memory shell stat cache with LRU eviction and watcher dirty invalidation.
- **it15 — Planned: Open-With Picker & Default Opener.** Implement detached cross-platform file execution (`crates/core/src/ops/open.rs`) with `gio launch` / `xdg-open` fallbacks and Windows `CREATE_NO_WINDOW` console suppression. Query macOS LaunchServices (`LSCopyApplicationURLsForURL`) for application candidates via off-thread `Work::EnumerateOpeners` with RAII memory management. Provide in-window modal fuzzy picker on `Cmd+O` / `Alt+Enter` with explicit empty state handling (`"No application registered for this file type"`) and system chooser fallback; surface launch failures via status hairline toasts; open default app on `Enter`.
- **it16 — Planned: Status Hairline.** Replace debug bottom panel with minimal 24px bar and 1px stone hairline divider (`#E2DFD7` / `#3c3836`). Add off-thread shallow directory file size calculation with bounded iteration and ticket cancellation. Integrate git branch, detached HEAD short-SHA parsing, submodule `.git` file resolution, and fast dirty indicator. Implement 3-tier flex layout priorities down to 560px window width. Add physical watcher health status dot with `ScanEvent::Coalescing` event signal for tactile amber debounce state (`#16A34A` olive, `#D97706` amber, `#E65100` orange).
- **it17 — Planned: Tokens, Embedded Assets & v0 Completion Gate.** Harmonize Dieter Rams Alabaster Linen and Gruvbox Dark token palettes. Embed `Inter` and `JetBrains Mono` fonts (SIL OFL 1.1) with CJK/non-Latin system fallback chains, and curated `Lucide` 1.5px stroke icons (ISC). Verify cold launch < 50ms, directory switch <= 16ms, zero UI thread blocking I/O, and 100% green test suite.
