use egui::{Context, RichText};

use crate::{
    app::{Action, App},
    theme,
    ui::{self, widgets},
};

pub fn show(app: &App, ctx: &Context) -> Option<Action> {
    let pending = app.pending_host_key.as_ref()?;
    let mut action = None;

    egui::Modal::new(egui::Id::new("host_key_prompt")).show(ctx, |ui| {
        ui.set_width(460.0);

        ui.label(
            RichText::new("Unrecognised server")
                .color(theme::TEXT)
                .size(16.0)
                .strong(),
        );
        ui.add_space(theme::S2);
        ui.label(
            RichText::new(format!(
                "{} has not been connected to before, so its identity cannot be confirmed!",
                pending.host
            ))
            .color(theme::TEXT_DIM),
        );
        ui.add_space(theme::S3);
        widgets::card_frame(theme::R_MD).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(&pending.key_type)
                    .color(theme::TEXT_FAINT)
                    .small(),
            );
            ui.label(
                RichText::new(&pending.fingerprint)
                    .color(theme::TEXT)
                    .monospace(),
            );
        });

        ui.add_space(theme::S3);
        ui.label(
            RichText::new(
                "Only continue if this fingerprint matches the one the server's \
            operator published. Trusting it writes the key to ~/.ssh/known_hosts.",
            )
            .color(theme::TEXT_FAINT)
            .small(),
        );

        ui.add_space(theme::S4);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Trusted and connect", true).clicked() {
                action = Some(Action::TrustHostKey);
            }
            if widgets::secondary_button(ui, "Cancel").clicked() {
                action = Some(Action::RejectHostKey);
            }
        });
    });
    action
}
