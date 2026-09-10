use egui::{RichText, Ui};

use crate::{
    app::{Action, App},
    theme,
    ui::widgets,
};

pub fn show(app: &App, ui: &mut Ui) -> Option<Action> {
    let update = app.update_available.as_ref()?;
    let mut action = None;

    egui::Panel::top("update_banner")
        .frame(
            egui::Frame::default()
                .fill(theme::tint(theme::ACCENT, 34))
                .inner_margin(egui::Margin::symmetric(theme::S3 as i8, theme::S2 as i8)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Version {} is available", update.version))
                        .color(theme::TEXT),
                );
                ui.label(
                    RichText::new(format!("(you have {})", env!("CARGO_PKG_VERSION")))
                        .color(theme::TEXT_FAINT)
                        .small(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::ghost_button(ui, "Dismiss", true).clicked() {
                        action = Some(Action::DismissUpdate);
                    }
                    let (label, hover) = match &update.asset {
                        Some(asset) => ("Download", asset.name.clone()),
                        None => ("View release", "Open the release page".to_owned()),
                    };
                    if widgets::secondary_button(ui, label)
                        .on_hover_text(hover)
                        .clicked()
                    {
                        action = Some(Action::OpenUpdate);
                    }
                });
            });
        });

    action
}
