# FF — Project Plan

> Codename retired (it8): project is named **FF**. Repo dir `FF`; binary, crate and config identifier lowercase `ff`; display identity `FF`.
> Status: **v0 in progress** (engine `c1b317f`; watcher `53259b2`; waker+session+cfg `it12`; virtualization `it13`; stats `it14`). Plan rev: v6 (2026-11-07).
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
| Config | `~/.config/ff/config`, flat `key = value`. Empty file = fully usable app. Unknown keys warn, never fail. |
| Core extras | Undo journal, selection anchors, mount guardrails (never sync-stat network volumes), keymap config section, frequents view, open-with picker, optional status hairline. |

**Permanent rejects (for now):** our own search index (delegated — see §3), tabs, grid view, bookmarks, webview shells (Tauri/QuickGUI family excluded from the hull).

## 3. Search: providers behind one `Find` trait

1. **`fsearch` (macOS, vendored dependency)** — third-party MIT OSS engine at
   `../fsearch` (NOT user-owned; classified it7). Integration shape:
   **in-process Rust crate** (`fsearch::Engine`) where available — but it is
   never a hard dependency: both name search and content search degrade
   gracefully to the portable providers. README reports vs fff on Chromium
   509k files: name lookup 0.21 ms vs 15.4 ms, content grep 2.0 ms vs 64 ms.
   **Preference rule (user, it7):** fsearch beats out fff-family and portable
   providers only where *our own integrated benchmark* confirms those margins
   on our scenarios — vendor README numbers are a claim, not a gate.
2. **Portable name filter (all OSes)** — incremental fuzzy filter over the
   loaded directory (SIMD scoring dependency acceptable), so Windows/Linux
   are never second-class while fsearch keeps its macOS-only fast paths
   (`getattrlistbulk`, FSEvents).

**Content search is tiered ("keep rg" decision, it7):**

| Tier | Engine | All OSes? | Nature |
|---|---|---|---|
| instant | fsearch indexed grep (mac) | no | trigram index + FSEvents; millisecond-class |
| always-correct | **ripgrep-family libraries** (`ignore`, `grep-searcher`, `grep-regex`) | yes | unindexed, in-process — the ripgrep repo's own library crates, not the `rg` binary; no external install required |

`content = auto | rg | fsearch | off` — `auto` resolves to fsearch only where
it's verified and sound, ripgrep-family libs everywhere else.

## 4. Architecture

