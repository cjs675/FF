//! Peek preview pane: lazy syntax-highlighted code, markdown rendering, image GPU textures, and directory summary.
//!
//! Follows Dieter Rams & Braun Minimalist System:
//! - 1px stone divider (`#E2DFD7` in Braun / `#3c3836` in Gruvbox)
//! - Zero cards / open borderless layout
//! - JetBrains Mono monospace typography
//! - Lazy syntect initialization with OnceLock (0 ms cold launch overhead)

use std::path::Path;
use std::sync::OnceLock;

use egui::{RichText, ScrollArea, TextFormat, Ui};
use egui::text::LayoutJob;
use ff_core::ops::peek::PeekPayload;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::theme::Palette;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

pub fn get_syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

pub fn get_theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn syntect_to_egui(c: syntect::highlighting::Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

pub struct PeekView<'a> {
    pub path: &'a Path,
    pub payload: Option<&'a PeekPayload>,
    pub loading: bool,
    pub texture: Option<&'a egui::TextureHandle>,
    pub palette: &'a Palette,
    pub theme_name: &'a str,
    pub syntax_enabled: bool,
}

impl<'a> PeekView<'a> {
    pub fn show(&self, ui: &mut Ui) {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.to_string_lossy().to_string());

        // Extract specs text (size, type, dimensions, lines)
        let (specs_text, truncated_hint) = if self.loading {
            ("Loading preview...".to_string(), None)
        } else if let Some(payload) = self.payload {
            match payload {
                PeekPayload::Empty => ("Empty file  ·  0 B".to_string(), None),
                PeekPayload::Directory { count, size, .. } => {
                    let size_str = ff_core::model::EntryStats {
                        size: *size,
                        mtime_secs: 0,
                    }
                    .format_size();
                    (format!("Directory  ·  {count} items  ·  {size_str}"), None)
                }
                PeekPayload::Image {
                    width,
                    height,
                    total_size,
                    ..
                } => {
                    let ext = self
                        .path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_uppercase();
                    let size_str = ff_core::model::EntryStats {
                        size: *total_size,
                        mtime_secs: 0,
                    }
                    .format_size();
                    (format!("{width} × {height} px  ·  {size_str}  ·  {ext}"), None)
                }
                PeekPayload::FileText {
                    content,
                    total_size,
                    truncated,
                } => {
                    let size_str = ff_core::model::EntryStats {
                        size: *total_size,
                        mtime_secs: 0,
                    }
                    .format_size();
                    let line_count = content.lines().count();
                    let ext = self
                        .path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let syntax_name = if matches!(ext.as_str(), "md" | "markdown" | "mdown" | "mkdn") {
                        "Markdown".to_string()
                    } else {
                        let ss = get_syntax_set();
                        ss.find_syntax_by_extension(&ext)
                            .map(|s| s.name.clone())
                            .unwrap_or_else(|| {
                                if ext.is_empty() {
                                    "Plain Text".to_string()
                                } else {
                                    ext.to_uppercase()
                                }
                            })
                    };
                    let hint = if *truncated {
                        Some("(head 128 KB)")
                    } else {
                        None
                    };
                    (
                        format!("{line_count} lines  ·  {size_str}  ·  {syntax_name}"),
                        hint,
                    )
                }
                PeekPayload::Binary { size } => {
                    let size_str = ff_core::model::EntryStats {
                        size: *size,
                        mtime_secs: 0,
                    }
                    .format_size();
                    (format!("Binary file  ·  {size_str}"), None)
                }
                PeekPayload::TooLarge { size } => {
                    let size_str = ff_core::model::EntryStats {
                        size: *size,
                        mtime_secs: 0,
                    }
                    .format_size();
                    (format!("Large file (> 10 MB)  ·  {size_str}"), None)
                }
                PeekPayload::Failed { error } => (format!("Cannot load  ·  {error}"), None),
            }
        } else {
            ("No selection".to_string(), None)
        };

