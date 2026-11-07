//! The borderless shell. v0 scope (PLAN §5): fake directory via
//! `MemoryScanner` (disk-untouched demo), substring filter, keyboard
//! navigation, frameless drag region on empty space. Enter/double-click
//! descends; file open + Peek are v0.5/v1.

use std::path::{Path, PathBuf};

use egui::{CentralPanel, Key, Panel, RichText, ScrollArea, TextEdit, Ui, ViewportCommand};
use ff_core::find::{FileFinder, PortableFilter};
use ff_core::model::Entry;
use ff_core::scan::{DirScanner, MemoryScanner};

use crate::theme;

pub struct FfShell {
    cwd: PathBuf,
    scanner: MemoryScanner,
    finder: PortableFilter,
    filter: String,
    selection: usize,
    entries: Vec<Entry>,
    filter_focus_request: bool,
    filter_focused: bool,
    pointer_over_interactive: bool,
}

impl FfShell {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let mut scanner = MemoryScanner::default();
        demo_tree(&mut scanner);

        let mut shell = Self {
            cwd: PathBuf::from("root"),
            scanner,
            finder: PortableFilter,
            filter: String::new(),
            selection: 0,
            entries: Vec::new(),
            filter_focus_request: false,
            filter_focused: false,
            pointer_over_interactive: false,
        };
        shell.refresh();
        shell
    }

    fn refresh(&mut self) {
        self.entries = self.scanner.scan(&self.cwd);
        self.selection = self.selection.min(self.entries.len().saturating_sub(1));
    }

    fn descend(&mut self, name: &str) {
        let next = self.cwd.join(name);
        if self.scanner.has(&next) {
            self.cwd = next;
            self.selection = 0;
            self.filter.clear();
            self.refresh();
        }
    }

    fn parent(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            let parent = parent.to_path_buf();
            if self.scanner.has(&parent) {
                self.cwd = parent;
                self.selection = 0;
                self.filter.clear();
                self.refresh();
            }
        }
    }
}

fn demo_tree(s: &mut MemoryScanner) {
    let root = Path::new("root");
    s.put_dir(root, "crates");
    s.put_dir(root, "docs");
    s.put_file(root, "PLAN.md");
    s.put_file(root, "readme.md");
    s.put_file(root, "ff.toml");

    let crates = root.join("crates");
    s.put_dir(&crates, "core");
    s.put_dir(&crates, "app");
    s.put_file(&crates, "Cargo.toml");
    let core = crates.join("core");
    s.put_file(&core, "lib.rs");
    s.put_file(&core, "scan.rs");
    s.put_file(&core, "cfg.rs");
    let app = crates.join("app");
    s.put_file(&app, "main.rs");
    s.put_file(&app, "shell.rs");
    s.put_file(&app, "theme.rs");

    let docs = root.join("docs");
    s.put_file(&docs, "screenshot.png");
    s.put_file(&docs, "keymap.txt");
}

impl eframe::App for FfShell {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let prev_selection = self.selection;
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));

        // --- keys, before widgets so filter typing stays literal -------
        if !self.filter_focused {
            if pressed(Key::Slash) {
                self.filter_focus_request = true;
            }
            if pressed(Key::ArrowUp) {
                self.selection = self.selection.saturating_sub(1);
            }
            if pressed(Key::ArrowDown) {
                self.selection += 1;
            }
            if pressed(Key::Enter) {
                if let Some(e) = self.entries.get(self.selection) {
                    if e.is_dir() {
                        let name = e.name.clone();
                        self.descend(&name);
                    }
                }
            }
            if pressed(Key::Backspace) {
                self.parent();
            }
        } else {
            if pressed(Key::Escape) {
                self.filter.clear(); // panel auto-hides, focus drops next frame
            } else if pressed(Key::Enter) {
                if let Some(e) = self.entries.get(self.selection) {
                    if e.is_dir() {
                        let name = e.name.clone();
                        self.descend(&name);
                    }
                }
            } else if pressed(Key::Backspace) && self.filter.is_empty() {
                self.parent();
            }
        }

        self.pointer_over_interactive = false;

        // --- bottom: debug status line (v1 replaces it with the hairline)
        Panel::bottom("status")
            .show_separator_line(false)
            .show(ui, |ui| {
                let filter_note = if self.filter.is_empty() {
                    ""
                } else {
                    "  |  filter"
                };
                ui.horizontal(|ui| {
                    let path = format!("[{}]", self.cwd.display());
                    ui.label(RichText::new(path).color(theme::MUTED).size(11.0));
                    ui.label(
                        RichText::new(format!("  {} items{filter_note}", self.entries.len()))
                            .color(theme::MUTED)
                            .size(11.0),
                    );
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
            let display = self.finder.filter(&self.entries, &self.filter);

            self.selection = self.selection.min(display.len().saturating_sub(1));
            let selection_moved = self.selection != prev_selection;

            if display.is_empty() {
                ui.label(RichText::new("no matches").color(theme::MUTED));
                return;
            }

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (i, entry) in display.iter().enumerate() {
                        let selected = i == self.selection;
                        let text = RichText::new(&entry.name).color(if entry.is_dir() {
                            theme::ACCENT
                        } else {
                            theme::FG
                        });
                        let resp = ui.selectable_label(selected, text);
                        self.pointer_over_interactive |= resp.hovered();

                        if resp.clicked() {
                            self.selection = i;
                        }
                        if resp.double_clicked() {
                            self.selection = i;
                            if entry.is_dir() {
                                let name = entry.name.clone();
                                self.descend(&name);
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
