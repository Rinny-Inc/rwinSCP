mod app;
mod backend;
mod connection;
mod icon;
mod store;
mod theme;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
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
