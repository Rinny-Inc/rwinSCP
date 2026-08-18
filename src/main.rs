mod app;
mod backend;
mod connection;
mod theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "rwinSCP",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(theme::visuals());
            Ok(Box::new(app::App::default()))
        }),
    )
}
