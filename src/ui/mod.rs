pub mod dashboard;
pub mod editor;
pub mod explorer;
pub mod host_card;
pub mod log_panel;
pub mod rail;
pub mod widgets;

use egui::Ui;

use crate::app::{Action, App};
use crate::theme;

pub(crate) fn keep(slot: &mut Option<Action>, candidate: Option<Action>) {
    if slot.is_none() {
        *slot = candidate;
    }
}

pub fn root(app: &mut App, ui: &mut Ui) -> Option<Action> {
    let mut action = None;

    egui::Panel::left("rail")
        .resizable(false)
        .default_size(56.0)
        .show(ui, |ui| {
            keep(&mut action, rail::show(app, ui));
        });

    if app.show_log {
        egui::Panel::bottom("activity")
            .resizable(true)
            .default_size(132.0)
            .show(ui, |ui| {
                keep(&mut action, log_panel::show(app, ui));
            });
    }

    egui::CentralPanel::default()
        .frame(
            egui::Frame::default()
                .fill(theme::BG_BASE)
                .inner_margin(egui::Margin {
                    left: 24,
                    right: 24,
                    top: 20,
                    bottom: 12,
                }),
        )
        .show(ui, |ui| {
            let view = if app.session.is_some() {
                explorer::show(app, ui)
            } else {
                dashboard::show(app, ui)
            };
            keep(&mut action, view);
        });

    action
}
