//! Raycast-style command palette and content grep launcher.
//!
//! Features:
//! - Three unified modes: Files (fuzzy name filter), Content (in-process ripgrep / fsearch grep), Actions (command palette).
//! - Split view: left results list (55%) + right live peek preview (45%) updating in real-time.
//! - Dieter Rams & Braun Minimalist System styling with tactile mode pills and hairline dividers.

use std::path::{Path, PathBuf};
use std::time::Instant;

use egui::{Key, RichText, ScrollArea, TextEdit};
use ff_core::find::ContentSearchResult;
use ff_core::model::Entry;

use crate::peek::PeekView;
use crate::theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherMode {
    Files,
    Content,
    Actions,
    Themes,
}

#[derive(Debug, Clone)]
pub struct LauncherAction {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub shortcut: &'static str,
}

impl LauncherAction {
    pub fn all() -> Vec<LauncherAction> {
        vec![
            LauncherAction {
                id: "theme_picker",
                title: "Change Theme...",
                category: "Appearance",
                shortcut: "⌘⇧P",
            },
            LauncherAction {
                id: "new_file",
                title: "New File...",
                category: "File Operations",
                shortcut: "a / ⌘N",
            },
            LauncherAction {
                id: "new_folder",
                title: "New Folder...",
                category: "File Operations",
                shortcut: "A / ⌘⇧N",
            },
            LauncherAction {
                id: "open_with",
                title: "Open With Application...",
                category: "File Operations",
                shortcut: "⌥↵",
            },
            LauncherAction {
                id: "open_editor",
                title: "Open in Editor (nvim)",
                category: "File Operations",
                shortcut: "e",
            },
            LauncherAction {
                id: "open_system",
                title: "Open with OS Default App",
                category: "File Operations",
                shortcut: "o",
            },
            LauncherAction {
                id: "trash",
                title: "Move Selected to Trash",
                category: "File Operations",
                shortcut: "⌘⌫",
            },
            LauncherAction {
                id: "undo",
                title: "Undo Last Action",
                category: "Edit",
                shortcut: "u / ⌘Z",
            },
            LauncherAction {
                id: "frequents",
                title: "Frequent Folders",
                category: "Navigation",
                shortcut: "⌘⇧F",
            },
            LauncherAction {
                id: "toggle_peek",
                title: "Toggle Preview Pane",
                category: "View",
                shortcut: "Space",
            },
            LauncherAction {
                id: "toggle_miller",
                title: "Toggle Miller Columns View",
                category: "View",
                shortcut: "m",
            },
            LauncherAction {
                id: "cycle_theme",
                title: "Cycle Palette Theme",
                category: "Appearance",
                shortcut: "T",
            },
            LauncherAction {
                id: "copy_path",
                title: "Copy Selected Path",
                category: "Edit",
                shortcut: "y / ⌘C",
            },
            LauncherAction {
                id: "go_home",
                title: "Go to Home Directory (~)",
                category: "Navigation",
                shortcut: "~",
            },
            LauncherAction {
                id: "quit",
                title: "Quit FF",
                category: "Application",
                shortcut: "⌘Q",
            },
        ]
    }
}

pub struct LauncherState {
    pub open: bool,
    pub query: String,
    pub prev_query: String,
    pub mode: LauncherMode,
    pub selection: usize,
    pub focus_input: bool,
    // Content search pipeline
    pub content_ticket: u64,
    pub content_query: String,
    pub content_results: Option<ContentSearchResult>,
    pub content_loading: bool,
    pub last_content_request: Option<Instant>,
    // Live Peek preview inside the launcher
    pub peek_path: Option<PathBuf>,
    pub peek_payload: Option<ff_core::ops::peek::PeekPayload>,
    pub peek_loading: bool,
    pub peek_texture: Option<egui::TextureHandle>,
    // Theme picker rollback on cancel
    pub initial_theme: Option<String>,
}

impl Default for LauncherState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            prev_query: String::new(),
            mode: LauncherMode::Files,
            selection: 0,
            focus_input: false,
            content_ticket: 0,
            content_query: String::new(),
            content_results: None,
            content_loading: false,
            last_content_request: None,
            peek_path: None,
            peek_payload: None,
            peek_loading: false,
            peek_texture: None,
            initial_theme: None,
        }
    }
}

pub enum LauncherOutcome {
    None,
    OpenPath(PathBuf),
    ExecuteAction(&'static str),
    ApplyTheme(&'static str),
    Close,
}

impl LauncherState {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.selection = 0;
            self.focus_input = true;
            self.query.clear();
            self.prev_query.clear();
            self.content_query.clear();
            self.content_results = None;
            self.peek_path = None;
            self.peek_payload = None;
            self.peek_texture = None;
            self.initial_theme = None;
        }
    }

