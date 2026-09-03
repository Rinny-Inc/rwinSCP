use egui::{RichText, Ui};

use crate::app::{
    Action, App, Direction, HISTORY_CAPACITY, TransferRecord, TransferState, human_size,
    relative_time,
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
            for (index, record) in app.history.iter().enumerate().rev() {
                row(ui, index, record, action);
                ui.add_space(theme::S2);
            }
        });
}

fn row(ui: &mut Ui, index: usize, record: &TransferRecord, action: &mut Option<Action>) {
    let (glyph, tint) = match record.direction {
        Direction::Upload => (icon::UPLOAD, theme::ACCENT),
        Direction::Download => (icon::DOWNLOAD, theme::OK),
    };
    let (status, status_color) = match record.state {
        TransferState::Queued => ("queue", theme::TEXT_FAINT),
        TransferState::Running => ("running", theme::PENDING),
        TransferState::Done => ("done", theme::OK),
        TransferState::Failed => ("failed", theme::DANGER),
        TransferState::Cancelled => ("cancelled", theme::TEXT_FAINT),
    };

    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).color(tint));
        ui.label(
            RichText::new(&record.label)
                .color(theme::TEXT)
                .monospace()
                .small(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(status).color(status_color).small());

            if matches!(record.state, TransferState::Running | TransferState::Queued)
                && widgets::ghost_button(ui, icon::X_SMALL, true)
                    .on_hover_text("Cancel this transfer")
                    .clicked()
            {
                *action = Some(Action::CancelTransfer(index));
            }
        });
    });

    if record.state == TransferState::Running {
        let bar = match record.fraction() {
            Some(fraction) => egui::ProgressBar::new(fraction).show_percentage(),
            None => egui::ProgressBar::new(0.0).animate(true),
        };
        ui.add(
            bar.fill(theme::ACCENT)
                .corner_radius(theme::R_SM)
                .desired_height(6.0),
        );
    }

    ui.label(
        RichText::new(detail_line(record))
            .color(theme::TEXT_FAINT)
            .small(),
    );
}

fn detail_line(record: &TransferRecord) -> String {
    let moved = human_size(record.bytes);
    let size = match record.total {
        Some(total) => format!("{moved} / {}", human_size(total)),
        None => moved,
    };
    let mut parts = vec![
        record.host.clone(),
        record.direction.clone().label().to_owned(),
        size,
    ];

    if record.state == TransferState::Running {
        let rate = record.bytes_per_second();
        if rate > 0.0 {
            parts.push(format!("{}/s", human_size(rate as u64)));
        }
        if let Some(eta) = record.eta_seconds() {
            parts.push(format!("{} left", format_duration(eta)));
        }
    } else {
        parts.push(relative_time(record.at));
    }

    parts.join(" · ")
}

fn format_duration(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60),
    }
}
