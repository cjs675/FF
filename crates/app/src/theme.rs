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

pub const GRUVBOX_LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xfb, 0xf1, 0xc7),
    surface: Color32::from_rgb(0xf9, 0xf5, 0xd7),
    border: Color32::from_rgb(0xd5, 0xc4, 0xa1),
    hover: Color32::from_rgb(0xeb, 0xdb, 0xb2),
    sel: Color32::from_rgb(0xd5, 0xc4, 0xa1),
    fg: Color32::from_rgb(0x3c, 0x38, 0x36),
    muted: Color32::from_rgb(0x7c, 0x6f, 0x64),
    label: Color32::from_rgb(0x92, 0x83, 0x74),
    accent: Color32::from_rgb(0xaf, 0x3a, 0x03),
    red: Color32::from_rgb(0x9d, 0x00, 0x06),
    green: Color32::from_rgb(0x79, 0x74, 0x0e),
    amber: Color32::from_rgb(0xb5, 0x76, 0x14),
    is_dark: false,
};

pub const CATPPUCCIN_MOCHA: Palette = Palette {
    bg: Color32::from_rgb(0x1e, 0x1e, 0x2e),
    surface: Color32::from_rgb(0x18, 0x18, 0x25),
    border: Color32::from_rgb(0x31, 0x32, 0x44),
    hover: Color32::from_rgb(0x31, 0x32, 0x44),
    sel: Color32::from_rgb(0x45, 0x47, 0x5a),
    fg: Color32::from_rgb(0xcd, 0xd6, 0xf4),
    muted: Color32::from_rgb(0xa6, 0xad, 0xc8),
    label: Color32::from_rgb(0x6c, 0x70, 0x86),
    accent: Color32::from_rgb(0xcb, 0xa6, 0xf7),
    red: Color32::from_rgb(0xf3, 0x8b, 0xa8),
    green: Color32::from_rgb(0xa6, 0xe3, 0xa1),
    amber: Color32::from_rgb(0xf9, 0xe2, 0xaf),
    is_dark: true,
};

pub const CATPPUCCIN_LATTE: Palette = Palette {
    bg: Color32::from_rgb(0xef, 0xf1, 0xf5),
    surface: Color32::from_rgb(0xe6, 0xe9, 0xef),
    border: Color32::from_rgb(0xcc, 0xd0, 0xda),
    hover: Color32::from_rgb(0xdc, 0xe0, 0xe8),
    sel: Color32::from_rgb(0xbc, 0xc0, 0xcc),
    fg: Color32::from_rgb(0x4c, 0x4f, 0x69),
    muted: Color32::from_rgb(0x6c, 0x6f, 0x85),
    label: Color32::from_rgb(0x9c, 0xa0, 0xb0),
    accent: Color32::from_rgb(0x88, 0x39, 0xef),
    red: Color32::from_rgb(0xd2, 0x0f, 0x39),
    green: Color32::from_rgb(0x40, 0xa0, 0x2b),
    amber: Color32::from_rgb(0xdf, 0x8e, 0x1d),
    is_dark: false,
};

pub const TOKYO_NIGHT: Palette = Palette {
    bg: Color32::from_rgb(0x1a, 0x1b, 0x26),
    surface: Color32::from_rgb(0x24, 0x28, 0x3b),
    border: Color32::from_rgb(0x2f, 0x35, 0x4f),
    hover: Color32::from_rgb(0x29, 0x2e, 0x42),
    sel: Color32::from_rgb(0x36, 0x3e, 0x5f),
    fg: Color32::from_rgb(0xc0, 0xca, 0xf5),
    muted: Color32::from_rgb(0x7a, 0xa2, 0xf7),
    label: Color32::from_rgb(0x56, 0x5f, 0x89),
    accent: Color32::from_rgb(0x7a, 0xa2, 0xf7),
    red: Color32::from_rgb(0xf7, 0x76, 0x8e),
    green: Color32::from_rgb(0x9e, 0xce, 0x6a),
    amber: Color32::from_rgb(0xe0, 0xaf, 0x68),
    is_dark: true,
};

