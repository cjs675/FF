//! The borderless shell, on the real engine (PLAN §5 engine milestone).
//! The frame's only contact with the filesystem is the non-blocking drain
//! inside `scanner.poll` — syscalls are structurally off the UI thread.
//! The selection is anchored by NAME across listing refreshes (raw first
//! batch to sorted completion), which is the machinery the watcher reuses
//! and session restore too: the last dir + selection come back on relaunch,
//! delegated through the worker (reads and debounced writes both).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{CentralPanel, Key, Panel, RichText, ScrollArea, TextEdit, Ui, ViewportCommand};
use ff_core::cfg::{self, Config};
use ff_core::find::{FileFinder, PortableFilter};
use ff_core::model::{AppOpener, Entry, EntryStats};
use ff_core::scan::{DirSource, FsEngine, WatcherState};
use ff_core::session::{self, SessionState};

use crate::theme::{self, Palette};

fn truncate_middle(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let left = max_len.saturating_sub(3) / 2;
        let right = max_len.saturating_sub(3) - left;
        let start: String = s.chars().take(left).collect();
        let end: String = s.chars().skip(char_count - right).collect();
        format!("{start}...{end}")
    }
}

struct OpenWithModal {
    path: PathBuf,
    name: String,
    filter: String,
    selection: usize,
    apps: Vec<AppOpener>,
    loading: bool,
    focus_filter: bool,
}

pub struct FfShell {
    #[allow(dead_code)]
    pub config: Config,
    #[allow(dead_code)]
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
    /// Bounded in-memory cache of lazy stat results for visible rows.
    stat_cache: HashMap<PathBuf, EntryStats>,
    /// In-window open-with application modal state.
    open_with_modal: Option<OpenWithModal>,
    /// Status hairline error toast: (message, expires_at).
    status_toast: Option<(String, Instant)>,
}

