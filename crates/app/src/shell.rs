//! The borderless shell, on the real engine (PLAN §5 engine milestone).
//! The frame's only contact with the filesystem is the non-blocking drain
//! inside `scanner.poll` — syscalls are structurally off the UI thread.
//! The selection is anchored by NAME across listing refreshes (raw first
//! batch to sorted completion), which is the machinery the watcher reuses
//! and session restore too: the last dir + selection come back on relaunch,
//! delegated through the worker (reads and debounced writes both).

use std::path::PathBuf;
use std::sync::Arc;

use egui::{CentralPanel, Key, Panel, RichText, ScrollArea, TextEdit, Ui, ViewportCommand};
use ff_core::cfg::{self, Config};
use ff_core::find::{FileFinder, PortableFilter};
use ff_core::model::Entry;
use ff_core::scan::{DirSource, FsEngine};
use ff_core::session::{self, SessionState};

use crate::theme::{self, Palette};

pub struct FfShell {
    #[allow(dead_code)]
    pub config: Config,
    pub config_warnings: Vec<String>,
    pub palette: Palette,
    scanner: FsEngine,
    finder: PortableFilter,
    cwd: PathBuf,
    /// Full listing for `cwd` as delivered by the engine.
    entries: Vec<Entry>,
    /// Filtered view — the one the UI renders; selection indexes into this.
    display: Vec<Entry>,
    /// Selection pinned by NAME across refreshes (partial-to-sorted included).
    anchor: Option<String>,
    filter: String,
    selection: usize,
    /// Negated engine `complete`: true while loading/streaming.
    listing_loading: bool,
    filter_focus_request: bool,
    filter_focused: bool,
    pointer_over_interactive: bool,
    /// Where the session file lives (`None` = persistence disabled).
    session_path: Option<PathBuf>,
    /// Last state we handed the engine for persisting (dedupe per frame).
    last_saved: Option<SessionState>,
    /// One-shot request to scroll the restored selection into view.
    pending_scroll: bool,
}

impl FfShell {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (config, config_warnings) = match cfg::default_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(contents) => cfg::parse(&contents),
            None => (Config::default(), Vec::new()),
        };

        let palette = theme::palette_for(&config.theme);
        theme::apply(&cc.egui_ctx, palette);

        // Placeholder cwd (`""`): the shell parks at an empty in-memory dir
        // until the worker delivers the restored (or default home) state —
        // including last-session selection, anchored by name.
        let session_path = session::default_path();

        let ctx = cc.egui_ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        let scanner = FsEngine::launch_with_waker(config.show_hidden, session_path.clone(), Some(waker));

        Self {
            config,
            config_warnings,
            palette,
            scanner,
            finder: PortableFilter,
            cwd: PathBuf::new(),
            entries: Vec::new(),
            display: Vec::new(),
            anchor: None,
            filter: String::new(),
            selection: 0,
            listing_loading: true,
            filter_focus_request: false,
            filter_focused: false,
            pointer_over_interactive: false,
            session_path,
            last_saved: None,
            pending_scroll: false,
        }
    }

    /// Applies the worker's restored state: navigate to the last dir, keep
    /// the selection by NAME (the anchor machinery re-seats the index once
    /// the listing arrives), request one scroll-into-view. Any saved state
    /// is marked as already persisted to avoid a redundant immediate write.
    fn apply_restore(&mut self, state: SessionState) {
        self.last_saved = Some(state.clone());
        self.cwd = state.cwd;
        self.entries.clear();
        self.display.clear();
        self.selection = 0;
        self.anchor = state.selection;
        self.filter.clear();
        self.pending_scroll = true;
    }

    fn descend(&mut self, name: &str) {
        self.cwd = self.cwd.join(name);
        self.selection = 0;
        self.anchor = None;
        self.filter.clear();
    }

    fn parent(&mut self) {
        if let Some(parent) = self.cwd.parent()
            && parent != self.cwd {
                self.cwd = parent.to_path_buf();
                self.selection = 0;
                self.anchor = None;
                self.filter.clear();
            }
    }

    /// Selection change => re-pin the anchor to the entry now selected.
    fn pin_anchor(&mut self) {
        self.anchor = self.display.get(self.selection).map(|e| e.name.clone());
    }

    /// After any listing refresh: keep the anchored NAME selected when it
    /// survived (the partial-to-sorted swap included); else clamp.
    fn remap_selection(&mut self) {
        let anchored = self
            .display
            .get(self.selection)
            .is_some_and(|e| self.anchor.as_deref() == Some(e.name.as_str()));
        if anchored {
            return;
        }
        self.selection = self
            .anchor
            .as_deref()
            .and_then(|a| self.display.iter().position(|e| e.name == a))
            .unwrap_or(0);
    }

    fn descend_selected(&mut self) {
        if let Some(entry) = self.display.get(self.selection)
            && entry.is_dir() {
                let name = entry.name.clone();
                self.descend(&name);
            }
    }

    pub fn save_session(&self) {
        if let Some(path) = &self.session_path {
            let state = SessionState {
                cwd: self.cwd.clone(),
                selection: self.anchor.clone(),
            };
            let _ = session::save(path, &state);
        }
    }
}

impl Drop for FfShell {
    fn drop(&mut self) {
        self.save_session();
    }
}

impl eframe::App for FfShell {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let previous_selection = self.selection;