```
FF/
├── PLAN.md
├── crates/
│   ├── core/            # no egui deps — pure, testable
│   │   ├── scan/        # DirSource (non-blocking poll -> Listing{entries, complete})
│   │                #   + FsEngine: worker no-stat readdir, ticketed batches,
│   │                #   first-batch paint, neighbor warm cache (caps 256/16),
│   │                #   notify watcher on the live dir (canonical-mapped keys,
│   │                #   150ms quiet-window debounce; dirty invalidates only)
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

- [x] **Name.** Locked (it8): `FF` everywhere — repo dir `FF`, binary/crate/config identifier `ff` (e.g. `~/.config/ff/config`), app/window identity `FF`.
- [x] **Route re-confirmation.** **Rust + egui confirmed by user (it7).** GoRex/MyGo recorded as shipped prior art (frameless window, appearance, palette) but not chosen; the Rust-core + MyGo-shell hybrid was also considered and rejected (re-introduces a boundary GoRex deliberately avoided).
- [x] **Provenance attestation.** Resolved (it7): `fsearch` is third-party MIT OSS, not user-owned — vendored-dependency treatment, isolated behind the `Find` trait, authorship not required.
- [ ] **Performance gate.** Before fsearch becomes a default provider on macOS, run *our own* benchmark on our scenarios (method patterned after `fsearch demo/vs_fff.py`) to confirm the README margins; until it passes, portable providers are the floor everywhere.
- [ ] **Icon/font sourcing.** Ghostty-vibe = no icon chrome by default; decide whether Nerd Fonts, embedded SVGs (Lucide, used by GoRex under ISC — acceptable with attribution), or text-only.

## 8. Config sketch (target shape)

```
# ~/.config/ff/config
theme                = gruvbox-dark
preview              = peek
preview.syntax       = true
search               = auto        # auto | portable | fsearch
search.scope         = ~
content              = auto        # auto | rg | fsearch | off
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
- **it6 — Local ground truth.** Repo scan discovered `../fsearch` (macOS whole-disk engine, MIT; self-authored hypothesis corrected in it7), `../gorex` (GoRex: Go + MyGo frameless cross-platform terminal replica — prior art for frameless/tabs/sessions/appearance), `../quickgui/my-app` (recent QuickGUI template — evaluation, not adoption). The `Find` trait became dual-provider: fsearch on mac + portable filter everywhere. §Gates added.
- **it7 — Workflow bound + gates resolved.** Working agreement encoded in §6; plan landed as root commit `f21f4fd`. User confirmed **Rust + egui stays** (GoRex/MyGo = prior art, not the route; hybrid rejected). User classified `fsearch`: **third-party MIT OSS** — vendored-dep treatment behind `Find`, with an added rule: prefer fsearch over alternatives only where *our own* benchmark confirms README margins (new performance gate). **Ripgrep is kept** — as the ripgrep repo's *library crates* (`ignore`, `grep-searcher`, `grep-regex`), not the binary: portable, always-correct, zero-install content search that outranks nothing but also never lies. Content search became tiered (`content = auto | rg | fsearch | off`).
- **it8 — Named: FF; de-emojified.** Project renamed per user: repo dir `filefinder` → `FF`; binary/crate/config identifier lowercase `ff`; display identity `FF`. Directory rename executed after this same commit (git travels with the dir). All emoji removed from this plan as a standing style rule — plain-words cells in any test matrix; no icons, matching the product's no-chrome aesthetic.
- **it9 — First code landed.** v0 scaffold committed (`06c2c8c`): `ff-core` (std-only engine types, 8 tests) + `ff-app` (egui/eframe 0.37 frameless shell, gruvbox tokens, demo tree via in-memory scanner). Plan status flipped pre-code → v0 in progress; roadmap chain unchanged. Independent read-only reporter confirmed zero deviations.
- **it10 — Engine milestone.** Sync `DirScanner` replaced by `DirSource::poll -> Listing{entries, complete}`: a non-blocking contract where no syscall can occur on the frame thread. `FsEngine`: single worker doing `read_dir` + `FileType` off dirents (per-entry `metadata()` never called), first ~96 entries paint raw immediately, full sorted list arrives as a second event, tickets fence fast navigation, subdirectory warm cache (cap 256 listings / 16 dirs per warm) makes descending instant, failures surface in the status line. `EntryKind::Symlink` added; listing never follows symlinks. Shell: starts in `$HOME`, selection anchored by NAME across the partial-to-sorted swap (shared machinery for the watcher milestone), display decoupled from entries so filter + enter can't disagree. Tests caught a real design bug before it shipped: "first frame before scan starts" was indistinguishable from "complete empty listing" — `Listing.complete` now carries the authoritative-listing state explicitly. `notify` watcher = next milestone.
- **it11 — Watcher milestone.** `notify` (v8, native backends: FSEvents/inotify/ReadDirectoryChangesW) added; the entire native backend lives on the worker thread (watcher creation, canonicalization, un/watch, event consumption — zero new syscalls for the frame). Design: per-switch `Work::Watch { dir }`; the worker canonicalizes targets (macOS /var ↔ /private/var reality) and maps event-path parents back to the scan key the UI knows; dirtiness coalesces into a per-dir map flushing after a 150ms quiet window, so event storms cannot become scan storms; a watch relies on a setup-validation flush to close the switch-gap. `ScanEvent::Dirty { dir }` drops only that cache entry and forces a fresh authoritative listing on the next poll — the old list stays visible, incomplete-but-not-empty, and the NAME anchor (it10 machinery) keeps selection and scroll stable through the swap. Watch-refresh measured end-to-end sub-second in tests (12/12 green). Warm-cache staleness of non-live subdirs remains an accepted v0 simplification (they re-warm after refresh).
- **it12 — Session formalization, config loading and event waker.** Repaint stall eliminated via abstract `RepaintWaker` callback (`Arc<dyn Fn() + Send + Sync>`) in `FsEngine`, invoking `ctx.request_repaint()` on event arrival without taking UI dependencies into `crates/core`. Diagnostic logs stripped. Atomic session persistence landed (`.tmp` write + atomic rename) with graceful `$HOME` fallback and `Drop`/`on_exit` save guarantees. Config discovery wired for `~/.config/ff/config` (and platform equivalents) using `cfg::parse`, adding `show_hidden` preference and non-intrusive warning surfacing. Palette selection wired (`braun` and `gruvbox-dark`). All workspace tests pass (21/21 green).
- **it13 — List view virtualization.** Replaced per-frame string vector allocations and unvirtualized full-list traversal with `egui::ScrollArea::show_rows` zero-allocation windowing. Steady-state row render borrows filenames directly with zero string clones. Replaced `resp.scroll_to_me` with container-level `ui.scroll_to_rect` using fixed row height calculation, guaranteeing reliable scroll-into-view on session restore and keyboard jumps (ArrowUp/Down, Home/End, PageUp/Down) for off-screen unrendered rows. All workspace tests pass (21/21 green).
- **it14 — Lazy stat columns and metadata worker pipeline.** Added non-blocking off-thread file size and modification date queries without violating the readdir-only directory listing invariant (`read_entries` retains 0 metadata calls). Worker executes `Work::Stat` using `symlink_metadata` (never following symlinks) under monotonic ticket fencing. Shell maintains a bounded in-memory cache and collects missing stats for visible rows during `ScrollArea::show_rows`. Tabular row layout renders Name, human-readable Size, and civil Date columns with Braun/Gruvbox palette integration. All workspace tests pass (24/24 green).
