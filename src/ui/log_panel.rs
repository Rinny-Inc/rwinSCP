use egui::{RichText, Ui};

use crate::app::{Action, App, LogLevel};
use crate::theme;
use crate::ui::widgets;

pub fn show(app: &App, ui: &mut Ui) -> Option<Action> {
    let mut action = None;

    ui.add_space(theme::S2);
    ui.horizontal(|ui| {
        ui.add_space(theme::S3);
        widgets::section_label(ui, "ACTIVITY");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::S3);
            if widgets::ghost_button(ui, "Clear", !app.log.is_empty()).clicked() {
                action = Some(Action::ClearLog);
            }
        });
    });
    ui.add_space(theme::S1);

    egui::ScrollArea::vertical()
        .id_salt("activity_log")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add_space(theme::S1);
            if app.log.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(theme::S3);
                    ui.label(
                        RichText::new("Nothing has happened yet.")
                            .color(theme::TEXT_FAINT)
                            .small()
                            .italics(),
                    );
                });
            }

            for line in &app.log {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(theme::S3);
                    let color = match line.level {
                        LogLevel::Info => theme::TEXT_FAINT,
                        LogLevel::Success => theme::OK,
                        LogLevel::Error => theme::DANGER,
                    };
                    ui.label(RichText::new(&line.text).color(color).monospace().small());
                });
            }
        });

    action
}