        // 1. Poll the engine: drain events, reconcile, refresh listing.
        let mut listing = self.scanner.poll(&self.cwd);

        // 2. First arrival of the restored (or default home) state: switch
        //    to that directory instantly, within this same frame.
        if let Some(state) = self.scanner.take_restored() {
            self.apply_restore(state);
            listing = self.scanner.poll(&self.cwd);
        }
        self.entries = listing.entries;
        self.listing_loading = !listing.complete;

        // 2. Recompute the filtered view, then re-seat the anchor.
        self.display = self.finder.filter(&self.entries, &self.filter);
        self.remap_selection();

        // 3. Keys, before widgets so filter typing stays literal.
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if !self.filter_focused {
            if pressed(Key::Slash) {
                self.filter_focus_request = true;
            } else if pressed(Key::ArrowUp) {
                if self.selection > 0 {
                    self.selection -= 1;
                    self.pin_anchor();
                }
            } else if pressed(Key::ArrowDown) {
                if self.selection + 1 < self.display.len() {
                    self.selection += 1;
                    self.pin_anchor();
                }
            } else if pressed(Key::Enter) {
                self.descend_selected();
            } else if pressed(Key::Backspace) {
                self.parent();
            }
        } else {
            if pressed(Key::Escape) {
                self.filter.clear(); // panel auto-hides, focus drops next frame
            } else if pressed(Key::Enter) {
                self.descend_selected();
            } else if pressed(Key::Backspace) && self.filter.is_empty() {
                self.parent();
            }
        }
        let selection_moved = self.selection != previous_selection;

        // 4. Session persistence: hand the state to the worker whenever it
        //    changed (debounced write there — never this thread).
        let current = SessionState {
            cwd: self.cwd.clone(),
            selection: self.display.get(self.selection).map(|e| e.name.clone()),
        };
        if self.last_saved.as_ref() != Some(&current) {
            self.scanner.persist(current.clone());
            self.last_saved = Some(current);
        }

        self.pointer_over_interactive = false;

        // --- bottom: debug status line (v1 replaces it with the hairline)
        let engine_error = self.scanner.last_error().map(str::to_owned);
        Panel::bottom("status")
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let path = format!("[{}]", self.cwd.display());
                    ui.label(RichText::new(path).color(self.palette.muted).size(11.0));
                    let mut note = format!(" {} items", self.display.len());
                    if self.listing_loading {
                        note.push_str("  |  loading");
                    } else if let Some(error) = &engine_error {
                        note = format!("  |  cannot read: {error}");
                    }
                    if !self.filter.is_empty() {
                        note.push_str("  |  filter");
                    }
                    if !self.config_warnings.is_empty() {
                        note.push_str(&format!("  |  cfg: {} warn", self.config_warnings.len()));
                    }
                    ui.label(RichText::new(note).color(self.palette.muted).size(11.0));
                });
            });

        // --- bottom: filter line, only while summoned or holding text
        if self.filter_focus_request || self.filter_focused || !self.filter.is_empty() {
            Panel::bottom("filter")
                .show_separator_line(false)
                .show(ui, |ui| {
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.filter)
                            .desired_width(f32::INFINITY)
                            .hint_text("/  type to filter  (esc to clear)"),
                    );
                    self.filter_focused = resp.has_focus();
                    self.pointer_over_interactive |= resp.hovered();
                    if self.filter_focus_request {
                        resp.request_focus();
                        if resp.has_focus() {
                            self.filter_focus_request = false;
                        }
                    }
                });
        }

        CentralPanel::default().show(ui, |ui| {
            if self.display.is_empty() && !self.listing_loading {
                ui.label(
                    RichText::new(if self.filter.is_empty() {
                        "empty directory"
                    } else {
                        "no matches"
                    })
                    .color(self.palette.muted),
                );
                return;
            }

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Row snapshot: owned values, so selection mutations don't
                    // fight the iterator over self.display.
                    let rows: Vec<(String, bool)> = self
                        .display
                        .iter()
                        .map(|e| (e.name.clone(), e.is_dir()))
                        .collect();
                    for (i, (name, is_dir)) in rows.iter().enumerate() {
                        let selected = i == self.selection;
                        let text = RichText::new(name.clone()).color(if *is_dir {
                            self.palette.accent
                        } else {
                            self.palette.fg
                        });
                        let resp = ui.selectable_label(selected, text);
                        self.pointer_over_interactive |= resp.hovered();

                        if resp.clicked() {
                            self.selection = i;
                            self.pin_anchor();
                        }
                        if resp.double_clicked() {
                            self.selection = i;
                            if *is_dir {
                                self.descend(name);
                            }
                            // file open lands with the opener work (v0.5)
                        }
                        if selected && (selection_moved || self.pending_scroll) {
                            resp.scroll_to_me(None);
                        }
                    }
                });
        });

        // --- frameless drag: press on any non-interactive space (PLAN §2)
        let press = ctx.input(|i| i.pointer.primary_pressed());
        if press && !self.pointer_over_interactive {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }

        // Restored-scroll is one-shot: consume once rows were displaying.
        if self.pending_scroll && !self.display.is_empty() {
            self.pending_scroll = false;
        }
    }

    /// Guaranteed final save. Runs after the render loop is gone — outside
    /// any frame — so the no-block-on-disk invariant (PLAN §1) still holds;
    /// the worker's debounced writes cover everything until this moment.
    fn on_exit(&mut self) {
        self.save_session();
    }
}
