pub mod dashboard;
pub mod editor;
pub mod explorer;
pub mod history;
pub mod host_card;
pub mod host_key;
pub mod log_panel;
pub mod rail;
pub mod tabs;
pub mod terminal;
pub mod update_banner;
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

    keep(&mut action, shortcuts(app, ui));

    keep(&mut action, update_banner::show(app, ui));

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
            keep(&mut action, tabs::show(app, ui));

            let view = match app.session() {
                Some(session) if session.terminal.is_some() => terminal::show(app, ui),
                Some(_) => explorer::show(app, ui),
                None => dashboard::show(app, ui),
            };
            keep(&mut action, view);
        });

    keep(&mut action, history::show(app, ui.ctx()));
    keep(&mut action, host_key::show(app, ui.ctx()));

    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    if !dropped.is_empty() && app.session().is_some_and(|s| s.terminal.is_none()) {
        keep(&mut action, Some(Action::DroppedFiles(dropped)));
    }

    action
}

fn shortcuts(app: &App, ui: &mut Ui) -> Option<Action> {
    use egui::{Key, KeyboardShortcut, Modifiers};

    let cmd = Modifiers::COMMAND;
    let close = KeyboardShortcut::new(cmd, Key::W);
    let locate = KeyboardShortcut::new(cmd, Key::L);

    let in_terminal = app.session().is_some_and(|s| s.terminal.is_some());
    let in_explorer = app.session().is_some_and(|s| s.terminal.is_none());

    let mut action = None;

    ui.input_mut(|i| {
        if i.consume_shortcut(&close)
            && let Some(index) = app.active
        {
            action = Some(Action::CloseTab(index));
            return;
        }

        if in_explorer && i.consume_shortcut(&locate) {
            action = Some(Action::EditPath);
            return;
        }

        if in_explorer
            && !in_terminal
            && app.session().is_some_and(|s| !s.selection.is_empty())
            && i.key_pressed(Key::Delete)
            && i.modifiers.is_none()
        {
            i.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Key {
                        key: Key::Delete,
                        ..
                    }
                )
            });
            action = Some(Action::DeleteSelected);
        }
    });

    action
}