pub const NORD: Palette = Palette {
    bg: Color32::from_rgb(0x2e, 0x34, 0x40),
    surface: Color32::from_rgb(0x3b, 0x42, 0x52),
    border: Color32::from_rgb(0x43, 0x4c, 0x5e),
    hover: Color32::from_rgb(0x43, 0x4c, 0x5e),
    sel: Color32::from_rgb(0x4c, 0x56, 0x6a),
    fg: Color32::from_rgb(0xec, 0xef, 0xf4),
    muted: Color32::from_rgb(0xd8, 0xde, 0xe9),
    label: Color32::from_rgb(0x81, 0xa1, 0xc1),
    accent: Color32::from_rgb(0x88, 0xc0, 0xd0),
    red: Color32::from_rgb(0xbf, 0x61, 0x6a),
    green: Color32::from_rgb(0xa3, 0xbe, 0x8c),
    amber: Color32::from_rgb(0xeb, 0xcb, 0x8b),
    is_dark: true,
};

pub const DRACULA: Palette = Palette {
    bg: Color32::from_rgb(0x28, 0x2a, 0x36),
    surface: Color32::from_rgb(0x34, 0x37, 0x46),
    border: Color32::from_rgb(0x44, 0x47, 0x5a),
    hover: Color32::from_rgb(0x44, 0x47, 0x5a),
    sel: Color32::from_rgb(0x62, 0x72, 0xa4),
    fg: Color32::from_rgb(0xf8, 0xf8, 0xf2),
    muted: Color32::from_rgb(0xbd, 0x93, 0xf9),
    label: Color32::from_rgb(0x62, 0x72, 0xa4),
    accent: Color32::from_rgb(0xff, 0x79, 0xc6),
    red: Color32::from_rgb(0xff, 0x55, 0x55),
    green: Color32::from_rgb(0x50, 0xfa, 0x7b),
    amber: Color32::from_rgb(0xff, 0xb8, 0x6c),
    is_dark: true,
};

pub const SOLARIZED_DARK: Palette = Palette {
    bg: Color32::from_rgb(0x00, 0x2b, 0x36),
    surface: Color32::from_rgb(0x07, 0x36, 0x42),
    border: Color32::from_rgb(0x0e, 0x4b, 0x5c),
    hover: Color32::from_rgb(0x07, 0x36, 0x42),
    sel: Color32::from_rgb(0x0e, 0x4b, 0x5c),
    fg: Color32::from_rgb(0x83, 0x94, 0x96),
    muted: Color32::from_rgb(0x58, 0x6e, 0x75),
    label: Color32::from_rgb(0x65, 0x7b, 0x83),
    accent: Color32::from_rgb(0x26, 0x8b, 0xd2),
    red: Color32::from_rgb(0xdc, 0x32, 0x2f),
    green: Color32::from_rgb(0x85, 0x99, 0x00),
    amber: Color32::from_rgb(0xb5, 0x89, 0x00),
    is_dark: true,
};

pub const SOLARIZED_LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xfd, 0xf6, 0xe3),
    surface: Color32::from_rgb(0xee, 0xe8, 0xd5),
    border: Color32::from_rgb(0xd6, 0xcc, 0xb4),
    hover: Color32::from_rgb(0xe0, 0xd8, 0xc3),
    sel: Color32::from_rgb(0xd6, 0xcc, 0xb4),
    fg: Color32::from_rgb(0x65, 0x7b, 0x83),
    muted: Color32::from_rgb(0x83, 0x94, 0x96),
    label: Color32::from_rgb(0x93, 0xa1, 0xa1),
    accent: Color32::from_rgb(0x26, 0x8b, 0xd2),
    red: Color32::from_rgb(0xdc, 0x32, 0x2f),
    green: Color32::from_rgb(0x85, 0x99, 0x00),
    amber: Color32::from_rgb(0xb5, 0x89, 0x00),
    is_dark: false,
};

