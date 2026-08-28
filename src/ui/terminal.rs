use crate::app::{Action, App, Status};
use crate::icon;
use crate::theme;
use crate::ui::{keep, widgets};
use egui::{Key, RichText, Ui};

pub fn show(app: &mut App, ui: &mut Ui) -> Option<Action> {
    let mut action = None;

    let Some(session) = &mut app.session else {
        return None;
    };

    ui.horizontal(|ui| {
        if widgets::ghost_button(ui, &format!("{} Hosts", icon::ARROW_LEFT), true).clicked() {
            action = Some(Action::Disconnect);
        }
        ui.add_space(theme::S2);

        let (color, label) = match session.status {
            Status::Connected => (theme::OK, "connected"),
            Status::Connecting => (theme::PENDING, "connecting"),
        };
        widgets::status_dot(ui, color, 8.0);
        ui.label(
            RichText::new(session.profile.display_name())
                .color(theme::TEXT)
                .strong(),
        );
        ui.label(RichText::new(label).color(theme::TEXT_FAINT).small());
    });

    ui.add_space(theme::S3);

    let Some(terminal) = &mut session.terminal else {
        widgets::empty_state(ui, "No shell", "This session has no interactive shell");
        return action;
    };

    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("$").color(theme::ACCENT).monospace().strong());

            let input = ui.add(
                egui::TextEdit::singleline(&mut terminal.input)
                    .font(egui::TextStyle::Monospace)
                    .frame(egui::Frame::NONE)
                    .hint_text(
                        RichText::new("type a command, Enter to run").color(theme::TEXT_FAINT),
                    )
                    .desired_width(f32::INFINITY),
            );

            if input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                keep(&mut action, Some(Action::ShellSend));
                input.request_focus();
            }
        });

        ui.add_space(theme::S2);

        egui::Frame::default()
            .fill(theme::BG_SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(theme::R_MD)
            .inner_margin(theme::S2 as i8)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            if terminal.output.is_empty() {
                                ui.label(
                                    RichText::new("Waiting for the remote shell\u{2026}")
                                        .color(theme::TEXT_FAINT)
                                        .monospace()
                                        .small(),
                                );
                            } else {
                                ui.label(
                                    RichText::new(terminal.output.trim_start_matches('\n'))
                                        .color(theme::TEXT)
                                        .monospace(),
                                );
                            }
                        });
                    });
            });
    });
    action
}
