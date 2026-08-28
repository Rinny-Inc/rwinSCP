use egui::{Align, Layout, RichText, Ui, Vec2};

use crate::app::{Action, Host, relative_time};
use crate::connection::Protocol;
use crate::icon;
use crate::theme;
use crate::ui::widgets;

pub const CARD_SIZE: Vec2 = Vec2::new(300.0, 168.0);
const PADDING: f32 = theme::S3;

pub fn show(ui: &mut Ui, index: usize, host: &Host) -> Option<Action> {
    let mut action = None;
    let profile = &host.profile;
    let name = profile.display_name();
    let color = theme::host_color(name);

    let response = widgets::fixed_card(ui, CARD_SIZE, theme::R_XL, PADDING, |ui| {
        widgets::corner_glow(ui, ui.max_rect(), color);

        ui.horizontal(|ui| {
            widgets::avatar(ui, name, color, 40.0);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::ghost_button(ui, icon::TRASH, true)
                    .on_hover_text("Remove host")
                    .clicked()
                {
                    action = Some(Action::DeleteHost(index));
                }
                if widgets::ghost_button(ui, icon::PENCIL, true)
                    .on_hover_text("Edit host")
                    .clicked()
                {
                    action = Some(Action::EditHost(index));
                }
                if widgets::ghost_button(ui, icon::ARROW_RIGHT, profile.protocol.browsable())
                    .on_hover_text("Open explorer")
                    .clicked()
                {
                    action = Some(Action::Connect(index));
                }
            });
        });

        ui.add_space(theme::S3);
        ui.label(RichText::new(name).color(theme::TEXT).size(15.0).strong());
        ui.add_space(2.0);
        ui.label(
            RichText::new(profile.endpoint())
                .color(theme::TEXT_FAINT)
                .monospace()
                .small(),
        );

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                let (dot, label) = match host.last_used {
                    Some(at) => (theme::OK, relative_time(at)),
                    None => (theme::TEXT_FAINT, "never connected".to_owned()),
                };
                widgets::status_dot(ui, dot, 6.0);
                ui.label(RichText::new(label).color(theme::TEXT_FAINT).small());
            });
        });
    });

    if action.is_none() && response.clicked() {
        action = Some(Action::Connect(index));
    }

    action
}

pub fn add_tile(ui: &mut Ui) -> Option<Action> {
    let response = widgets::fixed_card(ui, CARD_SIZE, theme::R_XL, PADDING, |ui| {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new(format!("{}  Add a host", icon::PLUS))
                    .color(theme::TEXT_FAINT)
                    .size(14.0),
            );
        });
    });

    response
        .clicked()
        .then_some(Action::NewHost(Protocol::Sftp))
}
