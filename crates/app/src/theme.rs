//! Token palettes (PLAN §2 & §11 it17): the whole UI speaks through twelve tokens,
//! never raw colors elsewhere. Dieter Rams Alabaster Linen and Gruvbox Dark
//! are first-class palettes supported under `theme = auto | braun | gruvbox-dark`.

use egui::{Color32, Context, Stroke, Style, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub border: Color32,
    pub hover: Color32,
    pub sel: Color32,
    pub fg: Color32,
    pub muted: Color32,
    pub label: Color32,
    pub accent: Color32,
    pub red: Color32,
    pub green: Color32,
    pub amber: Color32,
    pub is_dark: bool,
}

pub const GRUVBOX_DARK: Palette = Palette {
    bg: Color32::from_rgb(0x28, 0x28, 0x28),
    surface: Color32::from_rgb(0x3c, 0x38, 0x36),
    border: Color32::from_rgb(0x3c, 0x38, 0x36),
    hover: Color32::from_rgb(0x3c, 0x38, 0x36),
    sel: Color32::from_rgb(0x50, 0x49, 0x45),
    fg: Color32::from_rgb(0xeb, 0xdb, 0xb2),
    muted: Color32::from_rgb(0x92, 0x83, 0x74),
    label: Color32::from_rgb(0x7c, 0x6f, 0x64),
    accent: Color32::from_rgb(0xfa, 0xbd, 0x2f),
    red: Color32::from_rgb(0xfb, 0x49, 0x34),
    green: Color32::from_rgb(0xb8, 0xbb, 0x26),
    amber: Color32::from_rgb(0xd7, 0x99, 0x21),
    is_dark: true,
};

pub const BRAUN: Palette = Palette {
    bg: Color32::from_rgb(0xf5, 0xf4, 0xef),
    surface: Color32::from_rgb(0xff, 0xff, 0xff),
    border: Color32::from_rgb(0xe2, 0xdf, 0xd7),
    hover: Color32::from_rgb(0xea, 0xe8, 0xe1),
    sel: Color32::from_rgb(0xe0, 0xdc, 0xd3),
    fg: Color32::from_rgb(0x1a, 0x1a, 0x1e),
    muted: Color32::from_rgb(0x5c, 0x5b, 0x64),
    label: Color32::from_rgb(0x8e, 0x8c, 0x96),
    accent: Color32::from_rgb(0xe6, 0x51, 0x00),
    red: Color32::from_rgb(0xd9, 0x38, 0x1e),
    green: Color32::from_rgb(0x16, 0xa3, 0x4a),
    amber: Color32::from_rgb(0xd9, 0x77, 0x06),
    is_dark: false,
};

pub fn palette_for(name: &str, system_theme: Option<Theme>) -> Palette {
    match name {
        "braun" => BRAUN,
        "gruvbox-dark" | "gruvbox" => GRUVBOX_DARK,
        "auto" => match system_theme {
            Some(Theme::Light) => BRAUN,
            Some(Theme::Dark) => GRUVBOX_DARK,
            None => BRAUN,
        },
        _ => GRUVBOX_DARK,
    }
}

pub fn setup_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Embedded JetBrains Mono font (SIL Open Font License 1.1)
    fonts.font_data.insert(
        "JetBrainsMono-Regular".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );

    // Prioritize JetBrainsMono for monospace
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono-Regular".to_owned());

    // CJK and non-Latin system fallback font chain
    #[cfg(target_os = "macos")]
    let system_fallbacks = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ];
    #[cfg(target_os = "windows")]
    let system_fallbacks = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\arial.ttf",
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let system_fallbacks = [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];

    for (idx, path) in system_fallbacks.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(path) {
            let font_name = format!("system_fallback_{idx}");
            fonts.font_data.insert(
                font_name.clone(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push(font_name.clone());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push(font_name);
        }
    }

    ctx.set_fonts(fonts);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_palette_selection() {
        assert_eq!(palette_for("braun", None), BRAUN);
        assert_eq!(palette_for("gruvbox-dark", None), GRUVBOX_DARK);
        assert_eq!(palette_for("gruvbox", None), GRUVBOX_DARK);
        assert_eq!(palette_for("unknown", None), GRUVBOX_DARK);

        assert_eq!(palette_for("auto", Some(Theme::Light)), BRAUN);
        assert_eq!(palette_for("auto", Some(Theme::Dark)), GRUVBOX_DARK);
        assert_eq!(palette_for("auto", None), BRAUN);
    }
}