pub const ROSE_PINE: Palette = Palette {
    bg: Color32::from_rgb(0x19, 0x17, 0x24),
    surface: Color32::from_rgb(0x1f, 0x1d, 0x2e),
    border: Color32::from_rgb(0x26, 0x23, 0x3a),
    hover: Color32::from_rgb(0x26, 0x23, 0x3a),
    sel: Color32::from_rgb(0x40, 0x3d, 0x52),
    fg: Color32::from_rgb(0xe0, 0xde, 0xf4),
    muted: Color32::from_rgb(0x90, 0x8c, 0xaa),
    label: Color32::from_rgb(0x6e, 0x6a, 0x86),
    accent: Color32::from_rgb(0xeb, 0xbc, 0xba),
    red: Color32::from_rgb(0xeb, 0x6f, 0x92),
    green: Color32::from_rgb(0x31, 0x74, 0x8f),
    amber: Color32::from_rgb(0xf6, 0xc1, 0x77),
    is_dark: true,
};

pub const MONOKAI_PRO: Palette = Palette {
    bg: Color32::from_rgb(0x2d, 0x2a, 0x2e),
    surface: Color32::from_rgb(0x3a, 0x36, 0x3b),
    border: Color32::from_rgb(0x40, 0x3e, 0x41),
    hover: Color32::from_rgb(0x40, 0x3e, 0x41),
    sel: Color32::from_rgb(0x56, 0x53, 0x57),
    fg: Color32::from_rgb(0xfc, 0xfc, 0xfa),
    muted: Color32::from_rgb(0x93, 0x92, 0x93),
    label: Color32::from_rgb(0x72, 0x70, 0x72),
    accent: Color32::from_rgb(0xff, 0xd8, 0x66),
    red: Color32::from_rgb(0xff, 0x61, 0x88),
    green: Color32::from_rgb(0xa9, 0xdc, 0x76),
    amber: Color32::from_rgb(0xfc, 0x98, 0x67),
    is_dark: true,
};

pub const ONE_DARK: Palette = Palette {
    bg: Color32::from_rgb(0x28, 0x2c, 0x34),
    surface: Color32::from_rgb(0x21, 0x25, 0x2b),
    border: Color32::from_rgb(0x3b, 0x40, 0x48),
    hover: Color32::from_rgb(0x2c, 0x31, 0x3a),
    sel: Color32::from_rgb(0x3e, 0x44, 0x51),
    fg: Color32::from_rgb(0xab, 0xb2, 0xbf),
    muted: Color32::from_rgb(0x5c, 0x63, 0x70),
    label: Color32::from_rgb(0x4b, 0x52, 0x63),
    accent: Color32::from_rgb(0x61, 0xaf, 0xef),
    red: Color32::from_rgb(0xe0, 0x6c, 0x75),
    green: Color32::from_rgb(0x98, 0xc3, 0x79),
    amber: Color32::from_rgb(0xe5, 0xc0, 0x7b),
    is_dark: true,
};

