//! The borderless shell, on the real engine (PLAN §5 engine milestone).
//! The frame's only contact with the filesystem is the non-blocking drain
//! inside `scanner.poll` — syscalls are structurally off the UI thread.
//! The selection is anchored by NAME across listing refreshes (raw first
//! batch to sorted completion), which is the machinery the watcher reuses
//! and session restore too: the last dir + selection come back on relaunch,
//! delegated through the worker (reads and debounced writes both).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{CentralPanel, Key, Panel, PointerButton, RichText, ScrollArea, TextEdit, Ui, ViewportCommand};
use ff_core::cfg::{self, Config};
use ff_core::find::{FileFinder, FsearchFinder, PortableFilter};
use ff_core::model::{AppOpener, Entry, EntryStats};
use ff_core::scan::{DirSource, FileOpOutcome, FileOpRequest, FsEngine, WatcherState};
use ff_core::session::{self, SessionState};

use crate::launcher::{LauncherOutcome, LauncherState};
use crate::theme::{self, Palette};

pub(crate) fn truncate_middle(s: &str, max_len: usize) -> String {
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

#[derive(Debug, Clone)]
enum FileOpModal {
    Rename {
        path: PathBuf,
        input: String,
        initial_focused: bool,
    },
    NewFile {
        parent: PathBuf,
        input: String,
        initial_focused: bool,
    },
    NewFolder {
        parent: PathBuf,
        input: String,
        initial_focused: bool,
    },
}

pub struct FfShell {
    #[allow(dead_code)]
    pub config: Config,
    #[allow(dead_code)]
    pub config_warnings: Vec<String>,
    pub palette: Palette,
    scanner: FsEngine,
    finder: Box<dyn FileFinder>,
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
    /// Frequents view state: tracked visit directories and modal toggle.
    pub frequents: Vec<ff_core::session::FrequentEntry>,
    pub frequents_open: bool,
    pub frequents_selection: usize,
    pub frequents_filter: String,
    pub frequents_focus_filter: bool,
    last_visited_cwd: Option<PathBuf>,
    /// Navigation history stack for Back/Forward navigation.
    history_back: Vec<(PathBuf, Option<String>)>,
    history_forward: Vec<(PathBuf, Option<String>)>,
    /// Pending Vim count prefix (e.g. `5` in `5j`).
    vim_count: Option<usize>,
    /// Pending Vim `g` prefix (e.g. first `g` in `gg`).
    vim_pending_g: bool,
    /// Whether the in-window Peek preview pane is open.
    pub preview_open: bool,
    /// Active path inspected for peek preview.
    peek_path: Option<PathBuf>,
    /// Active peek payload delivered by the worker.
    peek_payload: Option<ff_core::ops::peek::PeekPayload>,
    /// Flag indicating peek request in flight.
    peek_loading: bool,
    /// Active GPU texture handle for image peek previews.
    peek_texture: Option<egui::TextureHandle>,
    /// Bounded in-memory undo journal (PLAN §4 ops).
    undo_journal: ff_core::ops::journal::UndoJournal,
    /// Modal prompt for file operations (rename, new file, new folder).
    file_op_modal: Option<FileOpModal>,
    /// Raycast-like command palette launcher state.
    launcher: LauncherState,
}



impl FfShell {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (mut config, config_warnings) = match cfg::default_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(contents) => cfg::parse(&contents),
            None => (Config::default(), Vec::new()),
        };

        // If a previously saved session stored a user-picked theme, honor it:
        if let Some(session_path) = session::default_path()
            && let Some(saved) = session::load(&session_path)
            && let Some(theme) = saved.theme
        {
            config.theme = theme;
        }

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

        let mut config_warnings = config_warnings;
        let finder: Box<dyn FileFinder> = match config.search.as_str() {
            "fsearch" => {
                #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
                {
                    Box::new(FsearchFinder::new())
                }
                #[cfg(not(all(target_os = "macos", feature = "fsearch-macos")))]
                {
                    config_warnings.push("search = fsearch configured, but fsearch is unavailable on this platform; falling back to portable".into());
                    Box::new(PortableFilter)
                }
            }
            "portable" => Box::new(PortableFilter),
            "auto" => {
                #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
                {
                    Box::new(FsearchFinder::new())
                }
                #[cfg(not(all(target_os = "macos", feature = "fsearch-macos")))]
                {
                    Box::new(PortableFilter)
                }
            }
            other => {
                config_warnings.push(format!("unknown search provider `{other}`, defaulting to auto"));
                #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
                {
                    Box::new(FsearchFinder::new())
                }
                #[cfg(not(all(target_os = "macos", feature = "fsearch-macos")))]
                {
                    Box::new(PortableFilter)
                }
            }
        };

        let preview_open = config.preview;

        Self {
            config,
            config_warnings,
            palette,
            scanner,
            finder,
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
            frequents: Vec::new(),
            frequents_open: false,
            frequents_selection: 0,
            frequents_filter: String::new(),
            frequents_focus_filter: false,
            last_visited_cwd: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            vim_count: None,
            vim_pending_g: false,
            preview_open,
            peek_path: None,
            peek_payload: None,
            peek_loading: false,
            peek_texture: None,
            undo_journal: ff_core::ops::journal::UndoJournal::default(),
            file_op_modal: None,
            launcher: LauncherState::default(),
        }
    }



    /// Applies the worker's restored state: navigate to the last dir, keep
    /// the selection by NAME (the anchor machinery re-seats the index once
    /// the listing arrives), request one scroll-into-view. Any saved state
    /// is marked as already persisted to avoid a redundant immediate write.
    fn apply_restore(&mut self, state: SessionState) {
        self.last_saved = Some(state.clone());
        self.frequents = state.frequents;

        match self.config.launch {
            cfg::Launch::Home => {
                self.cwd = session::home_dir();
                self.anchor = None;
            }
            cfg::Launch::Frequents => {
                self.cwd = state.cwd;
                self.anchor = state.selection;
                self.frequents_open = true;
                self.frequents_focus_filter = true;
            }
            cfg::Launch::Session => {
                self.cwd = state.cwd;
                self.anchor = state.selection;
            }
        }

        self.entries.clear();
        self.display.clear();
        self.selection = 0;
        self.filter.clear();
        self.pending_scroll = true;
    }

    fn push_history(&mut self) {
        if !self.cwd.as_os_str().is_empty() {
            self.history_back.push((self.cwd.clone(), self.anchor.clone()));
            if self.history_back.len() > 100 {
                self.history_back.remove(0);
            }
            self.history_forward.clear();
        }
    }

    fn go_back(&mut self) {
        if self.frequents_open {
            self.frequents_open = false;
            return;
        }
        if let Some((prev_cwd, prev_anchor)) = self.history_back.pop() {
            self.history_forward.push((self.cwd.clone(), self.anchor.clone()));
            self.cwd = prev_cwd;
            self.selection = 0;
            self.anchor = prev_anchor;
            self.filter.clear();
            self.pending_scroll = true;
        } else if let Some(parent) = self.cwd.parent().map(|p| p.to_path_buf())
            && parent != self.cwd
        {
            self.history_forward.push((self.cwd.clone(), self.anchor.clone()));
            self.cwd = parent;
            self.selection = 0;
            self.anchor = None;
            self.filter.clear();
            self.pending_scroll = true;
        }
    }

    fn go_forward(&mut self) {
        if let Some((next_cwd, next_anchor)) = self.history_forward.pop() {
            self.history_back.push((self.cwd.clone(), self.anchor.clone()));
            self.cwd = next_cwd;
            self.selection = 0;
            self.anchor = next_anchor;
            self.filter.clear();
            self.pending_scroll = true;
        }
    }

    fn navigate_to(&mut self, target: PathBuf) {
        if self.cwd != target {
            self.push_history();
        }
        self.cwd = target;
        self.selection = 0;
        self.anchor = None;
        self.filter.clear();
        self.pending_scroll = true;
    }

    fn descend(&mut self, name: &str) {
        self.push_history();
        self.cwd = self.cwd.join(name);
        self.selection = 0;
        self.anchor = None;
        self.filter.clear();
    }

    fn parent(&mut self) {
        if let Some(parent) = self.cwd.parent().map(|p| p.to_path_buf())
            && parent != self.cwd
        {
            self.push_history();
            self.cwd = parent;
            self.selection = 0;
            self.anchor = None;
            self.filter.clear();
        }
    }

    fn render_frequents_view(&mut self, ui: &mut Ui, selection_moved: bool) {
        let now = ff_core::session::now_epoch_secs();
        let ranked = ff_core::session::ranked_frequents(&self.frequents, now);
        let q = self.frequents_filter.trim().to_lowercase();
        let filtered: Vec<&ff_core::session::FrequentEntry> = ranked
            .iter()
            .filter(|f| {
                if q.is_empty() {
                    true
                } else {
                    f.path.to_string_lossy().to_lowercase().contains(&q)
                }
            })
            .collect();

        // Top search / filter input
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("FREQUENTS")
                    .monospace()
                    .size(11.0)
                    .color(self.palette.green),
            );
            let resp = ui.add(
                TextEdit::singleline(&mut self.frequents_filter)
                    .desired_width(f32::INFINITY)
                    .hint_text("Type to filter frequent folders... (Enter to open, Esc to close)"),
            );
            if resp.changed() {
                self.frequents_selection = 0;
            }
            if self.frequents_focus_filter {
                resp.request_focus();
                if resp.has_focus() {
                    self.frequents_focus_filter = false;
                }
            }
        });

        ui.add_space(4.0);
        let (hairline_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 1.0),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(hairline_rect, 0.0, self.palette.border);
        ui.add_space(4.0);

        if filtered.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(if self.frequents.is_empty() {
                        "No frequent folders recorded yet. Browse directories to populate."
                    } else {
                        "No frequent folders match filter."
                    })
                    .color(self.palette.muted)
                    .size(13.0),
                );
            });
            return;
        }

        if self.frequents_selection >= filtered.len() {
            self.frequents_selection = filtered.len().saturating_sub(1);
        }

        let row_height = ui.spacing().interact_size.y;
        let total_rows = filtered.len();
        let mut navigate_to_path: Option<PathBuf> = None;

        let scroll_out = ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, total_rows, |ui, row_range| {
                if selection_moved && self.frequents_selection < total_rows {
                    let row_height_with_spacing = row_height + ui.spacing().item_spacing.y;
                    let target_top = ui.max_rect().top()
                        + (self.frequents_selection as f32 - row_range.start as f32)
                            * row_height_with_spacing;
                    let target_rect = egui::Rect::from_min_size(
                        egui::pos2(ui.max_rect().min.x, target_top),
                        egui::vec2(ui.available_width(), row_height),
                    );
                    ui.scroll_to_rect_animation(
                        target_rect,
                        None,
                        egui::style::ScrollAnimation::none(),
                    );
                }

                for i in row_range {
                    let entry = filtered[i];
                    let selected = i == self.frequents_selection;
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_height),
                        egui::Sense::click(),
                    );
                    self.pointer_over_interactive |= resp.hovered();

                    if selected {
                        if selection_moved {
                            resp.scroll_to_me(None);
                        }
                        ui.painter().rect_filled(rect, 0.0, self.palette.sel);
                    }

                    // 1px hairline divider on row bottom
                    let div_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x, rect.max.y - 1.0),
                        egui::pos2(rect.max.x, rect.max.y),
                    );
                    ui.painter().rect_filled(div_rect, 0.0, self.palette.border);

                    let name = entry
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| entry.path.to_string_lossy().to_string());
                    let full_path = entry.path.to_string_lossy();

                    let width = rect.width();
                    let show_path = width >= 380.0;
                    let show_count = width >= 240.0;
                    let right_margin = if show_count { 80.0 } else { 8.0 };
                    let max_name_w = if show_path {
                        130.0
                    } else {
                        (width - right_margin - 16.0).max(20.0)
                    };
                    let char_budget = (max_name_w / 7.2) as usize;
                    let display_name = if name.chars().count() > char_budget {
                        truncate_middle(&name, char_budget.max(6))
                    } else {
                        name.clone()
                    };

                    let clip_rect = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.min.x + 8.0 + max_name_w, rect.max.y),
                    );
                    ui.painter().with_clip_rect(clip_rect).text(
                        egui::pos2(rect.min.x + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        display_name,
                        egui::FontId::monospace(13.0),
                        self.palette.accent,
                    );

                    if show_path {
                        let path_w = (width - 150.0 - right_margin - 8.0).max(20.0);
                        let path_budget = (path_w / 7.2) as usize;
                        let path_display = truncate_middle(&full_path, path_budget.max(10));
                        let path_clip = egui::Rect::from_min_max(
                            egui::pos2(rect.min.x + 150.0, rect.min.y),
                            egui::pos2(rect.min.x + 150.0 + path_w, rect.max.y),
                        );
                        ui.painter().with_clip_rect(path_clip).text(
                            egui::pos2(rect.min.x + 150.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            path_display,
                            egui::FontId::monospace(11.5),
                            self.palette.muted,
                        );
                    }

                    if show_count {
                        let count_str = format!("{} visits", entry.count);
                        ui.painter().text(
                            egui::pos2(rect.max.x - 12.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            count_str,
                            egui::FontId::monospace(11.0),
                            self.palette.muted,
                        );
                    }

                    if resp.clicked() {
                        self.frequents_selection = i;
                    }
                    if resp.double_clicked() {
                        navigate_to_path = Some(entry.path.clone());
                    }
                }
            });

        let scrollbar_width = ui.spacing().scroll.bar_width + 10.0;
        let scrollbar_rect = egui::Rect::from_min_max(
            egui::pos2(scroll_out.inner_rect.right() - scrollbar_width, scroll_out.inner_rect.top()),
            scroll_out.inner_rect.right_bottom(),
        );
        if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
            self.pointer_over_interactive |= scrollbar_rect.contains(pos);
        }

        if let Some(target) = navigate_to_path {
            self.frequents_open = false;
            self.navigate_to(target);
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
            } else if !self.preview_open {
                self.preview_open = true;
                self.peek_path = None;
                self.peek_payload = None;
                self.peek_texture = None;
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
                frequents: self.frequents.clone(),
                theme: Some(self.config.theme.clone()),
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

        while let Some((_ticket, path, payload)) = self.scanner.take_peek() {
            if self.launcher.open && self.launcher.peek_path.as_ref() == Some(&path) {
                if let ff_core::ops::peek::PeekPayload::Image { width, height, rgba, .. } = &payload {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [*width as usize, *height as usize],
                        rgba,
                    );
                    self.launcher.peek_texture = Some(ctx.load_texture(
                        "launcher_peek_image",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                } else {
                    self.launcher.peek_texture = None;
                }
                self.launcher.peek_payload = Some(payload.clone());
                self.launcher.peek_loading = false;
            }
            if self.peek_path.as_ref() == Some(&path) {
                if let ff_core::ops::peek::PeekPayload::Image { width, height, rgba, .. } = &payload {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [*width as usize, *height as usize],
                        rgba,
                    );
                    self.peek_texture = Some(ctx.load_texture(
                        "peek_image",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                } else {
                    self.peek_texture = None;
                }
                self.peek_payload = Some(payload);
                self.peek_loading = false;
            }
        }

        if let Some((_ticket, result)) = self
            .scanner
            .take_content_search()
            .filter(|(t, _)| *t == self.launcher.content_ticket)
        {
            self.launcher.content_results = Some(result);
            self.launcher.content_loading = false;
        }

        // Process file operation outcomes from the background worker (PLAN §4 ops)
        for outcome in self.scanner.take_file_ops() {
            match outcome {
                Ok(FileOpOutcome::Trashed { original_path, trash_path }) => {
                    let name = original_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| original_path.display().to_string());
                    self.undo_journal.record(
                        format!("trash {name}"),
                        ff_core::ops::journal::UndoAction::RestoreTrash {
                            original_path,
                            trash_path,
                        },
                    );
                    self.status_toast = Some((
                        format!("Moved to Trash: {name} (press 'u' to undo)"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
                Ok(FileOpOutcome::Renamed { from, to }) => {
                    let old_name = from
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| from.display().to_string());
                    let new_name = to
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| to.display().to_string());
                    self.undo_journal.record(
                        format!("rename {old_name} -> {new_name}"),
                        ff_core::ops::journal::UndoAction::Rename {
                            current: to,
                            original: from,
                        },
                    );
                    self.anchor = Some(new_name.clone());
                    self.status_toast = Some((
                        format!("Renamed: {old_name} -> {new_name} (press 'u' to undo)"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
                Ok(FileOpOutcome::CreatedFile { path }) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    self.undo_journal.record(
                        format!("create {name}"),
                        ff_core::ops::journal::UndoAction::DeleteCreatedFile { path },
                    );
                    self.anchor = Some(name.clone());
                    self.status_toast = Some((
                        format!("Created file: {name} (press 'u' to undo)"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
                Ok(FileOpOutcome::CreatedDir { path }) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    self.undo_journal.record(
                        format!("create folder {name}"),
                        ff_core::ops::journal::UndoAction::DeleteCreatedDir { path },
                    );
                    self.anchor = Some(name.clone());
                    self.status_toast = Some((
                        format!("Created folder: {name} (press 'u' to undo)"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
                Ok(FileOpOutcome::RestoredTrash { path }) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    self.anchor = Some(name.clone());
                    self.status_toast = Some((
                        format!("Undone: restored {name}"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
                Ok(FileOpOutcome::UndoneRename { to, .. }) => {
                    let name = to
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| to.display().to_string());
                    self.anchor = Some(name.clone());
                    self.status_toast = Some((
                        format!("Undone: renamed back to {name}"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
                Ok(FileOpOutcome::UndoneDelete { path }) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    self.status_toast = Some((
                        format!("Undone: removed {name}"),
                        Instant::now() + Duration::from_secs(4),
                    ));
                }
                Err(err) => {
                    self.status_toast = Some((
                        format!("Error: {err}"),
                        Instant::now() + Duration::from_secs(5),
                    ));
                }
            }
        }

        // 2. First arrival of the restored (or default home) state: switch
        //    to that directory instantly, within this same frame.
        if let Some(state) = self.scanner.take_restored() {
            self.apply_restore(state);
            listing = self.scanner.poll(&self.cwd);
        }
        self.entries = listing.entries;
        self.listing_loading = !listing.complete;

        if !self.cwd.as_os_str().is_empty() && self.last_visited_cwd.as_ref() != Some(&self.cwd) {
            session::record_visit(&mut self.frequents, &self.cwd, session::now_epoch_secs());
            self.last_visited_cwd = Some(self.cwd.clone());
        }

        // 2. Recompute the filtered view, then re-seat the anchor.
        self.display = self.finder.filter(&self.entries, &self.filter);
        self.remap_selection();

        if self.preview_open && !self.frequents_open {
            let active_path = self.display.get(self.selection).map(|e| e.path.clone());
            let needs_request = active_path != self.peek_path
                || (self.peek_payload.is_none() && !self.peek_loading && active_path.is_some());
            if needs_request {
                self.peek_path = active_path.clone();
                self.peek_payload = None;
                self.peek_texture = None;
                if let Some(path) = active_path {
                    self.peek_loading = true;
                    self.scanner.request_peek(path);
                } else {
                    self.peek_loading = false;
                }
            }
        }

        // 3. Keys, before widgets so filter typing stays literal.
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let open_with_requested = ctx.input(|i| {
            (i.modifiers.command && i.key_pressed(Key::O))
                || (i.modifiers.alt && i.key_pressed(Key::Enter))
        });
        let frequents_toggle = ctx.input(|i| {
            ((i.modifiers.command || i.modifiers.ctrl) && i.modifiers.shift && i.key_pressed(Key::F))
                || ((i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(Key::U))
        });
        let launcher_toggle = ctx.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl)
                && !i.modifiers.shift
                && (i.key_pressed(Key::K) || i.key_pressed(Key::P))
        });
        let theme_picker_toggle = ctx.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl)
                && i.modifiers.shift
                && i.key_pressed(Key::P)
        });

        let mouse_back = ctx.input(|i| {
            i.pointer.button_clicked(PointerButton::Extra1)
                || i.pointer.button_pressed(PointerButton::Extra1)
        });
        let mouse_forward = ctx.input(|i| {
            let b = PointerButton::Extra2;
            i.pointer.button_clicked(b) || i.pointer.button_pressed(b)
        });

        let key_back = ctx.input(|i| {
            let cmd_or_ctrl = i.modifiers.command || i.modifiers.ctrl;
            (cmd_or_ctrl && (i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::OpenBracket)))
                || (i.modifiers.alt && i.key_pressed(Key::ArrowLeft))
        });
        let key_forward = ctx.input(|i| {
            let cmd_or_ctrl = i.modifiers.command || i.modifiers.ctrl;
            (cmd_or_ctrl && (i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::CloseBracket)))
                || (i.modifiers.alt && i.key_pressed(Key::ArrowRight))
        });

        let back_requested = mouse_back || (!self.filter_focused && key_back);
        let forward_requested = mouse_forward || (!self.filter_focused && key_forward);

        if back_requested {
            self.go_back();
        } else if forward_requested {
            self.go_forward();
        } else if frequents_toggle {
            self.frequents_open = !self.frequents_open;
            if self.frequents_open {
                self.frequents_selection = 0;
                self.frequents_filter.clear();
                self.frequents_focus_filter = true;
            }
        } else if theme_picker_toggle {
            if self.launcher.open && self.launcher.mode == crate::launcher::LauncherMode::Themes {
                if let Some(initial) = self.launcher.initial_theme.take() {
                    self.palette = crate::theme::palette_for(&initial, None);
                    crate::theme::apply(&ctx, self.palette);
                }
                self.launcher.open = false;
            } else {
                self.launcher.open_mode(crate::launcher::LauncherMode::Themes, Some(&self.config.theme));
            }
        } else if launcher_toggle {
            self.launcher.toggle();
        }

        let previous_frequents_selection = self.frequents_selection;
        let mut frequents_selection_moved = false;

        if self.frequents_open {
            let now = ff_core::session::now_epoch_secs();
            let ranked = ff_core::session::ranked_frequents(&self.frequents, now);
            let q = self.frequents_filter.trim().to_lowercase();
            let filtered_len = ranked
                .iter()
                .filter(|f| q.is_empty() || f.path.to_string_lossy().to_lowercase().contains(&q))
                .count();

            if pressed(Key::Escape) {
                self.frequents_open = false;
            } else if pressed(Key::Enter) {
                let filtered: Vec<PathBuf> = ranked
                    .iter()
                    .filter(|f| q.is_empty() || f.path.to_string_lossy().to_lowercase().contains(&q))
                    .map(|f| f.path.clone())
                    .collect();
                if let Some(target) = filtered.get(self.frequents_selection) {
                    let target = target.clone();
                    self.frequents_open = false;
                    self.navigate_to(target);
                }
            } else if pressed(Key::ArrowDown) {
                if self.frequents_selection + 1 < filtered_len {
                    self.frequents_selection += 1;
                }
            } else if pressed(Key::ArrowUp) {
                if self.frequents_selection > 0 {
                    self.frequents_selection -= 1;
                }
            } else if pressed(Key::Home) {
                self.frequents_selection = 0;
            } else if pressed(Key::End) {
                self.frequents_selection = filtered_len.saturating_sub(1);
            } else if pressed(Key::PageDown) {
                self.frequents_selection = (self.frequents_selection + 10).min(filtered_len.saturating_sub(1));
            } else if pressed(Key::PageUp) {
                self.frequents_selection = self.frequents_selection.saturating_sub(10);
            }
            frequents_selection_moved = self.frequents_selection != previous_frequents_selection;
        } else if self.open_with_modal.is_none() && self.file_op_modal.is_none() && !self.launcher.open {
            let undo_requested = (!self.filter_focused && ctx.input(|i| !i.modifiers.any()) && pressed(Key::U))
                || ctx.input(|i| (i.modifiers.command || i.modifiers.ctrl) && !i.modifiers.shift && i.key_pressed(Key::Z));
            let trash_requested = ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Backspace))
                || (!self.filter_focused && (pressed(Key::Delete) || (ctx.input(|i| !i.modifiers.any()) && pressed(Key::X))));
            let rename_requested = (!self.filter_focused && ctx.input(|i| !i.modifiers.any()) && pressed(Key::R)) || pressed(Key::F2);
            let new_folder_requested = (!self.filter_focused && ctx.input(|i| i.modifiers.shift && !i.modifiers.command && !i.modifiers.ctrl && !i.modifiers.alt) && pressed(Key::A))
                || ctx.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.modifiers.shift && i.key_pressed(Key::N));
            let new_file_requested = (!self.filter_focused && ctx.input(|i| !i.modifiers.any()) && pressed(Key::A))
                || ctx.input(|i| (i.modifiers.command || i.modifiers.ctrl) && !i.modifiers.shift && i.key_pressed(Key::N));

            if open_with_requested {
                self.open_with_selected();
            } else if undo_requested {
                self.vim_count = None;
                self.vim_pending_g = false;
                if let Some(entry) = self.undo_journal.pop() {
                    self.status_toast = Some((
                        format!("Undoing: {}...", entry.description),
                        Instant::now() + Duration::from_secs(3),
                    ));
                    self.scanner.request_file_op(FileOpRequest::Undo(entry.action));
                } else {
                    self.status_toast = Some((
                        "Nothing to undo".to_string(),
                        Instant::now() + Duration::from_secs(2),
                    ));
                }
            } else if trash_requested {
                self.vim_count = None;
                self.vim_pending_g = false;
                if let Some(entry) = self.display.get(self.selection) {
                    let path = entry.path.clone();
                    self.scanner.request_file_op(FileOpRequest::Trash { path });
                }
            } else if rename_requested {
                self.vim_count = None;
                self.vim_pending_g = false;
                if let Some(entry) = self.display.get(self.selection) {
                    self.file_op_modal = Some(FileOpModal::Rename {
                        path: entry.path.clone(),
                        input: entry.name.clone(),
                        initial_focused: false,
                    });
                }
            } else if new_folder_requested {
                self.vim_count = None;
                self.vim_pending_g = false;
                self.file_op_modal = Some(FileOpModal::NewFolder {
                    parent: self.cwd.clone(),
                    input: String::new(),
                    initial_focused: false,
                });
            } else if new_file_requested {
                self.vim_count = None;
                self.vim_pending_g = false;
                self.file_op_modal = Some(FileOpModal::NewFile {
                    parent: self.cwd.clone(),
                    input: String::new(),
                    initial_focused: false,
                });
            } else if !self.filter_focused {
                let no_modifiers = !ctx.input(|i| i.modifiers.any());
                let shift_only = ctx.input(|i| {
                    i.modifiers.shift
                        && !i.modifiers.command
                        && !i.modifiers.ctrl
                        && !i.modifiers.alt
                });
                let ctrl_only = ctx.input(|i| {
                    i.modifiers.ctrl
                        && !i.modifiers.command
                        && !i.modifiers.shift
                        && !i.modifiers.alt
                });

                let digit = if pressed(Key::Num1) {
                    Some(1)
                } else if pressed(Key::Num2) {
                    Some(2)
                } else if pressed(Key::Num3) {
                    Some(3)
                } else if pressed(Key::Num4) {
                    Some(4)
                } else if pressed(Key::Num5) {
                    Some(5)
                } else if pressed(Key::Num6) {
                    Some(6)
                } else if pressed(Key::Num7) {
                    Some(7)
                } else if pressed(Key::Num8) {
                    Some(8)
                } else if pressed(Key::Num9) {
                    Some(9)
                } else if pressed(Key::Num0) && self.vim_count.is_some() {
                    Some(0)
                } else {
                    None
                };

                if no_modifiers && let Some(d) = digit {
                    let current = self.vim_count.unwrap_or(0);
                    self.vim_count = Some((current * 10 + d).min(999_999));
                    self.vim_pending_g = false;
                } else if no_modifiers && pressed(Key::Num0) {
                    self.vim_pending_g = false;
                    if !self.display.is_empty() {
                        self.selection = 0;
                        self.pin_anchor();
                    }
                } else if pressed(Key::Slash) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                    self.filter_focus_request = true;
                } else if pressed(Key::Space) {
                    self.preview_open = !self.preview_open;
                    if self.preview_open {
                        self.peek_path = None;
                        self.peek_payload = None;
                        self.peek_texture = None;
                    }
                    self.vim_count = None;
                    self.vim_pending_g = false;
                } else if pressed(Key::ArrowUp) || (no_modifiers && pressed(Key::K)) {
                    self.vim_pending_g = false;
                    let count = self.vim_count.take().unwrap_or(1);
                    self.selection = self.selection.saturating_sub(count);
                    self.pin_anchor();
                } else if pressed(Key::ArrowDown) || (no_modifiers && pressed(Key::J)) {
                    self.vim_pending_g = false;
                    let count = self.vim_count.take().unwrap_or(1);
                    if !self.display.is_empty() {
                        self.selection = (self.selection + count).min(self.display.len() - 1);
                        self.pin_anchor();
                    }
                } else if no_modifiers && pressed(Key::H) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                    self.go_back();
                } else if no_modifiers && pressed(Key::L) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                    self.enter_selected();
                } else if no_modifiers && pressed(Key::G) {
                    if self.vim_pending_g {
                        self.vim_pending_g = false;
                        let count = self.vim_count.take();
                        if !self.display.is_empty() {
                            self.selection = match count {
                                Some(c) => c.saturating_sub(1).min(self.display.len() - 1),
                                None => 0,
                            };
                            self.pin_anchor();
                        }
                    } else {
                        self.vim_pending_g = true;
                    }
                } else if shift_only && pressed(Key::G) {
                    self.vim_pending_g = false;
                    let count = self.vim_count.take();
                    if !self.display.is_empty() {
                        self.selection = match count {
                            Some(c) => c.saturating_sub(1).min(self.display.len() - 1),
                            None => self.display.len() - 1,
                        };
                        self.pin_anchor();
                    }
                } else if shift_only && pressed(Key::Num4) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                    if !self.display.is_empty() {
                        self.selection = self.display.len() - 1;
                        self.pin_anchor();
                    }
                } else if ctrl_only && pressed(Key::D) {
                    self.vim_pending_g = false;
                    let count = self.vim_count.take().unwrap_or(1);
                    if !self.display.is_empty() {
                        self.selection = (self.selection + count * 15).min(self.display.len() - 1);
                        self.pin_anchor();
                    }
                } else if pressed(Key::Home) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                    if !self.display.is_empty() {
                        self.selection = 0;
                        self.pin_anchor();
                    }
                } else if pressed(Key::End) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                    if !self.display.is_empty() {
                        self.selection = self.display.len() - 1;
                        self.pin_anchor();
                    }
                } else if pressed(Key::PageUp) {
                    self.vim_pending_g = false;
                    let count = self.vim_count.take().unwrap_or(1);
                    self.selection = self.selection.saturating_sub(count * 15);
                    self.pin_anchor();
                } else if pressed(Key::PageDown) {
                    self.vim_pending_g = false;
                    let count = self.vim_count.take().unwrap_or(1);
                    if !self.display.is_empty() {
                        self.selection = (self.selection + count * 15).min(self.display.len() - 1);
                        self.pin_anchor();
                    }
                } else if pressed(Key::Enter) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                    self.enter_selected();
                } else if pressed(Key::Backspace) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                    self.parent();
                } else if pressed(Key::Escape) {
                    self.vim_count = None;
                    self.vim_pending_g = false;
                }
            } else if pressed(Key::Escape) {
                self.filter.clear(); // panel auto-hides, focus drops next frame
                self.pending_scroll = true;
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
            frequents: self.frequents.clone(),
            theme: Some(self.config.theme.clone()),
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

                if self.frequents_open {
                    painter.circle_filled(egui::pos2(rect.min.x + 12.0, center_y), 3.0, self.palette.green);
                    painter.text(
                        egui::pos2(rect.min.x + 22.0, center_y),
                        egui::Align2::LEFT_CENTER,
                        format!("FREQUENTS  |  {} folders", self.frequents.len()),
                        egui::FontId::monospace(11.0),
                        self.palette.muted,
                    );
                    painter.text(
                        egui::pos2(rect.max.x - 12.0, center_y),
                        egui::Align2::RIGHT_CENTER,
                        "Enter: open  |  Esc: return",
                        egui::FontId::monospace(11.0),
                        self.palette.muted,
                    );
                    return;
                }

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

                let mut vim_cmd = String::new();
                if let Some(c) = self.vim_count {
                    vim_cmd.push_str(&c.to_string());
                }
                if self.vim_pending_g {
                    vim_cmd.push('g');
                }

                if !vim_cmd.is_empty() {
                    painter.text(
                        egui::pos2(rect.max.x - 24.0, center_y),
                        egui::Align2::RIGHT_CENTER,
                        vim_cmd,
                        egui::FontId::monospace(11.0),
                        self.palette.accent,
                    );
                } else if let Some(git) = self.scanner.git_status() {
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
        if !self.frequents_open && (self.filter_focus_request || self.filter_focused || !self.filter.is_empty()) {
            Panel::bottom("filter")
                .show_separator_line(false)
                .show(ui, |ui| {
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.filter)
                            .desired_width(f32::INFINITY)
                            .hint_text("/  type to filter  (esc to clear)"),
                    );
                    if resp.changed() {
                        self.pending_scroll = true;
                    }
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

        // --- right: Peek preview pane (PLAN §2, §5 it25)
        if self.preview_open && !self.frequents_open {
            let border_color = self.palette.border;
            Panel::right("peek_pane")
                .resizable(true)
                .default_size(360.0)
                .min_size(220.0)
                .show_separator_line(false)
                .frame(egui::Frame::NONE.fill(self.palette.bg))
                .show(ui, |ui| {
                    let rect = ui.max_rect();
                    if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                        self.pointer_over_interactive |= rect.contains(pos);
                    }
                    ui.painter().line_segment(
                        [rect.left_top(), rect.left_bottom()],
                        egui::Stroke::new(1.0, border_color),
                    );

                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 12,
                            top: 12,
                            bottom: 8,
                        })
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                            if let Some(path) = self.peek_path.as_ref() {
                                let view = crate::peek::PeekView {
                                    path,
                                    payload: self.peek_payload.as_ref(),
                                    loading: self.peek_loading,
                                    texture: self.peek_texture.as_ref(),
                                    palette: &self.palette,
                                    theme_name: &self.config.theme,
                                    syntax_enabled: self.config.preview_syntax,
                                };
                                view.show(ui);
                            } else {
                                ui.add_space(24.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("Select an item to preview")
                                            .color(self.palette.muted)
                                            .size(12.0),
                                    );
                                });
                            }
                        });
                });
        }

        CentralPanel::default().show(ui, |ui| {
            if self.frequents_open {
                self.render_frequents_view(ui, frequents_selection_moved);
                return;
            }

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

            let scroll_out = ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, row_height, total_rows, |ui, row_range| {
                    if (selection_moved || self.pending_scroll) && self.selection < total_rows {
                        let row_height_with_spacing = row_height + ui.spacing().item_spacing.y;
                        let target_top = ui.max_rect().top()
                            + (self.selection as f32 - row_range.start as f32) * row_height_with_spacing;
                        let target_rect = egui::Rect::from_min_size(
                            egui::pos2(ui.max_rect().min.x, target_top),
                            egui::vec2(ui.available_width(), row_height),
                        );
                        ui.scroll_to_rect_animation(
                            target_rect,
                            None,
                            egui::style::ScrollAnimation::none(),
                        );
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
                            if selection_moved || self.pending_scroll {
                                resp.scroll_to_me(None);
                            }
                            ui.painter().rect_filled(rect, 0.0, self.palette.sel);
                        }

                        let width = rect.width();
                        let (show_date, show_size, right_margin) = if width >= 440.0 {
                            (true, true, 210.0)
                        } else if width >= 280.0 {
                            (false, true, 76.0)
                        } else {
                            (false, false, 8.0)
                        };

                        let max_name_w = (width - right_margin - 12.0).max(20.0);
                        let char_budget = (max_name_w / 7.2) as usize;
                        let display_name = if entry.name.chars().count() > char_budget {
                            truncate_middle(&entry.name, char_budget.max(6))
                        } else {
                            entry.name.clone()
                        };

                        let clip_rect = egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(rect.min.x + 8.0 + max_name_w, rect.max.y),
                        );

                        let name_color = if entry.is_dir() {
                            self.palette.accent
                        } else {
                            self.palette.fg
                        };
                        ui.painter().with_clip_rect(clip_rect).text(
                            egui::pos2(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            display_name,
                            egui::FontId::monospace(12.5),
                            name_color,
                        );

                        if show_size && !entry.is_dir() {
                            let size_str = if let Some(stats) = self.stat_cache.get(&entry.path) {
                                stats.format_size()
                            } else {
                                String::new()
                            };
                            let size_x = if show_date {
                                rect.max.x - 130.0
                            } else {
                                rect.max.x - 12.0
                            };
                            ui.painter().text(
                                egui::pos2(size_x, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                size_str,
                                egui::FontId::monospace(11.0),
                                self.palette.muted,
                            );
                        }

                        if show_date {
                            let date_str = if let Some(stats) = self.stat_cache.get(&entry.path) {
                                stats.format_mtime()
                            } else {
                                String::new()
                            };
                            ui.painter().text(
                                egui::pos2(rect.max.x - 12.0, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                date_str,
                                egui::FontId::monospace(11.0),
                                self.palette.muted,
                            );
                        }

                        if resp.clicked() {
                            clicked = Some(i);
                        }
                        if resp.double_clicked() {
                            double_clicked = Some(i);
                        }
                    }
                });

            let scrollbar_width = ui.spacing().scroll.bar_width + 10.0;
            let scrollbar_rect = egui::Rect::from_min_max(
                egui::pos2(scroll_out.inner_rect.right() - scrollbar_width, scroll_out.inner_rect.top()),
                scroll_out.inner_rect.right_bottom(),
            );
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                self.pointer_over_interactive |= scrollbar_rect.contains(pos);
            }

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
                                        resp.scroll_to_me(None);
                                        ui.painter().rect_filled(rect, 4.0, self.palette.sel);
                                    }

                                    ui.painter().text(
                                        egui::pos2(rect.min.x + 8.0, rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        &app.name,
                                        egui::FontId::monospace(12.0),
                                        self.palette.fg,
                                    );

                                    if app.is_default {
                                        ui.painter().text(
                                            egui::pos2(rect.max.x - 8.0, rect.center().y),
                                            egui::Align2::RIGHT_CENTER,
                                            "(default)",
                                            egui::FontId::monospace(10.5),
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

        // --- modal palette: File operations (Rename, New File, New Folder)
        if let Some(mut modal) = self.file_op_modal.take() {
            let mut close = false;
            let mut submit = false;

            let esc_pressed = ctx.input(|i| i.key_pressed(Key::Escape));
            let enter_pressed = ctx.input(|i| i.key_pressed(Key::Enter));
            if esc_pressed {
                close = true;
            } else if enter_pressed {
                submit = true;
            }

            let border_color = self.palette.border;
            let (title, placeholder) = match &modal {
                FileOpModal::Rename { .. } => ("Rename", "New name"),
                FileOpModal::NewFile { .. } => ("New File", "Filename"),
                FileOpModal::NewFolder { .. } => ("New Folder", "Folder name"),
            };

            egui::Window::new("file_op_modal")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .fixed_size(egui::vec2(360.0, 130.0))
                .frame(
                    egui::Frame::NONE
                        .fill(self.palette.surface)
                        .stroke(egui::Stroke::new(1.0, border_color))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(16, 14)),
                )
                .show(&ctx, |ui| {
                    self.pointer_over_interactive = true;
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 10.0);

                    ui.label(
                        RichText::new(title)
                            .color(self.palette.fg)
                            .size(14.0)
                            .strong(),
                    );

                    let (input_ref, initial_focused) = match &mut modal {
                        FileOpModal::Rename { input, initial_focused, .. } => (input, initial_focused),
                        FileOpModal::NewFile { input, initial_focused, .. } => (input, initial_focused),
                        FileOpModal::NewFolder { input, initial_focused, .. } => (input, initial_focused),
                    };

                    let edit = egui::TextEdit::singleline(input_ref)
                        .hint_text(placeholder)
                        .font(egui::FontId::monospace(13.0))
                        .text_color(self.palette.fg)
                        .margin(egui::Margin::symmetric(8, 6));

                    let resp = ui.add(edit);
                    if !*initial_focused {
                        resp.request_focus();
                        *initial_focused = true;
                    }

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let confirm_btn = egui::Button::new(
                                RichText::new("Confirm")
                                    .color(self.palette.bg)
                                    .size(12.0)
                                    .strong(),
                            )
                            .fill(self.palette.accent)
                            .corner_radius(5);
                            if ui.add(confirm_btn).clicked() {
                                submit = true;
                            }

                            let cancel_btn = egui::Button::new(
                                RichText::new("Cancel")
                                    .color(self.palette.muted)
                                    .size(12.0),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .corner_radius(5);
                            if ui.add(cancel_btn).clicked() {
                                close = true;
                            }
                        });
                    });
                });

            if submit {
                let trimmed = match &modal {
                    FileOpModal::Rename { input, .. } => input.trim().to_string(),
                    FileOpModal::NewFile { input, .. } => input.trim().to_string(),
                    FileOpModal::NewFolder { input, .. } => input.trim().to_string(),
                };
                if !trimmed.is_empty() {
                    match modal {
                        FileOpModal::Rename { path, .. } => {
                            let new_path = path.parent().unwrap_or(Path::new("")).join(&trimmed);
                            if new_path != path {
                                self.scanner.request_file_op(FileOpRequest::Rename {
                                    from: path,
                                    to: new_path,
                                });
                            }
                        }
                        FileOpModal::NewFile { parent, .. } => {
                            let target = parent.join(&trimmed);
                            self.scanner.request_file_op(FileOpRequest::CreateFile {
                                path: target,
                            });
                        }
                        FileOpModal::NewFolder { parent, .. } => {
                            let target = parent.join(&trimmed);
                            self.scanner.request_file_op(FileOpRequest::CreateDir {
                                path: target,
                            });
                        }
                    }
                }
            } else if !close {
                self.file_op_modal = Some(modal);
            }
        }

        // --- Raycast-like command palette launcher & content search
        if self.launcher.open {
            self.pointer_over_interactive = true;
            let scanner = &self.scanner;
            let mut req_content = |dir: &Path, query: &str| -> u64 {
                scanner.request_content_search(dir.to_path_buf(), query.to_string(), 50)
            };
            let mut req_peek = |path: &Path| {
                scanner.request_peek(path.to_path_buf());
            };
            let mut previewed_theme_id: Option<&'static str> = None;
            let mut preview_theme = |theme_id: &'static str| {
                previewed_theme_id = Some(theme_id);
            };
            let outcome = self.launcher.render(
                &ctx,
                &self.cwd,
                &self.display,
                &self.palette,
                &self.config.theme,
                &mut req_content,
                &mut req_peek,
                &mut preview_theme,
            );
            if let Some(theme_id) = previewed_theme_id {
                let next_palette = crate::theme::palette_for(theme_id, None);
                if self.palette != next_palette {
                    self.palette = next_palette;
                    crate::theme::apply(&ctx, self.palette);
                }
            }
            match outcome {
                LauncherOutcome::ApplyTheme(theme_id) => {
                    self.config.theme = theme_id.to_string();
                    self.palette = crate::theme::palette_for(theme_id, None);
                    crate::theme::apply(&ctx, self.palette);
                    self.save_session();
                    self.status_toast = Some((
                        format!("Theme: {}", theme_id),
                        Instant::now() + Duration::from_secs(2),
                    ));
                }
                LauncherOutcome::OpenPath(path) => {
                    if path.is_dir() {
                        self.navigate_to(path);
                    } else if let Err(e) = ff_core::ops::open::open_default(&path) {
                        self.status_toast = Some((
                            format!("cannot open: {e}"),
                            Instant::now() + Duration::from_secs(4),
                        ));
                    }
                }
                LauncherOutcome::ExecuteAction(id) => match id {
                    "theme_picker" => {
                        self.launcher.open_mode(crate::launcher::LauncherMode::Themes, Some(&self.config.theme));
                    }
                    "new_file" => {
                        self.file_op_modal = Some(FileOpModal::NewFile {
                            parent: self.cwd.clone(),
                            input: String::new(),
                            initial_focused: false,
                        });
                    }
                    "new_folder" => {
                        self.file_op_modal = Some(FileOpModal::NewFolder {
                            parent: self.cwd.clone(),
                            input: String::new(),
                            initial_focused: false,
                        });
                    }
                    "open_with" => {
                        self.open_with_selected();
                    }
                    "trash" => {
                        if let Some(entry) = self.display.get(self.selection) {
                            let path = entry.path.clone();
                            self.scanner.request_file_op(FileOpRequest::Trash { path });
                        }
                    }
                    "undo" => {
                        if let Some(entry) = self.undo_journal.pop() {
                            self.status_toast = Some((
                                format!("Undoing: {}...", entry.description),
                                Instant::now() + Duration::from_secs(3),
                            ));
                            self.scanner.request_file_op(FileOpRequest::Undo(entry.action));
                        }
                    }
                    "frequents" => {
                        self.frequents_open = true;
                        self.frequents_selection = 0;
                        self.frequents_filter.clear();
                        self.frequents_focus_filter = true;
                    }
                    "toggle_peek" => {
                        self.preview_open = !self.preview_open;
                    }
                    "cycle_theme" => {
                        let all = crate::theme::all_themes();
                        let current_idx = all.iter().position(|t| t.id == self.config.theme).unwrap_or(0);
                        let next_idx = (current_idx + 1) % all.len();
                        let next = all[next_idx].id;
                        self.config.theme = next.to_string();
                        self.palette = crate::theme::palette_for(next, None);
                        crate::theme::apply(&ctx, self.palette);
                        self.save_session();
                        self.status_toast = Some((
                            format!("Theme: {}", next),
                            Instant::now() + Duration::from_secs(2),
                        ));
                    }
                    "copy_path" => {
                        if let Some(entry) = self.display.get(self.selection) {
                            ctx.copy_text(entry.path.to_string_lossy().to_string());
                            self.status_toast = Some((
                                "Copied path to clipboard".to_string(),
                                Instant::now() + Duration::from_secs(2),
                            ));
                        }
                    }
                    "go_home" => {
                        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                            self.navigate_to(home);
                        }
                    }
                    "quit" => {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    _ => {}
                },
                LauncherOutcome::Close => {
                    if let Some(initial) = self.launcher.initial_theme.take() {
                        self.palette = crate::theme::palette_for(&initial, None);
                        crate::theme::apply(&ctx, self.palette);
                    }
                    self.launcher.open = false;
                }
                LauncherOutcome::None => {}
            }
        }

        // --- frameless drag: press on any non-interactive space (PLAN §2)
        let press = ctx.input(|i| i.pointer.primary_pressed());
        if press && !self.pointer_over_interactive && !ctx.egui_is_using_pointer() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_middle() {
        assert_eq!(truncate_middle("short", 10), "short");
        assert_eq!(truncate_middle("hello world", 5), "h...d");
        assert_eq!(truncate_middle("abcdefghij", 7), "ab...ij");
    }

    #[test]
    fn test_apply_restore_modes() {
        let mut shell = FfShell {
            config: Config {
                launch: cfg::Launch::Frequents,
                ..Config::default()
            },
            config_warnings: Vec::new(),
            palette: crate::theme::palette_for("gruvbox-dark", None),
            scanner: FsEngine::launch(false),
            finder: Box::new(ff_core::find::PortableFilter),
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
            session_path: None,
            last_saved: None,
            pending_scroll: false,
            stat_cache: HashMap::new(),
            open_with_modal: None,
            status_toast: None,
            frequents: Vec::new(),
            frequents_open: false,
            frequents_selection: 0,
            frequents_filter: String::new(),
            frequents_focus_filter: false,
            last_visited_cwd: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            vim_count: None,
            vim_pending_g: false,
            preview_open: false,
            peek_path: None,
            peek_payload: None,
            peek_loading: false,
            peek_texture: None,
            undo_journal: ff_core::ops::journal::UndoJournal::default(),
            file_op_modal: None,
            launcher: LauncherState::default(),
        };

        let state = SessionState {
            cwd: PathBuf::from("/test/dir"),
            selection: Some("file.rs".into()),
            frequents: vec![ff_core::session::FrequentEntry {
                path: PathBuf::from("/test/dir"),
                count: 1,
                last_visited: 1000,
            }],
            theme: Some("gruvbox-dark".to_string()),
        };

        shell.apply_restore(state.clone());
        assert!(shell.frequents_open);
        assert_eq!(shell.frequents.len(), 1);

        shell.navigate_to(PathBuf::from("/new/path"));
        assert_eq!(shell.cwd, PathBuf::from("/new/path"));
        assert_eq!(shell.selection, 0);
        assert!(shell.pending_scroll);
    }

    #[test]
    fn test_navigation_history_back_forward() {
        let mut shell = FfShell {
            config: Config::default(),
            config_warnings: Vec::new(),
            palette: crate::theme::palette_for("gruvbox-dark", None),
            scanner: FsEngine::launch(false),
            finder: Box::new(ff_core::find::PortableFilter),
            cwd: PathBuf::from("/root/dir_a"),
            entries: Vec::new(),
            display: Vec::new(),
            anchor: Some("a.txt".into()),
            filter: String::new(),
            selection: 0,
            listing_loading: false,
            filter_focus_request: false,
            filter_focused: false,
            pointer_over_interactive: false,
            session_path: None,
            last_saved: None,
            pending_scroll: false,
            stat_cache: HashMap::new(),
            open_with_modal: None,
            status_toast: None,
            frequents: Vec::new(),
            frequents_open: false,
            frequents_selection: 0,
            frequents_filter: String::new(),
            frequents_focus_filter: false,
            last_visited_cwd: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            vim_count: None,
            vim_pending_g: false,
            preview_open: false,
            peek_path: None,
            peek_payload: None,
            peek_loading: false,
            peek_texture: None,
            undo_journal: ff_core::ops::journal::UndoJournal::default(),
            file_op_modal: None,
            launcher: LauncherState::default(),
        };

        // Navigate to dir_b
        shell.anchor = Some("file_in_a.txt".into());
        shell.navigate_to(PathBuf::from("/root/dir_b"));
        assert_eq!(shell.cwd, PathBuf::from("/root/dir_b"));
        assert_eq!(shell.history_back.len(), 1);
        assert_eq!(shell.history_back[0].0, PathBuf::from("/root/dir_a"));
        assert_eq!(shell.history_back[0].1, Some("file_in_a.txt".into()));

        // Navigate to dir_c
        shell.anchor = Some("file_in_b.txt".into());
        shell.navigate_to(PathBuf::from("/root/dir_c"));
        assert_eq!(shell.cwd, PathBuf::from("/root/dir_c"));
        assert_eq!(shell.history_back.len(), 2);

        // Go back -> returns to dir_b, restores anchor
        shell.go_back();
        assert_eq!(shell.cwd, PathBuf::from("/root/dir_b"));
        assert_eq!(shell.anchor, Some("file_in_b.txt".into()));
        assert_eq!(shell.history_forward.len(), 1);

        // Go back again -> returns to dir_a, restores anchor
        shell.go_back();
        assert_eq!(shell.cwd, PathBuf::from("/root/dir_a"));
        assert_eq!(shell.anchor, Some("file_in_a.txt".into()));
        assert_eq!(shell.history_forward.len(), 2);

        // Go forward -> returns to dir_b
        shell.go_forward();
        assert_eq!(shell.cwd, PathBuf::from("/root/dir_b"));
        assert_eq!(shell.anchor, Some("file_in_b.txt".into()));

        // Go forward -> returns to dir_c
        shell.go_forward();
        assert_eq!(shell.cwd, PathBuf::from("/root/dir_c"));

        // Frequents toggle + back closes frequents view
        shell.frequents_open = true;
        shell.go_back();
        assert!(!shell.frequents_open);
        assert_eq!(shell.cwd, PathBuf::from("/root/dir_c"));
    }

    #[test]
    fn test_vim_motions_with_count() {
        let mut shell = FfShell {
            config: Config::default(),
            config_warnings: Vec::new(),
            palette: crate::theme::palette_for("gruvbox-dark", None),
            scanner: FsEngine::launch(false),
            finder: Box::new(ff_core::find::PortableFilter),
            cwd: PathBuf::from("/root"),
            entries: Vec::new(),
            display: (0..25)
                .map(|i| Entry {
                    name: format!("file_{i}.txt"),
                    path: PathBuf::from(format!("/root/file_{i}.txt")),
                    kind: ff_core::model::EntryKind::File,
                })
                .collect(),
            anchor: None,
            filter: String::new(),
            selection: 0,
            listing_loading: false,
            filter_focus_request: false,
            filter_focused: false,
            pointer_over_interactive: false,
            session_path: None,
            last_saved: None,
            pending_scroll: false,
            stat_cache: HashMap::new(),
            open_with_modal: None,
            status_toast: None,
            frequents: Vec::new(),
            frequents_open: false,
            frequents_selection: 0,
            frequents_filter: String::new(),
            frequents_focus_filter: false,
            last_visited_cwd: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            vim_count: None,
            vim_pending_g: false,
            preview_open: false,
            peek_path: None,
            peek_payload: None,
            peek_loading: false,
            peek_texture: None,
            undo_journal: ff_core::ops::journal::UndoJournal::default(),
            file_op_modal: None,
            launcher: LauncherState::default(),
        };

        // 1. Initial selection at 0
        assert_eq!(shell.selection, 0);

        // 2. Count 5 + j moves down 5 lines
        shell.vim_count = Some(5);
        let count = shell.vim_count.take().unwrap_or(1);
        shell.selection = (shell.selection + count).min(shell.display.len() - 1);
        assert_eq!(shell.selection, 5);
        assert_eq!(shell.vim_count, None);

        // 3. Count 2 + k moves up 2 lines
        shell.vim_count = Some(2);
        let count = shell.vim_count.take().unwrap_or(1);
        shell.selection = shell.selection.saturating_sub(count);
        assert_eq!(shell.selection, 3);
        assert_eq!(shell.vim_count, None);

        // 4. Vim G without count jumps to bottom
        let count = shell.vim_count.take();
        shell.selection = match count {
            Some(c) => c.saturating_sub(1).min(shell.display.len() - 1),
            None => shell.display.len() - 1,
        };
        assert_eq!(shell.selection, 24);

        // 5. Vim gg without count jumps to top
        let count = shell.vim_count.take();
        shell.selection = match count {
            Some(c) => c.saturating_sub(1).min(shell.display.len() - 1),
            None => 0,
        };
        assert_eq!(shell.selection, 0);

        // 6. Vim 10gg jumps to line 10 (index 9)
        shell.vim_count = Some(10);
        let count = shell.vim_count.take();
        shell.selection = match count {
            Some(c) => c.saturating_sub(1).min(shell.display.len() - 1),
            None => 0,
        };
        assert_eq!(shell.selection, 9);
    }

    #[test]
    fn test_file_op_modal_and_undo_journal() {
        let mut shell = FfShell {
            config: Config::default(),
            config_warnings: Vec::new(),
            palette: crate::theme::palette_for("gruvbox-dark", None),
            scanner: FsEngine::launch(false),
            finder: Box::new(ff_core::find::PortableFilter),
            cwd: PathBuf::from("/test"),
            entries: Vec::new(),
            display: Vec::new(),
            anchor: None,
            filter: String::new(),
            selection: 0,
            listing_loading: false,
            filter_focus_request: false,
            filter_focused: false,
            pointer_over_interactive: false,
            session_path: None,
            last_saved: None,
            pending_scroll: false,
            stat_cache: HashMap::new(),
            open_with_modal: None,
            status_toast: None,
            frequents: Vec::new(),
            frequents_open: false,
            frequents_selection: 0,
            frequents_filter: String::new(),
            frequents_focus_filter: false,
            last_visited_cwd: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            vim_count: None,
            vim_pending_g: false,
            preview_open: false,
            peek_path: None,
            peek_payload: None,
            peek_loading: false,
            peek_texture: None,
            undo_journal: ff_core::ops::journal::UndoJournal::default(),
            file_op_modal: None,
            launcher: LauncherState::default(),
        };

        // 1. Initial journal is empty
        assert!(shell.undo_journal.is_empty());

        // 2. Open rename modal
        shell.file_op_modal = Some(FileOpModal::Rename {
            path: PathBuf::from("/test/old.txt"),
            input: "old.txt".into(),
            initial_focused: false,
        });
        assert!(shell.file_op_modal.is_some());

        // 3. Record undo action
        shell.undo_journal.record(
            "rename old.txt -> new.txt",
            ff_core::ops::journal::UndoAction::Rename {
                current: PathBuf::from("/test/new.txt"),
                original: PathBuf::from("/test/old.txt"),
            },
        );
        assert_eq!(shell.undo_journal.len(), 1);

        let undone = shell.undo_journal.pop().unwrap();
        assert_eq!(undone.description, "rename old.txt -> new.txt");
        assert!(shell.undo_journal.is_empty());
    }
}