        // 1. Centered Header: filename + specs (size, type, dimensions)
        ui.vertical_centered(|ui| {
            ui.add_space(2.0);
            ui.label(
                RichText::new(&name)
                    .monospace()
                    .strong()
                    .size(13.0)
                    .color(self.palette.accent),
            );
            ui.add_space(3.0);

            let mut header_specs = specs_text;
            if let Some(h) = truncated_hint {
                header_specs.push_str(&format!("  ·  {h}"));
            }
            ui.label(
                RichText::new(header_specs)
                    .monospace()
                    .size(11.0)
                    .color(self.palette.muted),
            );
            ui.add_space(6.0);
        });

        // 2. 1px stone divider line across the preview pane width
        let (hairline_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 1.0),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(hairline_rect, 0.0, self.palette.border);
        ui.add_space(10.0);

        if self.loading {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Loading preview...")
                        .monospace()
                        .color(self.palette.muted)
                        .size(12.0),
                );
            });
            return;
        }

        let Some(payload) = self.payload else {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Select an item to preview")
                        .monospace()
                        .color(self.palette.muted)
                        .size(12.0),
                );
            });
            return;
        };

        match payload {
            PeekPayload::Empty => {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Empty file")
                            .monospace()
                            .color(self.palette.muted)
                            .size(12.0),
                    );
                });
            }
            PeekPayload::Directory { mtime_secs, .. } => {
                let mtime_str = ff_core::model::EntryStats {
                    size: 0,
                    mtime_secs: *mtime_secs,
                }
                .format_mtime();
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(format!("Modified: {mtime_str}"))
                            .monospace()
                            .size(11.0)
                            .color(self.palette.muted),
                    );
                });
            }
            PeekPayload::Image {
                width,
                height,
                ..
            } => {
                if let Some(texture) = self.texture {
                    let avail_w = (ui.available_width() - 8.0).max(10.0);
                    let avail_h = (ui.available_height() - 16.0).max(10.0);
                    let aspect = *width as f32 / *height as f32;
                    let mut draw_w = avail_w.min(*width as f32);
                    let mut draw_h = draw_w / aspect;
                    if draw_h > avail_h {
                        draw_h = avail_h;
                        draw_w = draw_h * aspect;
                    }

                    ui.vertical_centered(|ui| {
                        ui.add_space(6.0);
                        let (rect, _resp) = ui.allocate_exact_size(
                            egui::vec2(draw_w.round(), draw_h.round()),
                            egui::Sense::hover(),
                        );
                        ui.painter().image(
                            texture.id(),
                            rect,
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            ),
                            egui::Color32::WHITE,
                        );
                        ui.painter().rect_stroke(
                            rect,
                            2.0,
                            egui::Stroke::new(1.0, self.palette.border),
                            egui::StrokeKind::Outside,
                        );
                    });
                }
            }
            PeekPayload::FileText { content, .. } => {
                let ext = self
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let is_markdown = matches!(ext.as_str(), "md" | "markdown" | "mdown" | "mkdn");

                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if is_markdown {
                            render_markdown(ui, content, self.palette);
                        } else {
                            let ss = get_syntax_set();
                            let ts = get_theme_set();

                            let syntax = ss
                                .find_syntax_by_extension(&ext)
                                .or_else(|| ss.find_syntax_by_first_line(content))
                                .unwrap_or_else(|| ss.find_syntax_plain_text());

                            let is_dark = self.theme_name == "gruvbox-dark";
                            let theme_key = if is_dark {
                                "base16-eighties.dark"
                            } else {
                                "InspiredGitHub"
                            };
                            let theme = &ts.themes[theme_key];

                            if self.syntax_enabled {
                                let mut highlighter = HighlightLines::new(syntax, theme);
                                for (idx, line) in LinesWithEndings::from(content).enumerate() {
                                    let line_num = idx + 1;
                                    let mut job = LayoutJob::default();
                                    job.append(
                                        &format!("{:>4} ", line_num),
                                        0.0,
                                        TextFormat {
                                            font_id: egui::FontId::monospace(11.0),
                                            color: self.palette.muted,
                                            ..Default::default()
                                        },
                                    );
                                    let ranges = highlighter
                                        .highlight_line(line, ss)
                                        .unwrap_or_default();
                                    for (style, text) in ranges {
                                        let clean_text = text.trim_end_matches(&['\r', '\n'][..]);
                                        job.append(
                                            clean_text,
                                            0.0,
                                            TextFormat {
                                                font_id: egui::FontId::monospace(11.0),
                                                color: syntect_to_egui(style.foreground),
                                                ..Default::default()
                                            },
                                        );
                                    }
                                    ui.label(job);
                                }
                            } else {
                                for (idx, line) in content.lines().enumerate() {
                                    let line_num = idx + 1;
                                    let mut job = LayoutJob::default();
                                    job.append(
                                        &format!("{:>4} ", line_num),
                                        0.0,
                                        TextFormat {
                                            font_id: egui::FontId::monospace(11.0),
                                            color: self.palette.muted,
                                            ..Default::default()
                                        },
                                    );
                                    job.append(
                                        line,
                                        0.0,
                                        TextFormat {
                                            font_id: egui::FontId::monospace(11.0),
                                            color: self.palette.fg,
                                            ..Default::default()
                                        },
                                    );
                                    ui.label(job);
                                }
                            }
                        }
                    });
            }
            PeekPayload::Binary { .. } => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Binary contents omitted")
                            .color(self.palette.muted)
                            .size(12.0),
                    );
                });
            }
            PeekPayload::TooLarge { .. } => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("File exceeds 10 MB preview limit")
                            .color(self.palette.accent)
                            .size(12.0),
                    );
                });
            }
            PeekPayload::Failed { error } => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Cannot load preview")
                            .strong()
                            .size(13.0)
                            .color(self.palette.accent),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(error)
                            .monospace()
                            .size(11.0)
                            .color(self.palette.muted),
                    );
                });
            }
        }
    }
}