pub const OLED_BLACK: Palette = Palette {
    bg: Color32::from_rgb(0x00, 0x00, 0x00),
    surface: Color32::from_rgb(0x12, 0x12, 0x12),
    border: Color32::from_rgb(0x27, 0x27, 0x27),
    hover: Color32::from_rgb(0x1c, 0x1c, 0x1c),
    sel: Color32::from_rgb(0x2c, 0x2c, 0x2c),
    fg: Color32::from_rgb(0xf0, 0xf0, 0xf0),
    muted: Color32::from_rgb(0x88, 0x88, 0x88),
    label: Color32::from_rgb(0x55, 0x55, 0x55),
    accent: Color32::from_rgb(0xff, 0xff, 0xff),
    red: Color32::from_rgb(0xff, 0x45, 0x3a),
    green: Color32::from_rgb(0x30, 0xd1, 0x58),
    amber: Color32::from_rgb(0xff, 0x9f, 0x0a),
    is_dark: true,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeItem {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub is_dark: bool,
    pub palette: Palette,
}

pub const ALL_THEMES: [ThemeItem; 14] = [
    ThemeItem {
        id: "braun",
        name: "Braun Alabaster",
        description: "Dieter Rams alabaster linen with tactile warm orange accent",
        is_dark: false,
        palette: BRAUN,
    },
    ThemeItem {
        id: "gruvbox-dark",
        name: "Gruvbox Dark",
        description: "Warm retro groove dark charcoal with golden amber accent",
        is_dark: true,
        palette: GRUVBOX_DARK,
    },
    ThemeItem {
        id: "gruvbox-light",
        name: "Gruvbox Light",
        description: "Light warm parchment with earthy ochre and terracotta",
        is_dark: false,
        palette: GRUVBOX_LIGHT,
    },
    ThemeItem {
        id: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        description: "Soothing pastel lavender and mauve on deep obsidian",
        is_dark: true,
        palette: CATPPUCCIN_MOCHA,
    },
    ThemeItem {
        id: "catppuccin-latte",
        name: "Catppuccin Latte",
        description: "Gentle light cream with soft pastel accents",
        is_dark: false,
        palette: CATPPUCCIN_LATTE,
    },
    ThemeItem {
        id: "tokyo-night",
        name: "Tokyo Night",
        description: "Midnight blue-slate with vivid neon cyan and magenta",
        is_dark: true,
        palette: TOKYO_NIGHT,
    },
    ThemeItem {
        id: "nord",
        name: "Nord",
        description: "Arctic icy slate with frosty blue and cold pastels",
        is_dark: true,
        palette: NORD,
    },
    ThemeItem {
        id: "dracula",
        name: "Dracula",
        description: "Gothic dark purple-slate with vibrant pink and green",
        is_dark: true,
        palette: DRACULA,
    },
    ThemeItem {
        id: "solarized-dark",
        name: "Solarized Dark",
        description: "Ethan Schoonover low-contrast teal-green dark palette",
        is_dark: true,
        palette: SOLARIZED_DARK,
    },
    ThemeItem {
        id: "solarized-light",
        name: "Solarized Light",
        description: "High-legibility solarized parchment and cyan-amber",
        is_dark: false,
        palette: SOLARIZED_LIGHT,
    },
    ThemeItem {
        id: "rose-pine",
        name: "Rosé Pine",
        description: "Muted pine-rose and goldenrod on warm twilight",
        is_dark: true,
        palette: ROSE_PINE,
    },
    ThemeItem {
        id: "monokai-pro",
        name: "Monokai Pro",
        description: "Warm dark background with vibrant spectrum filters",
        is_dark: true,
        palette: MONOKAI_PRO,
    },
    ThemeItem {
        id: "one-dark",
        name: "One Dark",
        description: "Balanced dark grey and soft blue from Atom and VS Code",
        is_dark: true,
        palette: ONE_DARK,
    },
    ThemeItem {
        id: "oled-black",
        name: "OLED Pure Black",
        description: "True pitch-black with crisp white contrast for OLED displays",
        is_dark: true,
        palette: OLED_BLACK,
    },
];

pub fn all_themes() -> &'static [ThemeItem] {
    &ALL_THEMES
}

