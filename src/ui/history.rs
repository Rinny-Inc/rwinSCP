use egui::{RichText, Ui};

use crate::app::{
    Action, App, Direction, HISTORY_CAPACITY, TransferState, human_size, relative_time,
};
use crate::icon;
use crate::theme;
use crate::ui::widgets;

pub fn show(app: &App, ctx: &egui::Context) -> Option<Action> {
    if !app.show_history {
        return None;
    }

    let mut action = None;
    let mut open = true;

    egui::Window::new("Transfer history")
        .open(&mut open)
        .default_size([560.0, 380.0])
        .collapsible(false)
        .show(ctx, |ui| {
            body(app, ui, &mut action);
        });

    if !open {
        action = Some(Action::ToggleHistory);
    }

    action
}

fn body(app: &App, ui: &mut Ui, action: &mut Option<Action>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} of {HISTORY_CAPACITY} kept", app.history.len()))
                .color(theme::TEXT_FAINT)
                .small(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::ghost_button(ui, "Clear", !app.history.is_empty()).clicked() {
                *action = Some(Action::ClearHistory);
            }
        });
    });

    ui.add_space(theme::S2);
    widgets::divider(ui);
    ui.add_space(theme::S2);

    if app.history.is_empty() {
        widgets::empty_state(ui, "No transfers yet", "Uploads and downloads show up here");
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for record in app.history.iter().rev() {
                ui.horizontal(|ui| {
                    let (glyph, tint) = match record.direction {
                        Direction::Upload => (icon::UPLOAD, theme::ACCENT),
                        Direction::Download => (icon::DOWNLOAD, theme::OK),
                    };
                    ui.label(RichText::new(glyph).color(tint));

                    let (status, color) = match record.state {
                        TransferState::Running => ("running", theme::PENDING),
                        TransferState::Done => ("done", theme::OK),
                        TransferState::Failed => ("failed", theme::DANGER),
                    };

                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&record.label)
                                .color(theme::TEXT)
                                .monospace()
                                .small(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · {} · {}",
                                record.host,
                                record.direction.clone().label(),
                                human_size(record.bytes),
                                relative_time(record.at)
                            ))
                            .color(theme::TEXT_FAINT)
                            .small(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(status).color(color).small());
                        });
                    });
                    ui.add_space(theme::S1);
                });
            }
        });
}