/// Renders parsed Markdown with clean typography conforming to Dieter Rams & Braun Minimalist System.
pub fn render_markdown(ui: &mut Ui, content: &str, palette: &Palette) {
    use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

    let parser = Parser::new(content);

    let mut current_job = LayoutJob::default();
    current_job.wrap.max_width = ui.available_width();

    let mut is_heading = None;
    let mut is_bold = false;
    let mut is_italic = false;
    let mut is_code_block = false;
    let mut code_block_text = String::new();
    let mut list_number: Option<u64> = None;
    let mut is_blockquote = false;
    let mut in_list_item = false;

    let flush_job = |ui: &mut Ui, job: &mut LayoutJob, bottom_space: f32| {
        if !job.sections.is_empty() {
            ui.label(std::mem::take(job));
            job.wrap.max_width = ui.available_width();
            if bottom_space > 0.0 {
                ui.add_space(bottom_space);
            }
        }
    };

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush_job(ui, &mut current_job, 4.0);
                is_heading = Some(level);
                ui.add_space(8.0);
            }
            Event::End(TagEnd::Heading(_)) => {
                is_heading = None;
                flush_job(ui, &mut current_job, 6.0);
            }
            Event::Start(Tag::Paragraph) => {
                if !in_list_item {
                    flush_job(ui, &mut current_job, 0.0);
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if is_blockquote {
                    if !current_job.sections.is_empty() {
                        let job = std::mem::take(&mut current_job);
                        current_job.wrap.max_width = ui.available_width();
                        ui.horizontal(|ui| {
                            let (bar_rect, _) = ui.allocate_exact_size(
                                egui::vec2(2.0, 16.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(bar_rect, 1.0, palette.accent);
                            ui.add_space(6.0);
                            ui.label(job);
                        });
                        ui.add_space(4.0);
                    }
                } else if !in_list_item {
                    flush_job(ui, &mut current_job, 6.0);
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush_job(ui, &mut current_job, 2.0);
                is_blockquote = true;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                is_blockquote = false;
                flush_job(ui, &mut current_job, 4.0);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush_job(ui, &mut current_job, 4.0);
                is_code_block = true;
                code_block_text.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                is_code_block = false;
                let text = code_block_text.trim_end();
                if !text.is_empty() {
                    egui::Frame::NONE
                        .fill(palette.hover)
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 10,
                            top: 8,
                            bottom: 8,
                        })
                        .corner_radius(3.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(text)
                                    .monospace()
                                    .size(11.5)
                                    .color(palette.fg),
                            );
                        });
                    ui.add_space(6.0);
                }
                code_block_text.clear();
            }
            Event::Start(Tag::List(start)) => {
                flush_job(ui, &mut current_job, 2.0);
                list_number = start;
            }
            Event::End(TagEnd::List(_)) => {
                list_number = None;
                flush_job(ui, &mut current_job, 4.0);
            }
            Event::Start(Tag::Item) => {
                flush_job(ui, &mut current_job, 0.0);
                in_list_item = true;
                let bullet = match &mut list_number {
                    Some(num) => {
                        let b = format!(" {:>2}. ", num);
                        *num += 1;
                        b
                    }
                    None => "  • ".to_string(),
                };
                current_job.append(
                    &bullet,
                    0.0,
                    TextFormat {
                        font_id: egui::FontId::monospace(12.0),
                        color: palette.accent,
                        ..Default::default()
                    },
                );
            }
            Event::End(TagEnd::Item) => {
                in_list_item = false;
                flush_job(ui, &mut current_job, 4.0);
            }
            Event::Start(Tag::Strong) => is_bold = true,
            Event::End(TagEnd::Strong) => is_bold = false,
            Event::Start(Tag::Emphasis) => is_italic = true,
            Event::End(TagEnd::Emphasis) => is_italic = false,
            Event::Rule => {
                flush_job(ui, &mut current_job, 4.0);
                let (r, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(r, 0.0, palette.border);
                ui.add_space(8.0);
            }
            Event::SoftBreak => {
                current_job.append(
                    " ",
                    0.0,
                    TextFormat {
                        font_id: egui::FontId::proportional(13.0),
                        color: palette.fg,
                        line_height: Some(18.0),
                        ..Default::default()
                    },
                );
            }
            Event::HardBreak => {
                current_job.append(
                    "\n",
                    0.0,
                    TextFormat {
                        font_id: egui::FontId::proportional(13.0),
                        color: palette.fg,
                        line_height: Some(18.0),
                        ..Default::default()
                    },
                );
            }
            Event::Code(text) => {
                if is_code_block {
                    code_block_text.push_str(&text);
                } else {
                    current_job.append(
                        &format!(" `{text}` "),
                        0.0,
                        TextFormat {
                            font_id: egui::FontId::monospace(11.5),
                            color: palette.accent,
                            ..Default::default()
                        },
                    );
                }
            }
            Event::Text(text) => {
                if is_code_block {
                    code_block_text.push_str(&text);
                } else {
                    let (font_size, color) = if let Some(lvl) = is_heading {
                        match lvl {
                            HeadingLevel::H1 => (18.0, palette.accent),
                            HeadingLevel::H2 => (15.5, palette.fg),
                            HeadingLevel::H3 => (14.0, palette.fg),
                            _ => (13.0, palette.fg),
                        }
                    } else if is_bold {
                        (13.5, palette.accent)
                    } else if is_blockquote {
                        (12.0, palette.muted)
                    } else {
                        (13.0, palette.fg)
                    };

                    let line_height = Some(font_size * 1.35);
                    let font_id = egui::FontId::proportional(font_size);
                    current_job.append(
                        &text,
                        0.0,
                        TextFormat {
                            font_id,
                            color,
                            italics: is_italic || is_blockquote,
                            line_height,
                            ..Default::default()
                        },
                    );
                }
            }
            _ => {}
        }
    }

    flush_job(ui, &mut current_job, 4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown_parser_and_ui() {
        let md = "# Title\n\nSome **bold** and *italic* and `code`.\n\n- item 1\n- item 2\n\n```rust\nfn main() {}\n```\n\n> quote\n\n---";
        let palette = crate::theme::palette_for("gruvbox-dark", None);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                render_markdown(ui, md, &palette);
            });
        });
        output.textures_delta.clear();
    }
}