pub fn palette_for(name: &str, system_theme: Option<Theme>) -> Palette {
    match name {
        "braun" => BRAUN,
        "gruvbox-dark" | "gruvbox" => GRUVBOX_DARK,
        "gruvbox-light" => GRUVBOX_LIGHT,
        "catppuccin-mocha" | "mocha" => CATPPUCCIN_MOCHA,
        "catppuccin-latte" | "latte" => CATPPUCCIN_LATTE,
        "tokyo-night" | "tokyonight" => TOKYO_NIGHT,
        "nord" => NORD,
        "dracula" => DRACULA,
        "solarized-dark" => SOLARIZED_DARK,
        "solarized-light" => SOLARIZED_LIGHT,
        "rose-pine" | "rosepine" => ROSE_PINE,
        "monokai-pro" | "monokai" => MONOKAI_PRO,
        "one-dark" | "onedark" => ONE_DARK,
        "oled-black" | "oled" => OLED_BLACK,
        "auto" => match system_theme {
            Some(Theme::Light) => BRAUN,
            Some(Theme::Dark) => GRUVBOX_DARK,
            None => BRAUN,
        },
        other => {
            if let Some(t) = ALL_THEMES
                .iter()
                .find(|t| t.id.eq_ignore_ascii_case(other) || t.name.eq_ignore_ascii_case(other))
            {
                t.palette
            } else {
                GRUVBOX_DARK
            }
        }
    }
}

pub fn setup_fonts(ctx: &Context) {
    // Enable crisp pixel-grid alignment for text, lines, and rects
    ctx.tessellation_options_mut(|to| {
        to.round_text_to_pixels = true;
        to.round_line_segments_to_pixels = true;
        to.round_rects_to_pixels = true;
    });

    let mut fonts = egui::FontDefinitions::default();

    // Embedded JetBrains Mono font (SIL Open Font License 1.1)
    fonts.font_data.insert(
        "JetBrainsMono-Regular".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );

    // Prioritize JetBrainsMono for BOTH Monospace and Proportional so all text across the app is purely monospace & uniform
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono-Regular".to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "JetBrainsMono-Regular".to_owned());

    // Fallback system fonts for proportional (behind JetBrainsMono)
    #[cfg(target_os = "macos")]
    let primary_proportional = [
        ("/System/Library/Fonts/SFNS.ttf", "SFNS"),
        ("/System/Library/Fonts/HelveticaNeue.ttc", "HelveticaNeue"),
        ("/System/Library/Fonts/Supplemental/Arial.ttf", "Arial"),
    ];
    #[cfg(target_os = "windows")]
    let primary_proportional = [
        ("C:\\Windows\\Fonts\\segoeui.ttf", "SegoeUI"),
        ("C:\\Windows\\Fonts\\arial.ttf", "Arial"),
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let primary_proportional = [
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "DejaVuSans"),
    ];

    for (path, name) in primary_proportional {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                name.to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push(name.to_owned());
            break;
        }
    }

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
    style.spacing.scroll.kinetic.rubber_band = false;

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
        assert_eq!(palette_for("gruvbox-light", None), GRUVBOX_LIGHT);
        assert_eq!(palette_for("catppuccin-mocha", None), CATPPUCCIN_MOCHA);
        assert_eq!(palette_for("tokyo-night", None), TOKYO_NIGHT);
        assert_eq!(palette_for("nord", None), NORD);
        assert_eq!(palette_for("dracula", None), DRACULA);
        assert_eq!(palette_for("oled-black", None), OLED_BLACK);
        assert_eq!(palette_for("unknown", None), GRUVBOX_DARK);

        assert_eq!(palette_for("auto", Some(Theme::Light)), BRAUN);
        assert_eq!(palette_for("auto", Some(Theme::Dark)), GRUVBOX_DARK);
        assert_eq!(palette_for("auto", None), BRAUN);

        let themes = all_themes();
        assert_eq!(themes.len(), 14);
        assert!(themes.iter().any(|t| t.id == "braun"));
        assert!(themes.iter().any(|t| t.id == "tokyo-night"));
    }

    #[test]
    fn test_setup_fonts_run() {
        let ctx = Context::default();
        ctx.tessellation_options_mut(|to| {
            to.round_text_to_pixels = true;
            to.round_line_segments_to_pixels = true;
            to.round_rects_to_pixels = true;
        });
        setup_fonts(&ctx);
        let mut output = ctx.run_ui(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(egui::RichText::new("Crisp and clean text").font(egui::FontId::monospace(13.0)));
            });
        });
        output.textures_delta.clear();
    }
}
