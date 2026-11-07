# FileFinder — Project Plan

> Codename: `filefinder`. Final name is an open gate before remote push.
> Status: **pre-code**. Plan rev: v1 (2026-11-07).
> This file is the living plan. Iterations append to §History; sections above are replaced in place, never deleted retroactively.

A cross-platform, chromeless, GPU-rendered file browser.
Finder's job. Ghostty's feel. Honest I/O discipline.

---

## 1. Non-negotiables

1. **Cross-platform from day one:** macOS, Windows, Linux.
2. **Zero chrome:** no header, no tabs, no traffic lights — frameless window with custom drag region and per-OS resize handling.
3. **Measured snappiness:**
   - cold launch → first frame **< 50 ms**
   - directory switch **≤ 1 frame** (~16 ms)
   - every interaction **≤ 16 ms**
   - **Invariant: the UI thread never blocks on disk.** All filesystem I/O lives in the core, async.

## 2. Locked decisions

| Dimension | Decision |
|---|---|
| Hull | Rust + egui (eframe). One GPU-rendered codebase, three OSes. |
| Architecture | `crates/core` (pure, no UI deps) + `crates/app` (eframe shell). Boundary = ordinary Rust module API; no FFI anywhere. |
| Directory engine | `readdir`-only listing (no stat), viewport-first population, neighbor-dir prefetch, `notify` watcher, selection-anchored refresh (pin scroll/selection by name). |
| Theming | Token palettes (5–8 colors). `gruvbox-dark` shipped as default. `theme = auto` follows OS dark/light. In-app cycle keybind. Custom themes = palette file in config dir. |
| Peek (preview) | Right pane, in-window, `Space` toggles. **syntect**, lazily loaded; gruvbox `.tmTheme`; images via `image` crate → GPU texture. Head-of-file read; highlight only the visible window; large-file guard. |
| Launch | Last-session directory restored (dir + selection + scroll). Frequent view one keybind away. |
| Config | `~/.config/filefinder/config`, flat `key = value`. Empty file = fully usable app. Unknown keys warn, never fail. |
| Core extras | Undo journal, selection anchors, mount guardrails (never sync-stat network volumes), keymap config section, frequents view, open-with picker, optional status hairline. |

**Permanent rejects (for now):** our own search index (delegated — see §3), tabs, grid view, bookmarks, webview shells (Tauri/QuickGUI family excluded from the hull).

## 3. Search: two providers behind one `Find` trait

Local discovery changed this section in v1 (see §History it6):

1. **`fsearch` provider (macOS)** — the user's own engine at `../fsearch`
   (MIT; README reports on Chromium 509k files: name lookup 0.21 ms vs fff's
   15.4 ms; content grep 2.0 ms vs 64 ms; whole-disk index ≈ 63 MB RSS).
   Integration shape: **as an in-process Rust crate** (`fsearch::Engine`),
   no FFI; our app is a long-running host, a natural index owner.
   Verify before landing: crate builds standalone; license/attribution attested (§Gates).
2. **Portable filter (all OSes)** — incremental fuzzy filter over the loaded
   directory (`frizbee`-style SIMD scoring acceptable as a dependency), so
   Windows/Linux are never second-class while fsearch keeps its
   macOS-only fast paths (`getattrlistbulk`, FSEvents).

Content grep mode (`ext:`/`grep:`/`sym:` fsearch query language) is a v2 UI
surface; the provider already supports it.

## 4. Architecture

```
filefinder/
├── PLAN.md
├── crates/
│   ├── core/            # no egui deps — pure, testable
│   │   ├── scan/        # async dir engine: no-stat listing, prefetch, watcher
│   │   ├── model/       # entries, columns, selection state, anchors
│   │   ├── find/        # Find trait + portable filter provider (+ fsearch provider, v0.5)
│   │   ├── ops/         # open-with, rename, trash (per-OS correct), undo journal
│   │   ├── cfg/         # flat key=value parser, forgiving (warn ≠ fail)
│   │   └── session/     # last-dir/selection/scroll persistence (debounced writes)
│   └── app/             # eframe shell: borderless, drag region, palette, peek pane
└── (docs/, assets/ later)
```

## 5. Roadmap

