//! ff — a cross-platform, chromeless file browser. PLAN.md §1: no header,
//! no tabs, no traffic lights; the window earns its keep.

mod shell;
mod theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ff")
            .with_decorations(false)
            .with_inner_size([860.0, 520.0])
            .with_min_inner_size([560.0, 320.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ff",
        options,
        Box::new(|cc| Ok(Box::new(shell::FfShell::new(cc)))),
    )
}
