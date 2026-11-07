//! The borderless shell, on the real engine (PLAN §5 engine milestone).
//! The frame's only contact with the filesystem is the non-blocking drain
//! inside `scanner.poll` — syscalls are structurally off the UI thread.
//! The selection is anchored by NAME across listing refreshes (raw first
//! batch to sorted completion), which is the machinery the watcher will
//! reuse next milestone.

use std::path::PathBuf;

use egui::{CentralPanel, Key, Panel, RichText, ScrollArea, TextEdit, Ui, ViewportCommand};
use ff_core::find::{FileFinder, PortableFilter};
use ff_core::model::Entry;
use ff_core::scan::{DirSource, FsEngine};

use crate::theme;

pub struct FfShell {
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
}

impl FfShell {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let cwd = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            scanner: FsEngine::launch(false),
            finder: PortableFilter,
            cwd,
            entries: Vec::new(),
            display: Vec::new(),
            anchor: None,
            filter: String::new(),
            selection: 0,
            listing_loading: true,
            filter_focus_request: false,
            filter_focused: false,
            pointer_over_interactive: false,
        }
    }

    fn descend(&mut self, name: &str) {
        self.cwd = self.cwd.join(name);
        self.selection = 0;
        self.anchor = None;
        self.filter.clear();
    }

    fn parent(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            if parent != self.cwd {
                self.cwd = parent.to_path_buf();
                self.selection = 0;
                self.anchor = None;
                self.filter.clear();
            }
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
        if let Some(entry) = self.display.get(self.selection) {
            if entry.is_dir() {
                let name = entry.name.clone();
                self.descend(&name);
            }
        }
    }
}

impl eframe::App for FfShell {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let previous_selection = self.selection;

        // 1. Poll the engine: drain events, reconcile, refresh listing.
        let listing = self.scanner.poll(&self.cwd);
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

        self.pointer_over_interactive = false;

        // --- bottom: debug status line (v1 replaces it with the hairline)
        let engine_error = self.scanner.last_error().map(str::to_owned);
        Panel::bottom("status")
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let path = format!("[{}]", self.cwd.display());
                    ui.label(RichText::new(path).color(theme::MUTED).size(11.0));
                    let mut note = format!(" {} items", self.display.len());
                    if self.listing_loading {
                        note.push_str("  |  loading");
                    } else if let Some(error) = &engine_error {
                        note = format!("  |  cannot read: {error}");
                    }
                    if !self.filter.is_empty() {
                        note.push_str("  |  filter");
                    }
                    ui.label(RichText::new(note).color(theme::MUTED).size(11.0));
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
                    .color(theme::MUTED),
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
                            theme::ACCENT
                        } else {
                            theme::FG
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
                        if selected && selection_moved {
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
    }
}