impl FfShell {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (config, config_warnings) = match cfg::default_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(contents) => cfg::parse(&contents),
            None => (Config::default(), Vec::new()),
        };

        let palette = theme::palette_for(&config.theme, cc.egui_ctx.system_theme());
        theme::setup_fonts(&cc.egui_ctx);
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
            stat_cache: HashMap::new(),
            open_with_modal: None,
            status_toast: None,
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

    fn enter_selected(&mut self) {
        if let Some(entry) = self.display.get(self.selection) {
            if entry.is_dir() {
                let name = entry.name.clone();
                self.descend(&name);
            } else {
                let path = entry.path.clone();
                if let Err(e) = ff_core::ops::open::open_default(&path) {
                    self.status_toast = Some((
                        format!("cannot open: {e}"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
            }
        }
    }

    fn open_with_selected(&mut self) {
        if let Some(entry) = self.display.get(self.selection)
            && !entry.is_dir()
        {
            let path = entry.path.clone();
            let name = entry.name.clone();
            self.scanner.enumerate_openers(path.clone());
            self.open_with_modal = Some(OpenWithModal {
                path,
                name,
                filter: String::new(),
                selection: 0,
                apps: Vec::new(),
                loading: true,
                focus_filter: true,
            });
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

        for (path, stats) in self.scanner.take_stats() {
            if self.stat_cache.len() >= 10_000 {
                self.stat_cache.clear();
            }
            self.stat_cache.insert(path, stats);
        }

        if let Some((path, apps)) = self.scanner.take_openers()
            && let Some(modal) = &mut self.open_with_modal
            && modal.path == path
        {
            modal.apps = apps;
            modal.loading = false;
            modal.selection = 0;
        }

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
        let open_with_requested = ctx.input(|i| {
            (i.modifiers.command && i.key_pressed(Key::O))
                || (i.modifiers.alt && i.key_pressed(Key::Enter))
        });

        if self.open_with_modal.is_none() {
            if open_with_requested {
                self.open_with_selected();
            } else if !self.filter_focused {
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
                } else if pressed(Key::Home) {
                    if !self.display.is_empty() {
                        self.selection = 0;
                        self.pin_anchor();
                    }
                } else if pressed(Key::End) {
                    if !self.display.is_empty() {
                        self.selection = self.display.len() - 1;
                        self.pin_anchor();
                    }
                } else if pressed(Key::PageUp) {
                    self.selection = self.selection.saturating_sub(15);
                    self.pin_anchor();
                } else if pressed(Key::PageDown) {
                    if !self.display.is_empty() {
                        self.selection = (self.selection + 15).min(self.display.len() - 1);
                        self.pin_anchor();
                    }
                } else if pressed(Key::Enter) {
                    self.enter_selected();
                } else if pressed(Key::Backspace) {
                    self.parent();
                }
            } else if pressed(Key::Escape) {
                self.filter.clear(); // panel auto-hides, focus drops next frame
            } else if pressed(Key::Enter) {
                self.enter_selected();
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

        // --- bottom: 24px status hairline (PLAN §10 it16)
        let engine_error = self.scanner.last_error().map(str::to_owned);
        let now = Instant::now();
        let active_toast = match &self.status_toast {
            Some((msg, expires)) if now < *expires => {
                ctx.request_repaint_after(*expires - now);
                Some(msg.clone())
            }
            _ => {
                if self.status_toast.is_some() {
                    self.status_toast = None;
                }
                None
            }
        };

        let hairline_border = self.palette.border;

        Panel::bottom("status_hairline")
            .show_separator_line(false)
            .resizable(false)
            .exact_size(24.0)
            .frame(egui::Frame::NONE.fill(self.palette.bg))
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 24.0),
                    egui::Sense::hover(),
                );
                let painter = ui.painter();

                // 1px stone divider line across top of hairline
                painter.line_segment(
                    [rect.left_top(), rect.right_top()],
                    egui::Stroke::new(1.0, hairline_border),
                );

                let center_y = rect.center().y;

                // Priority 1: Left segment (item count and shallow file size)
                let dir_size_str = match self.scanner.dir_size() {
                    Some(bytes) => format!("{} in files", EntryStats { size: bytes, mtime_secs: 0 }.format_size()),
                    None => "--".to_string(),
                };
                let mut left_text = format!("{} items  |  {}", self.display.len(), dir_size_str);
                if self.listing_loading {
                    left_text.push_str("  |  loading");
                } else if let Some(error) = &engine_error {
                    left_text = format!("cannot read: {error}");
                }
                painter.text(
                    egui::pos2(rect.min.x + 10.0, center_y),
                    egui::Align2::LEFT_CENTER,
                    left_text,
                    egui::FontId::monospace(11.0),
                    self.palette.muted,
                );

                // Priority 2: Right segment (git branch, dirty marker, 6px watcher status dot)
                let dot_center = egui::pos2(rect.max.x - 12.0, center_y);
                let dot_color = match self.scanner.watcher_state() {
                    WatcherState::Active => self.palette.green,
                    WatcherState::Coalescing => self.palette.amber,
                    WatcherState::Degraded => self.palette.accent,
                };
                painter.circle_filled(dot_center, 3.0, dot_color);

                if let Some(git) = self.scanner.git_status() {
                    let git_text = if git.is_dirty {
                        format!("{} *", git.branch)
                    } else {
                        git.branch.clone()
                    };
                    let git_color = if git.is_dirty {
                        self.palette.accent
                    } else {
                        self.palette.muted
                    };
                    painter.text(
                        egui::pos2(rect.max.x - 24.0, center_y),
                        egui::Align2::RIGHT_CENTER,
                        git_text,
                        egui::FontId::monospace(11.0),
                        git_color,
                    );
                }

                // Priority 3: Center segment (error toast OR selected item inspection with responsive degradation)
                let width = rect.width();
                if let Some(toast) = active_toast {
                    painter.text(
                        egui::pos2(rect.center().x, center_y),
                        egui::Align2::CENTER_CENTER,
                        toast,
                        egui::FontId::monospace(11.0),
                        self.palette.accent,
                    );
                } else if width >= 500.0
                    && let Some(entry) = self.display.get(self.selection)
                {
                    let center_text = if width < 600.0 {
                        truncate_middle(&entry.name, 20)
                    } else {
                        let stats = self.stat_cache.get(&entry.path);
                        let size = stats.map(|s| s.format_size()).unwrap_or_default();
                        let mtime = stats.map(|s| s.format_mtime()).unwrap_or_default();
                        if !entry.is_dir() && !size.is_empty() && !mtime.is_empty() {
                            format!("{}  ·  {}  ·  {}", truncate_middle(&entry.name, 26), size, mtime)
                        } else {
                            truncate_middle(&entry.name, 32)
                        }
                    };
                    painter.text(
                        egui::pos2(rect.center().x, center_y),
                        egui::Align2::CENTER_CENTER,
                        center_text,
                        egui::FontId::monospace(11.0),
                        self.palette.muted,
                    );
                }
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

            let row_height = ui.spacing().interact_size.y;
            let total_rows = self.display.len();

            let mut clicked = None;
            let mut double_clicked = None;
            let mut missing_stats = Vec::new();

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, row_height, total_rows, |ui, row_range| {
                    if (selection_moved || self.pending_scroll) && self.selection < total_rows {
                        let top_y = self.selection as f32 * (row_height + ui.spacing().item_spacing.y);
                        let target_rect = egui::Rect::from_min_size(
                            egui::pos2(ui.max_rect().min.x, ui.max_rect().min.y + top_y),
                            egui::vec2(ui.available_width(), row_height),
                        );
                        ui.scroll_to_rect(target_rect, None);
                    }

                    for i in row_range {
                        let entry = &self.display[i];
                        if !self.stat_cache.contains_key(&entry.path) {
                            missing_stats.push(entry.path.clone());
                        }

                        let selected = i == self.selection;
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::click(),
                        );
                        self.pointer_over_interactive |= resp.hovered();

                        if selected {
                            ui.painter().rect_filled(rect, 0.0, self.palette.sel);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, 0.0, self.palette.hover);
                        }

                        let name_color = if entry.is_dir() {
                            self.palette.accent
                        } else {
                            self.palette.fg
                        };
                        ui.painter().text(
                            egui::pos2(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &entry.name,
                            egui::FontId::proportional(13.0),
                            name_color,
                        );

                        if !entry.is_dir() {
                            let size_str = if let Some(stats) = self.stat_cache.get(&entry.path) {
                                stats.format_size()
                            } else {
                                String::new()
                            };
                            ui.painter().text(
                                egui::pos2(rect.max.x - 130.0, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                size_str,
                                egui::FontId::monospace(11.0),
                                self.palette.muted,
                            );
                        }

                        let date_str = if let Some(stats) = self.stat_cache.get(&entry.path) {
                            stats.format_mtime()
                        } else {
                            String::new()
                        };
                        ui.painter().text(
                            egui::pos2(rect.max.x - 12.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            date_str,
                            egui::FontId::proportional(11.0),
                            self.palette.muted,
                        );

                        if resp.clicked() {
                            clicked = Some(i);
                        }
                        if resp.double_clicked() {
                            double_clicked = Some(i);
                        }
                    }
                });

            if !missing_stats.is_empty() {
                self.scanner.request_stats(missing_stats);
            }

            if let Some(i) = clicked {
                self.selection = i;
                self.pin_anchor();
            }
            if let Some(i) = double_clicked {
                self.selection = i;
                self.enter_selected();
            }
        });

        // --- modal palette: Open With application chooser
        if let Some(mut modal) = self.open_with_modal.take() {
            let mut close_modal = false;
            let mut launch_app: Option<AppOpener> = None;
            let mut launch_chooser = false;

            let query = modal.filter.trim().to_lowercase();
            let filtered_indices: Vec<usize> = modal
                .apps
                .iter()
                .enumerate()
                .filter(|(_, app)| {
                    query.is_empty() || app.name.to_lowercase().contains(&query)
                })
                .map(|(idx, _)| idx)
                .collect();

            if !filtered_indices.is_empty() {
                modal.selection = modal.selection.min(filtered_indices.len() - 1);
            } else {
                modal.selection = 0;
            }

            let esc_pressed = ctx.input(|i| i.key_pressed(Key::Escape));
            let enter_pressed = ctx.input(|i| i.key_pressed(Key::Enter));
            let up_pressed = ctx.input(|i| i.key_pressed(Key::ArrowUp));
            let down_pressed = ctx.input(|i| i.key_pressed(Key::ArrowDown));

            if esc_pressed {
                close_modal = true;
            } else if up_pressed {
                modal.selection = modal.selection.saturating_sub(1);
            } else if down_pressed {
                if !filtered_indices.is_empty() {
                    modal.selection = (modal.selection + 1).min(filtered_indices.len() - 1);
                }
            } else if enter_pressed {
                if !filtered_indices.is_empty() {
                    launch_app = Some(modal.apps[filtered_indices[modal.selection]].clone());
                } else if !modal.loading && modal.apps.is_empty() {
                    launch_chooser = true;
                }
            }

            let border_color = self.palette.border;

            egui::Window::new("open_with_palette")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .fixed_size(egui::vec2(400.0, 260.0))
                .frame(
                    egui::Frame::NONE
                        .fill(self.palette.surface)
                        .stroke(egui::Stroke::new(1.0, border_color))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(&ctx, |ui| {
                    self.pointer_over_interactive = true;
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 8.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Open with")
                                .color(self.palette.fg)
                                .size(13.0)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(&modal.name)
                                .color(self.palette.muted)
                                .size(13.0),
                        );
                    });

                    let filter_response = ui.add(
                        TextEdit::singleline(&mut modal.filter)
                            .desired_width(f32::INFINITY)
                            .hint_text("Search applications..."),
                    );
                    if modal.focus_filter {
                        filter_response.request_focus();
                        if filter_response.has_focus() {
                            modal.focus_filter = false;
                        }
                    }

                    ui.separator();

                    ScrollArea::vertical()
                        .max_height(180.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if modal.loading {
                                ui.add_space(16.0);
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new("Searching applications...")
                                            .color(self.palette.muted)
                                            .size(12.0),
                                    );
                                });
                            } else if modal.apps.is_empty() {
                                ui.add_space(12.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("No application registered for this file type")
                                            .color(self.palette.muted)
                                            .size(12.0),
                                    );
                                    ui.add_space(8.0);
                                    let chooser_btn = ui.button(
                                        RichText::new("Open with system chooser...")
                                            .color(self.palette.accent)
                                            .size(12.0),
                                    );
                                    if chooser_btn.clicked() {
                                        launch_chooser = true;
                                    }
                                });
                            } else if filtered_indices.is_empty() {
                                ui.add_space(16.0);
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new("No matching application")
                                            .color(self.palette.muted)
                                            .size(12.0),
                                    );
                                });
                            } else {
                                let row_height = 24.0;
                                for (display_idx, &app_idx) in filtered_indices.iter().enumerate() {
                                    let app = &modal.apps[app_idx];
                                    let is_selected = display_idx == modal.selection;

                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), row_height),
                                        egui::Sense::click(),
                                    );

                                    if is_selected {
                                        ui.painter().rect_filled(rect, 4.0, self.palette.sel);
                                    } else if resp.hovered() {
                                        ui.painter().rect_filled(rect, 4.0, self.palette.hover);
                                    }

                                    ui.painter().text(
                                        egui::pos2(rect.min.x + 8.0, rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        &app.name,
                                        egui::FontId::proportional(12.0),
                                        self.palette.fg,
                                    );

                                    if app.is_default {
                                        ui.painter().text(
                                            egui::pos2(rect.max.x - 8.0, rect.center().y),
                                            egui::Align2::RIGHT_CENTER,
                                            "(default)",
                                            egui::FontId::proportional(11.0),
                                            self.palette.muted,
                                        );
                                    }

                                    if resp.clicked() {
                                        modal.selection = display_idx;
                                        launch_app = Some(app.clone());
                                    }
                                }
                            }
                        });
                });

            if let Some(app) = launch_app {
                if let Err(e) = ff_core::ops::open::open_with(&app.id, &modal.path) {
                    self.status_toast = Some((
                        format!("cannot open: {e}"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
            } else if launch_chooser {
                if let Err(e) = ff_core::ops::open::open_chooser(&modal.path) {
                    self.status_toast = Some((
                        format!("cannot open: {e}"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
            } else if !close_modal {
                self.open_with_modal = Some(modal);
            }
        }

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
