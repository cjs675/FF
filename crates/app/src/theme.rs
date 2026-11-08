//! Token palettes (PLAN §2): the whole UI speaks through six tokens, never
//! raw colors elsewhere. gruvbox-dark ships; `theme = auto` and palette
//! files come with the v1 theming milestone.
//!
//! Fonts: deliberately untouched — font/icon sourcing is an open gate
//! (PLAN §7), so defaults remain until that decision.

use egui::{Color32, Context, Stroke, Style, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: Color32,
    pub hover: Color32,
    pub sel: Color32,
    pub fg: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub red: Color32,
    pub is_dark: bool,
}

pub const GRUVBOX_DARK: Palette = Palette {
    bg: Color32::from_rgb(0x28, 0x28, 0x28),
    hover: Color32::from_rgb(0x3c, 0x38, 0x36),
    sel: Color32::from_rgb(0x50, 0x49, 0x45),
    fg: Color32::from_rgb(0xeb, 0xdb, 0xb2),
    muted: Color32::from_rgb(0x92, 0x83, 0x74),
    accent: Color32::from_rgb(0xfa, 0xbd, 0x2f),
    red: Color32::from_rgb(0xfb, 0x49, 0x34),
    is_dark: true,
};

pub const BRAUN: Palette = Palette {
    bg: Color32::from_rgb(0xf5, 0xf4, 0xef),
    hover: Color32::from_rgb(0xea, 0xe8, 0xe1),
    sel: Color32::from_rgb(0xdf, 0xdd, 0xd4),
    fg: Color32::from_rgb(0x1a, 0x1a, 0x1e),
    muted: Color32::from_rgb(0x5c, 0x5b, 0x64),
    accent: Color32::from_rgb(0xe6, 0x51, 0x00),
    red: Color32::from_rgb(0xd9, 0x38, 0x1e),
    is_dark: false,
};

pub fn palette_for(name: &str) -> Palette {
    match name {
        "braun" => BRAUN,
        _ => GRUVBOX_DARK,
    }
}

#[allow(dead_code)]
pub const BG: Color32 = GRUVBOX_DARK.bg;
#[allow(dead_code)]
pub const HOVER: Color32 = GRUVBOX_DARK.hover;
#[allow(dead_code)]
pub const SEL: Color32 = GRUVBOX_DARK.sel;
#[allow(dead_code)]
pub const FG: Color32 = Color32::from_rgb(0xeb, 0xdb, 0xb2);
#[allow(dead_code)]
pub const MUTED: Color32 = GRUVBOX_DARK.muted;
#[allow(dead_code)]
pub const ACCENT: Color32 = GRUVBOX_DARK.accent;
/// Destructive-ops accent; goes live with the v1 undo journal (PLAN §5).
#[allow(dead_code)]
pub const RED: Color32 = GRUVBOX_DARK.red;

pub fn apply(ctx: &Context, palette: Palette) {
    let mode = if palette.is_dark {
        Theme::Dark
    } else {
        Theme::Light
    };
    ctx.set_theme(mode);
    let mut style: Style = ctx.style_of(mode).as_ref().clone();
    let v = &mut style.visuals;

    v.panel_fill = palette.bg;
    v.window_fill = palette.bg;
    v.extreme_bg_color = palette.bg;
    v.faint_bg_color = palette.bg;
    v.override_text_color = Some(palette.fg);
    v.selection.bg_fill = palette.sel;
    v.selection.stroke = Stroke::new(1.0, palette.accent);

    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.hovered.bg_fill = palette.hover;
    v.widgets.active.bg_fill = palette.sel;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.noninteractive,
    ] {
        w.fg_stroke.color = palette.fg;
        w.bg_stroke.color = Color32::TRANSPARENT;
    }

    style.spacing.item_spacing = egui::vec2(0.0, 0.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.button_padding = egui::vec2(8.0, 4.0);

    ctx.set_style_of(mode, style);
}