- **v0 — useful already.** Workspace scaffold · `DirScanner` trait + in-memory fake ·
  borderless window + drag region · dir engine (no-stat, viewport-first, prefetch) ·
  list view + lazy stat columns · plain incremental filter · gruvbox-dark through
  tokens only · config file · session restore · watcher + selection anchors.
- **v0.5 —** fsearch provider (mac), frequents view, open-with picker.
- **v1 —** Peek pane (syntect + images), undo journal, keymap section,
  status hairline, mount guardrails surfaced, `theme = auto`.
- **v2 —** content grep mode (fsearch query language on mac, portable later),
  CLI/TUI sibling on the same core, tabs only if ever forgiven.

## 6. Working agreement

1. **All work is committed.** Conventional prefixes (`feat:`, `fix:`, `docs:`,
   `chore:`, `refactor:`, `test:`); subject ≤ ~72 chars; body carries the *why*
   when the diff doesn't.
2. **Local first.** No remote is configured; `origin` is added and pushed only
   when the user green-lights a converged state.
3. **Milestone reporter.** At each milestone the main agent spawns one
   background subagent whose job is read-only reporting: diff since last
   report, commit list, checks run, deviations from this plan. Updates land
   in the session for the user; ad-hoc "status?" requests spawn one immediately.
4. **This file governs.** Deviations must be edited into the plan in the same
   commit that makes the deviation.

## 7. Open gates (must clear before first remote push)

- [ ] **Name.** `filefinder` is a codename; rename affects binary name, config path, window class.
- [ ] **Route re-confirmation.** Rust+egui was chosen with these local assets visible: `../fsearch` (Rust, in-process synergy) argues for the locked route; `../gorex` (a shipped Go + MyGo frameless app, cross-platform, same egoist ecosystem as the Rex terminal the user already lives in) is the alternative-family path. Locked decision stands at Rust+egui unless re-confirmed otherwise.
- [ ] **Provenance attestation.** `../fsearch` treated as user-owned code; confirm before we ship on it.
- [ ] **Icon/font sourcing.** Ghostty-vibe = no icon chrome by default; decide whether Nerd Fonts, embedded SVGs (Lucide, used by GoRex under ISC — acceptable with attribution), or text-only.

## 8. Config sketch (target shape)

```
# ~/.config/filefinder/config
theme                = gruvbox-dark
preview              = peek
preview.syntax       = true
search               = auto        # auto | portable | fsearch
search.scope         = ~
launch               = session     # session | frequents | home
keybind              = space=peek
keybind              = cmd+f=find
keybind              = cmd+shift+f=jump-to-volume
```

## 9. History (append-only)

- **it1 — Idea phase.** TUI vs native shell debate; ghostty love (load speed, no-chrome window, minimal config) as the north-star feel. Core/front-shell split established.
- **it2 — Verification round 1.** quickgui.dev is egoist's pre-alpha GPU-renderer framework (Go/TS/Rust SDKs, macOS only today, Win/Linux marked in progress). libghostty: exposed APIs are terminal-only, C+Zig surface, *"the current libghostty API is not stable"* per its docs. Steal the architecture (portable core + thin shell), never libghostty itself.
- **it3 — Portability required → route pivot.** Per-platform native shells eliminated (Ghostty's own shells are per-platform: Swift/AppKit, GTK4). Portable custom-GPU-shell chosen. Toolkit fork resolved: egui (immediate mode, fastest iteration, effortless under-styling) over iced / GPUI / QuickGUI. FFI problem dissolved (single language).
- **it4 — Spec additions.** GruvBox + theme toggling; fff-style search (verified: engine escaped nvim as the standalone `fff-search` crate); peek with syntax highlighting; core extras list (undo journal, anchors, mounts, keymap, frequents, open-with, status hairline).
- **it5 — Forks resolved.** syntect · right-pane peek · last-session launch. v1 roadmap set.
- **it6 — Local ground truth.** Repo scan discovered `../fsearch` (user's own macOS whole-disk engine, MIT, benchmarks above), `../gorex` (GoRex: Go + MyGo frameless cross-platform terminal replica — prior art for frameless/tabs/sessions/appearance), `../quickgui/my-app` (recent QuickGUI template — evaluation, not adoption). The `Find` trait became dual-provider: fsearch on mac + portable filter everywhere. §Gates added.
