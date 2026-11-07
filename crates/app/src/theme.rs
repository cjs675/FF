//! Token palettes (PLAN §2): the whole UI speaks through six tokens, never
//! raw colors elsewhere. gruvbox-dark ships; `theme = auto` and palette
//! files come with the v1 theming milestone.
//!
//! Fonts: deliberately untouched — font/icon sourcing is an open gate
//! (PLAN §7), so defaults remain until that decision.

use egui::{Color32, Context, Stroke, Style, Theme};

pub const BG: Color32 = Color32::from_rgb(0x28, 0x28, 0x28);
pub const HOVER: Color32 = Color32::from_rgb(0x3c, 0x38, 0x36);
pub const SEL: Color32 = Color32::from_rgb(0x50, 0x49, 0x45);
pub const FG: Color32 = Color32::from_rgb(0xeb, 0xdb, 0xb2);
pub const MUTED: Color32 = Color32::from_rgb(0x92, 0x83, 0x74);
pub const ACCENT: Color32 = Color32::from_rgb(0xfa, 0xbd, 0x2f);
/// Destructive-ops accent; goes live with the v1 undo journal (PLAN §5).
#[allow(dead_code)]
pub const RED: Color32 = Color32::from_rgb(0xfb, 0x49, 0x34);

pub fn apply(ctx: &Context) {
    ctx.set_theme(Theme::Dark);
    let mut style: Style = ctx.style_of(Theme::Dark).as_ref().clone();
    let v = &mut style.visuals;

    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = BG;
    v.faint_bg_color = BG;
    v.override_text_color = Some(FG);
    v.selection.bg_fill = SEL;
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.active.bg_fill = SEL;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.noninteractive,
    ] {
        w.fg_stroke.color = FG;
        w.bg_stroke.color = Color32::TRANSPARENT;
    }

    style.spacing.item_spacing = egui::vec2(0.0, 0.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.button_padding = egui::vec2(8.0, 4.0);

    ctx.set_style_of(Theme::Dark, style);
}
