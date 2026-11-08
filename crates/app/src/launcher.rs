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
}

impl Default for LauncherState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
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
        }
    }
}

pub enum LauncherOutcome {
    None,
    OpenPath(PathBuf),
    ExecuteAction(&'static str),
    Close,
}

impl LauncherState {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.selection = 0;
            self.focus_input = true;
            self.query.clear();
            self.content_query.clear();
            self.content_results = None;
            self.peek_path = None;
            self.peek_payload = None;
            self.peek_texture = None;
        }
    }

    pub fn cycle_mode(&mut self) {
        self.mode = match self.mode {
            LauncherMode::Files => LauncherMode::Content,
            LauncherMode::Content => LauncherMode::Actions,
            LauncherMode::Actions => LauncherMode::Files,
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

        let all_actions = LauncherAction::all();
        let query_lower = self.query.trim().to_lowercase();
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

        // Determine total item count for selection bounds
        let total_items = match self.mode {
            LauncherMode::Files => filtered_entries.len(),
            LauncherMode::Content => {
                self.content_results.as_ref().map(|r| r.matches.len()).unwrap_or(0)
            }
            LauncherMode::Actions => filtered_actions.len(),
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

        // Determine currently active selected path for live peek preview
        let current_target_path: Option<PathBuf> = match self.mode {
            LauncherMode::Files => filtered_entries.get(self.selection).map(|e| e.path.clone()),
            LauncherMode::Content => self
                .content_results
                .as_ref()
                .and_then(|r| r.matches.get(self.selection).map(|m| m.path.clone())),
            LauncherMode::Actions => None,
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
                    if let Some(entry) = filtered_entries.get(self.selection) {
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
            }
            return outcome;
        }

        let border_color = palette.border;

        // Render Raycast-like floating HUD
        egui::Window::new("command_launcher_window")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -20.0))
            .fixed_size(egui::vec2(720.0, 460.0))
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
                let header_height = 48.0;
                let header_out = ui.allocate_ui(egui::vec2(ui.available_width(), header_height), |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.add_space(12.0);

                        // Tactile Mode Pills
                        let modes = [
                            (LauncherMode::Files, "Files"),
                            (LauncherMode::Content, "Content"),
                            (LauncherMode::Actions, "Actions"),
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
                                    .size(11.0)
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
                        };

                        let edit = TextEdit::singleline(&mut self.query)
                            .hint_text(placeholder)
                            .font(egui::FontId::proportional(14.0))
                            .text_color(palette.fg)
                            .frame(egui::Frame::NONE)
                            .desired_width(ui.available_width() - 50.0);

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
                                    .size(11.0)
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

                // --- SPLIT VIEW: RESULTS (55%) + PREVIEW (45%) ---
                let split_height = 384.0;
                let full_w = ui.available_width();
                let left_w = (full_w * 0.55).round();
                let right_w = full_w - left_w;

                ui.allocate_ui(egui::vec2(full_w, split_height), |ui| {
                    ui.horizontal(|ui| {
                        // Left Results List
                        ui.allocate_ui(egui::vec2(left_w, split_height), |ui| {
                            ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);

                                    match self.mode {
                                        LauncherMode::Files => {
                                            if filtered_entries.is_empty() {
                                                ui.add_space(40.0);
                                                ui.vertical_centered(|ui| {
                                                    ui.label(
                                                        RichText::new("No matching files found")
                                                            .color(palette.muted)
                                                            .size(12.0),
                                                    );
                                                });
                                            } else {
                                                for (i, entry) in filtered_entries.iter().enumerate() {
                                                    let is_selected = i == self.selection;
                                                    let row_h = 32.0;
                                                    let (rect, resp) = ui.allocate_exact_size(
                                                        egui::vec2(ui.available_width(), row_h),
                                                        egui::Sense::click(),
                                                    );

                                                    if is_selected {
                                                        resp.scroll_to_me(None);
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

                                                    ui.painter().text(
                                                        egui::pos2(rect.min.x + 8.0, rect.center().y),
                                                        egui::Align2::LEFT_CENTER,
                                                        &display_name,
                                                        egui::FontId::proportional(12.5),
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
                                                            .size(12.0),
                                                    );
                                                });
                                            } else if let Some(results) = &self.content_results {
                                                if results.matches.is_empty() {
                                                    ui.add_space(40.0);
                                                    ui.vertical_centered(|ui| {
                                                        ui.label(
                                                            RichText::new("No content matches found")
                                                                .color(palette.muted)
                                                                .size(12.0),
                                                        );
                                                    });
                                                } else {
                                                    for (i, mat) in results.matches.iter().enumerate() {
                                                        let is_selected = i == self.selection;
                                                        let row_h = 44.0;
                                                        let (rect, resp) = ui.allocate_exact_size(
                                                            egui::vec2(ui.available_width(), row_h),
                                                            egui::Sense::click(),
                                                        );

                                                        if is_selected {
                                                            resp.scroll_to_me(None);
                                                            ui.painter().rect_filled(rect, 4.0, palette.sel);
                                                        }

                                                        let file_name = mat
                                                            .path
                                                            .file_name()
                                                            .map(|n| n.to_string_lossy().to_string())
                                                            .unwrap_or_else(|| mat.path.display().to_string());

                                                        // Top row: Filename
                                                        ui.painter().text(
                                                            egui::pos2(rect.min.x + 8.0, rect.min.y + 12.0),
                                                            egui::Align2::LEFT_CENTER,
                                                            &file_name,
                                                            egui::FontId::proportional(12.0),
                                                            palette.fg,
                                                        );

                                                        // Bottom row: L{line}: {snippet}
                                                        let line_badge = format!("L{}: ", mat.line_number);
                                                        let snippet = &mat.line_text;
                                                        let max_len = 40;
                                                        let display_snippet = if snippet.chars().count() > max_len {
                                                            format!("{}...", &snippet[..snippet.char_indices().nth(max_len).map(|(idx, _)| idx).unwrap_or(snippet.len())])
                                                        } else {
                                                            snippet.clone()
                                                        };

                                                        ui.painter().text(
                                                            egui::pos2(rect.min.x + 8.0, rect.min.y + 28.0),
                                                            egui::Align2::LEFT_CENTER,
                                                            format!("{line_badge}{display_snippet}"),
                                                            egui::FontId::monospace(10.5),
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
                                                            .size(12.0),
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
                                                            .size(12.0),
                                                    );
                                                });
                                            } else {
                                                for (i, action) in filtered_actions.iter().enumerate() {
                                                    let is_selected = i == self.selection;
                                                    let row_h = 32.0;
                                                    let (rect, resp) = ui.allocate_exact_size(
                                                        egui::vec2(ui.available_width(), row_h),
                                                        egui::Sense::click(),
                                                    );

                                                    if is_selected {
                                                        resp.scroll_to_me(None);
                                                        ui.painter().rect_filled(rect, 4.0, palette.sel);
                                                    }

                                                    // Title
                                                    ui.painter().text(
                                                        egui::pos2(rect.min.x + 10.0, rect.center().y),
                                                        egui::Align2::LEFT_CENTER,
                                                        action.title,
                                                        egui::FontId::proportional(12.5),
                                                        palette.fg,
                                                    );

                                                    // Category pill
                                                    ui.painter().text(
                                                        egui::pos2(rect.max.x - 70.0, rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        action.category,
                                                        egui::FontId::proportional(10.5),
                                                        palette.label,
                                                    );

                                                    // Shortcut
                                                    ui.painter().text(
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
                                    }
                                });
                        });

                        // Vertical hairline divider between split panes
                        let divider_x = ui.cursor().left();
                        ui.painter().line_segment(
                            [
                                egui::pos2(divider_x, ui.cursor().top()),
                                egui::pos2(divider_x, ui.cursor().top() + split_height),
                            ],
                            egui::Stroke::new(1.0, border_color),
                        );

                        // Right Live Peek Preview
                        ui.allocate_ui(egui::vec2(right_w, split_height), |ui| {
                            if let Some(path) = &self.peek_path {
                                let peek_view = PeekView {
                                    path,
                                    payload: self.peek_payload.as_ref(),
                                    loading: self.peek_loading,
                                    texture: self.peek_texture.as_ref(),
                                    palette,
                                    theme_name,
                                    syntax_enabled: true,
                                };
                                peek_view.show(ui);
                            } else {
                                ui.add_space(80.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("Raycast Launcher")
                                            .color(palette.muted)
                                            .size(14.0)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new("Arrow through items to live preview")
                                            .color(palette.label)
                                            .size(11.5),
                                    );
                                });
                            }
                        });
                    });
                });

                // --- FOOTER BAR ---
                let footer_height = 28.0;
                let footer_out = ui.allocate_ui(egui::vec2(ui.available_width(), footer_height), |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("↵ Open  |  ⇥ Mode  |  ↑↓ Navigate  |  esc Close")
                                .color(palette.muted)
                                .size(11.0)
                                .monospace(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(10.0);
                            match self.mode {
                                LauncherMode::Files => {
                                    ui.label(
                                        RichText::new(format!("{} files", filtered_entries.len()))
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
        assert_eq!(launcher.mode, LauncherMode::Files);

        launcher.toggle();
        assert!(!launcher.open);
    }

    #[test]
    fn test_launcher_actions_list() {
        let actions = LauncherAction::all();
        assert!(!actions.is_empty());
        assert!(actions.iter().any(|a| a.id == "new_file"));
        assert!(actions.iter().any(|a| a.id == "undo"));
        assert!(actions.iter().any(|a| a.id == "copy_path"));
    }
}
