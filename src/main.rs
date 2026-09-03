mod app;
mod backend;
mod connection;
mod icon;
mod store;
mod theme;
mod ui;
mod update;

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default();
    match eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => eprintln!("couldn't load window icon {e}"),
    }
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([880.0, 560.0])
            .with_title("rwinSCP"),
        ..Default::default()
    };

    eframe::run_native(
        "rwinSCP",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::App::load()))
        }),
    )
}