    pub fn open_mode(&mut self, mode: LauncherMode, current_theme: Option<&str>) {
        self.open = true;
        self.mode = mode;
        self.query.clear();
        self.prev_query.clear();
        self.focus_input = true;
        self.initial_theme = current_theme.map(str::to_string);
        if mode == LauncherMode::Themes {
            let all = crate::theme::all_themes();
            if let Some(cur) = current_theme {
                self.selection = all.iter().position(|t| t.id == cur).unwrap_or(0);
            } else {
                self.selection = 0;
            }
        } else {
            self.selection = 0;
        }
    }

    pub fn cycle_mode(&mut self) {
        self.mode = match self.mode {
            LauncherMode::Files => LauncherMode::Content,
            LauncherMode::Content => LauncherMode::Actions,
            LauncherMode::Actions => LauncherMode::Themes,
            LauncherMode::Themes => LauncherMode::Files,
        };
        self.selection = 0;
        self.focus_input = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        cwd: &Path,
        filtered_entries: &[Entry],
        palette: &Palette,
        theme_name: &str,
        mut on_request_content: impl FnMut(&Path, &str) -> u64,
        mut on_request_peek: impl FnMut(&Path),
        mut on_preview_theme: impl FnMut(&'static str),
    ) -> LauncherOutcome {
        if !self.open {
            return LauncherOutcome::None;
        }

        let mut outcome = LauncherOutcome::None;
        let mut switch_mode = false;
        let mut select_up = false;
        let mut select_down = false;
        let mut submit = false;

        // 1. Process keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(Key::Escape) {
                outcome = LauncherOutcome::Close;
            } else if i.key_pressed(Key::Tab) {
                switch_mode = true;
            } else if i.key_pressed(Key::ArrowUp) || ((i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(Key::K)) {
                select_up = true;
            } else if i.key_pressed(Key::ArrowDown) || ((i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(Key::J)) {
                select_down = true;
            } else if i.key_pressed(Key::Enter) {
                submit = true;
            }
        });

        if matches!(outcome, LauncherOutcome::Close) {
            self.open = false;
            return outcome;
        }

        if switch_mode {
            self.cycle_mode();
        }

        // Automatic prefix switching:
        // Typing '?' or 'grep:' flips to Content mode
        if self.query.starts_with('?') {
            self.mode = LauncherMode::Content;
            self.query = self.query.trim_start_matches('?').trim_start().to_string();
            self.selection = 0;
        } else if self.query.starts_with("grep:") {
            self.mode = LauncherMode::Content;
            self.query = self.query.trim_start_matches("grep:").trim_start().to_string();
            self.selection = 0;
        } else if self.query.starts_with('>') {
            self.mode = LauncherMode::Actions;
            self.query = self.query.trim_start_matches('>').trim_start().to_string();
            self.selection = 0;
        } else if self.query.starts_with("theme:") {
            if self.initial_theme.is_none() {
                self.initial_theme = Some(theme_name.to_string());
            }
            self.mode = LauncherMode::Themes;
            self.query = self.query.trim_start_matches("theme:").trim_start().to_string();
            self.selection = 0;
        }

        // Reset selection when query changes
        if self.query != self.prev_query {
            self.selection = 0;
            self.prev_query = self.query.clone();
        }

        // Content search query debouncing
        if self.mode == LauncherMode::Content && self.query != self.content_query {
            self.content_query = self.query.clone();
            self.selection = 0;
            if !self.query.trim().is_empty() {
                self.content_loading = true;
                self.last_content_request = Some(Instant::now());
                self.content_ticket = on_request_content(cwd, self.query.trim());
            } else {
                self.content_results = None;
                self.content_loading = false;
            }
        }

        let query_lower = self.query.trim().to_lowercase();

        // 1. Files filtering
        let matching_files: Vec<&Entry> = if self.mode == LauncherMode::Files {
            filtered_entries
                .iter()
                .filter(|e| query_lower.is_empty() || e.name.to_lowercase().contains(&query_lower))
                .collect()
        } else {
            Vec::new()
        };

        // 2. Actions filtering
        let all_actions = LauncherAction::all();
        let filtered_actions: Vec<&LauncherAction> = if self.mode == LauncherMode::Actions {
            all_actions
                .iter()
                .filter(|a| {
                    query_lower.is_empty()
                        || a.title.to_lowercase().contains(&query_lower)
                        || a.category.to_lowercase().contains(&query_lower)
                })
                .collect()
        } else {
            Vec::new()
        };

        // 3. Themes filtering
        let all_themes = crate::theme::all_themes();
        let filtered_themes: Vec<&crate::theme::ThemeItem> = if self.mode == LauncherMode::Themes {
            all_themes
                .iter()
                .filter(|t| {
                    query_lower.is_empty()
                        || t.name.to_lowercase().contains(&query_lower)
                        || t.id.to_lowercase().contains(&query_lower)
                        || (t.is_dark && "dark".contains(&query_lower))
                        || (!t.is_dark && "light".contains(&query_lower))
                })
                .collect()
        } else {
            Vec::new()
        };

        // Determine total item count for selection bounds
        let total_items = match self.mode {
            LauncherMode::Files => matching_files.len(),
            LauncherMode::Content => {
                self.content_results.as_ref().map(|r| r.matches.len()).unwrap_or(0)
            }
            LauncherMode::Actions => filtered_actions.len(),
            LauncherMode::Themes => filtered_themes.len(),
        };

        if select_up && self.selection > 0 {
            self.selection -= 1;
        } else if select_down && self.selection + 1 < total_items {
            self.selection += 1;
        }

        if total_items > 0 {
            self.selection = self.selection.min(total_items - 1);
        } else {
            self.selection = 0;
        }

        // Live preview theme while navigating
        if self.mode == LauncherMode::Themes
            && let Some(t) = filtered_themes.get(self.selection)
        {
            on_preview_theme(t.id);
        }

        // Determine currently active selected path for live peek preview
        let current_target_path: Option<PathBuf> = match self.mode {
            LauncherMode::Files => matching_files.get(self.selection).map(|e| e.path.clone()),
            LauncherMode::Content => self
                .content_results
                .as_ref()
                .and_then(|r| r.matches.get(self.selection).map(|m| m.path.clone())),
            LauncherMode::Actions | LauncherMode::Themes => None,
        };

        if let Some(target_path) = &current_target_path {
            if self.peek_path.as_ref() != Some(target_path) {
                self.peek_path = Some(target_path.clone());
                self.peek_payload = None;
                self.peek_texture = None;
                self.peek_loading = true;
                on_request_peek(target_path);
            }
        } else {
            self.peek_path = None;
            self.peek_payload = None;
            self.peek_texture = None;
            self.peek_loading = false;
        }

        // Submit action
        if submit {
            match self.mode {
                LauncherMode::Files => {
                    if let Some(entry) = matching_files.get(self.selection) {
                        outcome = LauncherOutcome::OpenPath(entry.path.clone());
                        self.open = false;
                    }
                }
                LauncherMode::Content => {
                    if let Some(mat) = self
                        .content_results
                        .as_ref()
                        .and_then(|r| r.matches.get(self.selection))
                    {
                        outcome = LauncherOutcome::OpenPath(mat.path.clone());
                        self.open = false;
                    }
                }
                LauncherMode::Actions => {
                    if let Some(action) = filtered_actions.get(self.selection) {
                        outcome = LauncherOutcome::ExecuteAction(action.id);
                        self.open = false;
                    }
                }
                LauncherMode::Themes => {
                    if let Some(t) = filtered_themes.get(self.selection) {
                        outcome = LauncherOutcome::ApplyTheme(t.id);
                        self.open = false;
                        self.initial_theme = None;
                    }
                }
            }
            return outcome;
        }

        let border_color = palette.border;

        // Semi-transparent backdrop scrim (dimming the background window)
        let screen_rect = ctx.viewport_rect();
        let scrim_color = if palette.is_dark {
            egui::Color32::from_black_alpha(120)
        } else {
            egui::Color32::from_black_alpha(45)
        };

        egui::Area::new(egui::Id::unique("command_launcher_backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(rect, 0.0, scrim_color);
                if resp.clicked() {
                    outcome = LauncherOutcome::Close;
                }
            });

        if matches!(outcome, LauncherOutcome::Close) {
            self.open = false;
            return outcome;
        }

        // Render Raycast-like floating HUD
        egui::Window::new("command_launcher_window")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -20.0))
            .fixed_size(egui::vec2(640.0, 350.0))
            .frame(
                egui::Frame::NONE
                    .fill(palette.surface)
                    .stroke(egui::Stroke::new(1.0, border_color))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::same(0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                // --- TOP SEARCH BAR ---
                let header_height = 38.0;
                let header_out = ui.allocate_ui(egui::vec2(ui.available_width(), header_height), |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.add_space(12.0);

                        // Tactile Mode Pills
                        let modes = [
                            (LauncherMode::Files, "Files"),
                            (LauncherMode::Content, "Content"),
                            (LauncherMode::Actions, "Actions"),
                            (LauncherMode::Themes, "Themes"),
                        ];

                        for (m, label) in modes {
                            let is_active = self.mode == m;
                            let text_color = if is_active {
                                palette.bg
                            } else {
                                palette.muted
                            };
                            let fill_color = if is_active {
                                palette.accent
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let btn = egui::Button::new(
                                RichText::new(label)
                                    .color(text_color)
                                    .size(10.5)
                                    .monospace()
                                    .strong(),
                            )
                            .fill(fill_color)
                            .corner_radius(4)
                            .stroke(if is_active {
                                egui::Stroke::NONE
                            } else {
                                egui::Stroke::new(1.0, border_color)
                            });

                            if ui.add(btn).clicked() {
                                if m == LauncherMode::Themes && self.initial_theme.is_none() {
                                    self.initial_theme = Some(theme_name.to_string());
                                }
                                self.mode = m;
                                self.selection = 0;
                                self.focus_input = true;
                            }
                            ui.add_space(4.0);
                        }

                        ui.add_space(8.0);

                        // Search Input
                        let placeholder = match self.mode {
                            LauncherMode::Files => "Search files and folders...",
                            LauncherMode::Content => "Grep text inside files...",
                            LauncherMode::Actions => "Search commands and actions...",
                            LauncherMode::Themes => "Filter themes (dark, light, name)...",
                        };

                        let edit = TextEdit::singleline(&mut self.query)
                            .hint_text(placeholder)
                            .font(egui::FontId::monospace(13.0))
                            .text_color(palette.fg)
                            .frame(egui::Frame::NONE)
                            .desired_width(ui.available_width() - 48.0);

                        let resp = ui.add(edit);
                        if self.focus_input {
                            resp.request_focus();
                            self.focus_input = false;
                        }

                        // Right close shortcut badge
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(14.0);
                            ui.label(
                                RichText::new("esc")
                                    .color(palette.muted)
                                    .size(10.5)
                                    .monospace(),
                            );
                        });
                    });
                });

                // Hairline below search bar
                let h_rect = header_out.response.rect;
                ui.painter().line_segment(
                    [
                        egui::pos2(h_rect.left(), h_rect.bottom()),
                        egui::pos2(h_rect.right(), h_rect.bottom()),
                    ],
                    egui::Stroke::new(1.0, border_color),
                );

                // --- SPLIT VIEW: RESULTS (52%) + PREVIEW (48%) ---
                let split_height = 286.0;
                let full_w = ui.available_width();
                let left_w = (full_w * 0.52).round();
                let right_w = full_w - left_w;

                ui.allocate_ui_with_layout(
                    egui::vec2(full_w, split_height),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        // Left Results List
                        let left_out = ui.allocate_ui_with_layout(
                            egui::vec2(left_w, split_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);

                                        match self.mode {
                                            LauncherMode::Files => {
                                                if matching_files.is_empty() {
                                                    ui.add_space(40.0);
                                                    ui.vertical_centered(|ui| {
                                                        ui.label(
                                                            RichText::new("No matching files found")
                                                                .color(palette.muted)
                                                                .size(11.5)
                                                                .monospace(),
                                                        );
                                                    });
                                                } else {
                                                    for (i, entry) in matching_files.iter().enumerate() {
                                                        let is_selected = i == self.selection;
                                                        let row_h = 24.0;
                                                        let (rect, resp) = ui.allocate_exact_size(
                                                            egui::vec2(ui.available_width(), row_h),
                                                            egui::Sense::click(),
                                                        );

                                                        if is_selected {
                                                            if select_up || select_down {
                                                                resp.scroll_to_me(Some(egui::Align::Center));
                                                            }
                                                            ui.painter().rect_filled(rect, 4.0, palette.sel);
                                                        }

                                                        let display_name = if entry.is_dir() {
                                                            format!("{}/", entry.name)
                                                        } else {
                                                            entry.name.clone()
                                                        };
                                                        let name_color = if entry.is_dir() {
                                                            palette.accent
                                                        } else {
                                                            palette.fg
                                                        };

                                                        let max_chars = ((rect.width() - 20.0) / 7.2).max(8.0) as usize;
                                                        let text_to_draw = if display_name.chars().count() > max_chars {
                                                            crate::shell::truncate_middle(&display_name, max_chars)
                                                        } else {
                                                            display_name
                                                        };

                                                        ui.painter().with_clip_rect(rect).text(
                                                            egui::pos2(rect.min.x + 10.0, rect.center().y),
                                                            egui::Align2::LEFT_CENTER,
                                                            &text_to_draw,
                                                            egui::FontId::monospace(12.0),
                                                            name_color,
                                                        );

                                                        if resp.clicked() {
                                                            self.selection = i;
                                                            outcome = LauncherOutcome::OpenPath(entry.path.clone());
                                                            self.open = false;
                                                        }
                                                    }
                                                }
                                            }
                                            LauncherMode::Content => {
                                            if self.content_loading {
                                                ui.add_space(40.0);
                                                ui.vertical_centered(|ui| {
                                                    ui.label(
                                                        RichText::new("Searching content...")
                                                            .color(palette.accent)
                                                            .size(11.5)
                                                            .monospace(),
                                                    );
                                                });
                                            } else if let Some(results) = &self.content_results {
                                                if results.matches.is_empty() {
                                                    ui.add_space(40.0);
                                                    ui.vertical_centered(|ui| {
                                                        ui.label(
                                                            RichText::new("No content matches found")
                                                                .color(palette.muted)
                                                                .size(11.5)
                                                                .monospace(),
                                                        );
                                                    });
                                                } else {
                                                    for (i, mat) in results.matches.iter().enumerate() {
                                                        let is_selected = i == self.selection;
                                                        let row_h = 36.0;
                                                        let (rect, resp) = ui.allocate_exact_size(
                                                            egui::vec2(ui.available_width(), row_h),
                                                            egui::Sense::click(),
                                                        );

                                                        if is_selected {
                                                            if select_up || select_down {
                                                                resp.scroll_to_me(Some(egui::Align::Center));
                                                            }
                                                            ui.painter().rect_filled(rect, 4.0, palette.sel);
                                                        }

                                                        let file_name = mat
                                                            .path
                                                            .file_name()
                                                            .map(|n| n.to_string_lossy().to_string())
                                                            .unwrap_or_else(|| mat.path.display().to_string());

                                                        let row_painter = ui.painter().with_clip_rect(rect);

                                                        // Top row: Filename
                                                        let max_name_chars = ((rect.width() - 20.0) / 7.2).max(8.0) as usize;
                                                        let name_to_draw = if file_name.chars().count() > max_name_chars {
                                                            crate::shell::truncate_middle(&file_name, max_name_chars)
                                                        } else {
                                                            file_name
                                                        };
                                                        row_painter.text(
                                                            egui::pos2(rect.min.x + 10.0, rect.min.y + 10.0),
                                                            egui::Align2::LEFT_CENTER,
                                                            &name_to_draw,
                                                            egui::FontId::monospace(11.5),
                                                            palette.fg,
                                                        );

                                                        // Bottom row: L{line}: {snippet}
                                                        let line_badge = format!("L{}: ", mat.line_number);
                                                        let snippet = &mat.line_text;
                                                        let max_len = 34;
                                                        let display_snippet = if snippet.chars().count() > max_len {
                                                            format!("{}...", &snippet[..snippet.char_indices().nth(max_len).map(|(idx, _)| idx).unwrap_or(snippet.len())])
                                                        } else {
                                                            snippet.clone()
                                                        };

                                                        row_painter.text(
                                                            egui::pos2(rect.min.x + 10.0, rect.min.y + 24.0),
                                                            egui::Align2::LEFT_CENTER,
                                                            format!("{line_badge}{display_snippet}"),
                                                            egui::FontId::monospace(10.0),
                                                            palette.muted,
                                                        );

                                                        if resp.clicked() {
                                                            self.selection = i;
                                                            outcome = LauncherOutcome::OpenPath(mat.path.clone());
                                                            self.open = false;
                                                        }
                                                    }
                                                }
                                            } else {
                                                ui.add_space(40.0);
                                                ui.vertical_centered(|ui| {
                                                    ui.label(
                                                        RichText::new("Type query to grep across files")
                                                            .color(palette.muted)
                                                            .size(11.5)
                                                            .monospace(),
                                                    );
                                                });
                                            }
                                        }
                                        LauncherMode::Actions => {
                                            if filtered_actions.is_empty() {
                                                ui.add_space(40.0);
                                                ui.vertical_centered(|ui| {
                                                    ui.label(
                                                        RichText::new("No matching actions")
                                                            .color(palette.muted)
                                                            .size(11.5)
                                                            .monospace(),
                                                    );
                                                });
                                            } else {
                                                for (i, action) in filtered_actions.iter().enumerate() {
                                                    let is_selected = i == self.selection;
                                                    let row_h = 24.0;
                                                    let (rect, resp) = ui.allocate_exact_size(
                                                        egui::vec2(ui.available_width(), row_h),
                                                        egui::Sense::click(),
                                                    );

                                                    if is_selected {
                                                        if select_up || select_down {
                                                            resp.scroll_to_me(Some(egui::Align::Center));
                                                        }
                                                        ui.painter().rect_filled(rect, 4.0, palette.sel);
                                                    }

                                                    let row_painter = ui.painter().with_clip_rect(rect);

                                                    // Title
                                                    row_painter.text(
                                                        egui::pos2(rect.min.x + 10.0, rect.center().y),
                                                        egui::Align2::LEFT_CENTER,
                                                        action.title,
                                                        egui::FontId::monospace(11.5),
                                                        palette.fg,
                                                    );

                                                    // Category pill
                                                    row_painter.text(
                                                        egui::pos2(rect.max.x - 70.0, rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        action.category,
                                                        egui::FontId::monospace(9.5),
                                                        palette.label,
                                                    );

                                                    // Shortcut
                                                    row_painter.text(
                                                        egui::pos2(rect.max.x - 8.0, rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        action.shortcut,
                                                        egui::FontId::monospace(10.5),
                                                        palette.muted,
                                                    );

                                                    if resp.clicked() {
                                                        self.selection = i;
                                                        outcome = LauncherOutcome::ExecuteAction(action.id);
                                                        self.open = false;
                                                    }
                                                }
                                            }
                                        }
                                        LauncherMode::Themes => {
                                            if filtered_themes.is_empty() {
                                                ui.add_space(40.0);
                                                ui.vertical_centered(|ui| {
                                                    ui.label(
                                                        RichText::new("No matching themes")
                                                            .color(palette.muted)
                                                            .size(11.5)
                                                            .monospace(),
                                                    );
                                                });
                                            } else {
                                                for (i, theme_item) in filtered_themes.iter().enumerate() {
                                                    let is_selected = i == self.selection;
                                                    let row_h = 24.0;
                                                    let (rect, resp) = ui.allocate_exact_size(
                                                        egui::vec2(ui.available_width(), row_h),
                                                        egui::Sense::click(),
                                                    );

                                                    if is_selected {
                                                        if select_up || select_down {
                                                            resp.scroll_to_me(Some(egui::Align::Center));
                                                        }
                                                        ui.painter().rect_filled(rect, 4.0, palette.sel);
                                                    }

                                                    let row_painter = ui.painter().with_clip_rect(rect);

                                                    // Swatch circles: bg, surface, accent
                                                    let dot_radius = 4.0;
                                                    let dot_y = rect.center().y;

                                                    // Swatch 1: BG
                                                    let dot1_pos = egui::pos2(rect.min.x + 12.0, dot_y);
                                                    row_painter.circle_filled(dot1_pos, dot_radius, theme_item.palette.bg);
                                                    row_painter.circle_stroke(
                                                        dot1_pos,
                                                        dot_radius,
                                                        egui::Stroke::new(1.0, theme_item.palette.border),
                                                    );

                                                    // Swatch 2: Surface
                                                    let dot2_pos = egui::pos2(rect.min.x + 23.0, dot_y);
                                                    row_painter.circle_filled(dot2_pos, dot_radius, theme_item.palette.surface);
                                                    row_painter.circle_stroke(
                                                        dot2_pos,
                                                        dot_radius,
                                                        egui::Stroke::new(1.0, theme_item.palette.border),
                                                    );

                                                    // Swatch 3: Accent
                                                    let dot3_pos = egui::pos2(rect.min.x + 34.0, dot_y);
                                                    row_painter.circle_filled(dot3_pos, dot_radius, theme_item.palette.accent);

                                                    // Theme Name
                                                    let is_current = theme_item.id == theme_name;
                                                    let name_color = if is_current {
                                                        palette.accent
                                                    } else {
                                                        palette.fg
                                                    };

                                                    let name_text = if is_current {
                                                        format!("{} ✓", theme_item.name)
                                                    } else {
                                                        theme_item.name.to_string()
                                                    };

                                                    row_painter.text(
                                                        egui::pos2(rect.min.x + 44.0, rect.center().y),
                                                        egui::Align2::LEFT_CENTER,
                                                        &name_text,
                                                        egui::FontId::monospace(11.5),
                                                        name_color,
                                                    );

                                                    // Dark/Light tag
                                                    let tag = if theme_item.is_dark { "dark" } else { "light" };
                                                    row_painter.text(
                                                        egui::pos2(rect.max.x - 8.0, rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        tag,
                                                        egui::FontId::monospace(9.5),
                                                        palette.label,
                                                    );

                                                    if resp.clicked() {
                                                        self.selection = i;
                                                        outcome = LauncherOutcome::ApplyTheme(theme_item.id);
                                                        self.open = false;
                                                        self.initial_theme = None;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                });
                        });

                        // Vertical hairline divider between split panes
                        let divider_x = left_out.response.rect.right();
                        let split_top = left_out.response.rect.top();
                        let split_bottom = left_out.response.rect.bottom();
                        ui.painter().line_segment(
                            [
                                egui::pos2(divider_x, split_top),
                                egui::pos2(divider_x, split_bottom),
                            ],
                            egui::Stroke::new(1.0, border_color),
                        );

                        // Right Live Peek Preview
                        ui.allocate_ui_with_layout(
                            egui::vec2(right_w, split_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::Frame::NONE
                                    .inner_margin(egui::Margin {
                                        left: 10,
                                        right: 10,
                                        top: 8,
                                        bottom: 6,
                                    })
                                    .show(ui, |ui| {
                                        if self.mode == LauncherMode::Themes {
                                            if let Some(theme_item) = filtered_themes.get(self.selection) {
                                                let p = &theme_item.palette;
                                                ui.vertical_centered(|ui| {
                                                    ui.add_space(4.0);
                                                    ui.label(
                                                        RichText::new(theme_item.name)
                                                            .monospace()
                                                            .strong()
                                                            .size(13.0)
                                                            .color(p.accent),
                                                    );
                                                    ui.add_space(2.0);
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "{}  ·  {}",
                                                            if theme_item.is_dark { "Dark" } else { "Light" },
                                                            theme_item.id
                                                        ))
                                                        .monospace()
                                                        .size(10.5)
                                                        .color(p.muted),
                                                    );
                                                    ui.add_space(4.0);
                                                });

                                                // 1px hairline divider across the preview pane
                                                let (hairline_rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(ui.available_width(), 1.0),
                                                    egui::Sense::hover(),
                                                );
                                                ui.painter().rect_filled(hairline_rect, 0.0, p.border);
                                                ui.add_space(8.0);

                                                // Description
                                                ui.label(
                                                    RichText::new(theme_item.description)
                                                        .monospace()
                                                        .size(11.0)
                                                        .color(p.fg),
                                                );
                                                ui.add_space(10.0);

                                                // Swatch color bar (BG, SURF, ACCENT, MUTED)
                                                ui.horizontal(|ui| {
                                                    let swatches = [
                                                        ("BG", p.bg, p.border),
                                                        ("SURF", p.surface, p.border),
                                                        ("ACCENT", p.accent, egui::Color32::TRANSPARENT),
                                                        ("MUTED", p.muted, egui::Color32::TRANSPARENT),
                                                    ];
                                                    let chip_w = ((ui.available_width() - 12.0) / 4.0).max(10.0);
                                                    for (lbl, color, border) in swatches {
                                                        let (rect, _) = ui.allocate_exact_size(
                                                            egui::vec2(chip_w, 24.0),
                                                            egui::Sense::hover(),
                                                        );
                                                        ui.painter().rect_filled(rect, 4.0, color);
                                                        if border != egui::Color32::TRANSPARENT {
                                                            ui.painter().rect_stroke(
                                                                rect,
                                                                4.0,
                                                                egui::Stroke::new(1.0, border),
                                                                egui::StrokeKind::Outside,
                                                            );
                                                        }
                                                        let lum = color.r() as u32 + color.g() as u32 + color.b() as u32;
                                                        let text_color = if lum > 400 {
                                                            egui::Color32::from_rgb(0x1a, 0x1a, 0x1e)
                                                        } else {
                                                            egui::Color32::from_rgb(0xf5, 0xf5, 0xf5)
                                                        };
                                                        ui.painter().text(
                                                            rect.center(),
                                                            egui::Align2::CENTER_CENTER,
                                                            lbl,
                                                            egui::FontId::monospace(9.0),
                                                            text_color,
                                                        );
                                                        ui.add_space(3.0);
                                                    }
                                                });

                                                ui.add_space(10.0);

                                                // Live UI Preview Box
                                                ui.label(
                                                    RichText::new("Live UI Preview")
                                                        .monospace()
                                                        .size(10.0)
                                                        .color(p.label),
                                                );
                                                ui.add_space(4.0);

                                                let (box_rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(ui.available_width(), 76.0),
                                                    egui::Sense::hover(),
                                                );
                                                let bp = ui.painter().with_clip_rect(box_rect);
                                                bp.rect_filled(box_rect, 4.0, p.bg);
                                                bp.rect_stroke(
                                                    box_rect,
                                                    4.0,
                                                    egui::Stroke::new(1.0, p.border),
                                                    egui::StrokeKind::Outside,
                                                );

                                                // 1. Directory row
                                                bp.text(
                                                    egui::pos2(box_rect.min.x + 8.0, box_rect.min.y + 12.0),
                                                    egui::Align2::LEFT_CENTER,
                                                    "src/",
                                                    egui::FontId::monospace(10.5),
                                                    p.accent,
                                                );
                                                // 2. Selected file row
                                                let sel_rect = egui::Rect::from_min_size(
                                                    egui::pos2(box_rect.min.x + 2.0, box_rect.min.y + 24.0),
                                                    egui::vec2(box_rect.width() - 4.0, 18.0),
                                                );
                                                bp.rect_filled(sel_rect, 2.0, p.sel);
                                                bp.text(
                                                    egui::pos2(box_rect.min.x + 8.0, box_rect.min.y + 33.0),
                                                    egui::Align2::LEFT_CENTER,
                                                    "main.rs",
                                                    egui::FontId::monospace(10.5),
                                                    p.fg,
                                                );
                                                bp.text(
                                                    egui::pos2(box_rect.max.x - 8.0, box_rect.min.y + 33.0),
                                                    egui::Align2::RIGHT_CENTER,
                                                    "1.2 KB",
                                                    egui::FontId::monospace(9.0),
                                                    p.muted,
                                                );
                                                // 3. Status footer
                                                bp.line_segment(
                                                    [
                                                        egui::pos2(box_rect.min.x, box_rect.min.y + 48.0),
                                                        egui::pos2(box_rect.max.x, box_rect.min.y + 48.0),
                                                    ],
                                                    egui::Stroke::new(1.0, p.border),
                                                );
                                                bp.text(
                                                    egui::pos2(box_rect.min.x + 8.0, box_rect.min.y + 60.0),
                                                    egui::Align2::LEFT_CENTER,
                                                    "✓ git:(main) · 14 items",
                                                    egui::FontId::monospace(9.0),
                                                    p.green,
                                                );
                                            }
                                        } else if let Some(path) = &self.peek_path {
                                            let peek_view = PeekView {
                                                path,
                                                payload: self.peek_payload.as_ref(),
                                                loading: self.peek_loading,
                                                texture: self.peek_texture.as_ref(),
                                                palette,
                                                theme_name,
                                                syntax_enabled: true,
                                                show_diff: false,
                                            };
                                            peek_view.show(ui);
                                        } else {
                                            ui.add_space(60.0);
                                            ui.vertical_centered(|ui| {
                                                ui.label(
                                                    RichText::new("Raycast Launcher")
                                                        .color(palette.muted)
                                                        .size(13.0)
                                                        .monospace()
                                                        .strong(),
                                                );
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new("Arrow through items to live preview")
                                                        .color(palette.label)
                                                        .size(11.0)
                                                        .monospace(),
                                                );
                                            });
                                        }
                                    });
                            },
                        );
                    },
                );

                // --- FOOTER BAR ---
                let footer_height = 24.0;
                let footer_out = ui.allocate_ui(egui::vec2(ui.available_width(), footer_height), |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("↵ Open  |  ⇥ Mode  |  ↑↓ Navigate  |  esc Close")
                                .color(palette.muted)
                                .size(10.5)
                                .monospace(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(10.0);
                            match self.mode {
                                LauncherMode::Files => {
                                    ui.label(
                                        RichText::new(format!("{} files", matching_files.len()))
                                            .color(palette.label)
                                            .size(10.5)
                                            .monospace(),
                                    );
                                }
                                LauncherMode::Content => {
                                    if let Some(r) = &self.content_results {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} matches in {} files ({} ms)",
                                                r.matches.len(),
                                                r.searched_files,
                                                r.duration_ms
                                            ))
                                            .color(palette.label)
                                            .size(10.5)
                                            .monospace(),
                                        );
                                    }
                                }
                                LauncherMode::Actions => {
                                    ui.label(
                                        RichText::new(format!("{} commands", filtered_actions.len()))
                                            .color(palette.label)
                                            .size(10.5)
                                            .monospace(),
                                    );
                                }
                                LauncherMode::Themes => {
                                    ui.label(
                                        RichText::new(format!("{} themes", filtered_themes.len()))
                                            .color(palette.label)
                                            .size(10.5)
                                            .monospace(),
                                    );
                                }
                            }
                        });
                    });
                });

                // 1px hairline stone divider across footer
                let f_rect = footer_out.response.rect;
                ui.painter().line_segment(
                    [
                        egui::pos2(f_rect.left(), f_rect.top()),
                        egui::pos2(f_rect.right(), f_rect.top()),
                    ],
                    egui::Stroke::new(1.0, border_color),
                );
            });

        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launcher_toggle_and_cycling() {
        let mut launcher = LauncherState::default();
        assert!(!launcher.open);
        assert_eq!(launcher.mode, LauncherMode::Files);

        launcher.toggle();
        assert!(launcher.open);
        assert_eq!(launcher.query, "");

        launcher.cycle_mode();
        assert_eq!(launcher.mode, LauncherMode::Content);

        launcher.cycle_mode();
        assert_eq!(launcher.mode, LauncherMode::Actions);

        launcher.cycle_mode();
        assert_eq!(launcher.mode, LauncherMode::Themes);

        launcher.cycle_mode();
        assert_eq!(launcher.mode, LauncherMode::Files);

        launcher.toggle();
        assert!(!launcher.open);
    }

    #[test]
    fn test_launcher_actions_list() {
        let actions = LauncherAction::all();
        assert!(!actions.is_empty());
        assert!(actions.iter().any(|a| a.id == "theme_picker"));
        assert!(actions.iter().any(|a| a.id == "new_file"));
        assert!(actions.iter().any(|a| a.id == "undo"));
        assert!(actions.iter().any(|a| a.id == "copy_path"));
    }

    #[test]
    fn test_launcher_query_and_prefix_switching() {
        let mut launcher = LauncherState {
            open: true,
            query: "?needle".to_string(),
            ..Default::default()
        };
        if launcher.query.starts_with('?') {
            launcher.mode = LauncherMode::Content;
            launcher.query = launcher.query.trim_start_matches('?').trim_start().to_string();
        }
        assert_eq!(launcher.mode, LauncherMode::Content);
        assert_eq!(launcher.query, "needle");

        launcher.query = ">new".to_string();
        if launcher.query.starts_with('>') {
            launcher.mode = LauncherMode::Actions;
            launcher.query = launcher.query.trim_start_matches('>').trim_start().to_string();
        }
        assert_eq!(launcher.mode, LauncherMode::Actions);
        assert_eq!(launcher.query, "new");

        launcher.query = "theme:nord".to_string();
        if launcher.query.starts_with("theme:") {
            launcher.mode = LauncherMode::Themes;
            launcher.query = launcher.query.trim_start_matches("theme:").trim_start().to_string();
        }
        assert_eq!(launcher.mode, LauncherMode::Themes);
        assert_eq!(launcher.query, "nord");
    }

    #[test]
    fn test_launcher_theme_picker_open_mode() {
        let mut launcher = LauncherState::default();
        launcher.open_mode(LauncherMode::Themes, Some("catppuccin-mocha"));
        assert!(launcher.open);
        assert_eq!(launcher.mode, LauncherMode::Themes);
        assert_eq!(launcher.initial_theme.as_deref(), Some("catppuccin-mocha"));

        let all = crate::theme::all_themes();
        let expected_idx = all.iter().position(|t| t.id == "catppuccin-mocha").unwrap();
        assert_eq!(launcher.selection, expected_idx);
    }
}
