use egui::{RichText, Ui};

use crate::app::{Action, App, relative_time};
use crate::connection::Protocol;
use crate::icon;
use crate::theme;
use crate::ui::{editor, host_card, keep, widgets};

pub fn show(app: &mut App, ui: &mut Ui) -> Option<Action> {
    let mut action = None;

    egui::ScrollArea::vertical()
        .id_salt("dashboard")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let content_width = ui.available_width();

            widgets::page_heading(
                ui,
                "Hosts",
                "Manage your saved servers and connect with one click",
            );

            ui.add_space(theme::S4);
            widgets::search_field(ui, &mut app.search, "Search hosts...");

            keep(&mut action, recents(app, ui));
            keep(&mut action, actions_row(ui));

            if let Some((profile, editing)) = &mut app.draft {
                ui.add_space(theme::S4);
                keep(&mut action, editor::show(ui, profile, editing.is_some()));
            }

            ui.add_space(theme::S5);
            keep(&mut action, grid(app, ui, content_width));
            ui.add_space(theme::S5);
        });

    action
}

fn recents(app: &App, ui: &mut Ui) -> Option<Action> {
    let (index, host) = app.most_recent()?;
    let last_used = host.last_used?;

    ui.add_space(theme::S4);
    widgets::section_label(ui, "RECENT");
    ui.add_space(theme::S2);

    let response = widgets::pill(ui, "recent", |ui| {
        widgets::status_dot(ui, theme::host_color(host.profile.display_name()), 8.0);
        ui.label(RichText::new(host.profile.display_name()).color(theme::TEXT));
        ui.label(
            RichText::new(relative_time(last_used))
                .color(theme::TEXT_FAINT)
                .small(),
        );
    })
    .on_hover_text("Reconnect");

    response.clicked().then_some(Action::Connect(index))
}

fn actions_row(ui: &mut Ui) -> Option<Action> {
    let mut action = None;

    ui.add_space(theme::S4);
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, &format!("{}  NEW SERVER", icon::PLUS)).clicked() {
            action = Some(Action::NewHost(Protocol::Sftp));
        }
        if widgets::secondary_button(ui, &format!("{}  NEW S3", icon::CLOUD)).clicked() {
            action = Some(Action::NewHost(Protocol::S3));
        }
        ui.add_enabled(
            false,
            egui::Button::new(format!("{}  NEW GROUP", icon::FOLDER_PLUS))
                .corner_radius(theme::R_MD),
        )
        .on_disabled_hover_text("Groups are not implemented yet");
        ui.add_enabled(
            false,
            egui::Button::new(format!("{}  IMPORT", icon::DOWNLOAD)).corner_radius(theme::R_MD),
        )
        .on_disabled_hover_text("Import is not implemented yet");
    });

    action
}

fn grid(app: &App, ui: &mut Ui, content_width: f32) -> Option<Action> {
    let mut action = None;

    widgets::section_label(ui, "HOSTS");
    ui.add_space(theme::S3);

    if app.hosts.is_empty() {
        return host_card::add_tile(ui);
    }

    let visible = app.visible_hosts();
    if visible.is_empty() {
        widgets::empty_state(ui, "No matches", "Try a different search term");
        return None;
    }

    let spacing = ui.spacing().item_spacing.x;
    let per_row =
        (((content_width + spacing) / (host_card::CARD_SIZE.x + spacing)).floor() as usize).max(1);

    for chunk in visible.chunks(per_row) {
        ui.horizontal(|ui| {
            for (index, host) in chunk {
                keep(&mut action, host_card::show(ui, *index, host));
            }
        });
        ui.add_space(theme::S3);
    }

    action
}
