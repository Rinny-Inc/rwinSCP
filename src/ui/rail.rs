use egui::{Align, Color32, CornerRadius, FontId, Layout, Rect, Sense, Ui, Vec2};

use crate::app::{Action, App};
use crate::icon;
use crate::theme;

const NAV: [(&str, &str, bool); 4] = [
    (icon::DESKTOP, "Hosts", true),
    (icon::CODE, "Snippets", false),
    (icon::ARROWS_LEFT_RIGHT, "Tunnels", false),
    (icon::CLOCK_HISTORY, "History", true),
];

const FOOTER: [(&str, &str); 2] = [(icon::GEAR, "Settings"), (icon::LIST, "Activity log")];

pub fn show(app: &mut App, ui: &mut Ui) -> Option<Action> {
    let mut action = None;

    ui.add_space(theme::S3);
    ui.vertical_centered(|ui| {
        for (glyph, tooltip, enabled) in NAV {
            let active = match tooltip {
                "Hosts" => app.active.is_none(),
                "History" => app.show_history,
                _ => false,
            };
            let clicked = icon(ui, glyph, active, enabled)
                .on_hover_text(tooltip)
                .clicked();

            if clicked && enabled {
                match tooltip {
                    "Hosts" if app.active.is_some() => {
                        action = Some(Action::SelectTab(None));
                    }
                    "History" => action = Some(Action::ToggleHistory),
                    _ => {}
                }
            }
            ui.add_space(theme::S1 + 2.0);
        }
    });

    ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
        ui.add_space(theme::S3);
        for (glyph, tooltip) in FOOTER {
            let response = icon(ui, glyph, false, true).on_hover_text(tooltip);
            if response.clicked() && tooltip == "Activity log" {
                app.show_log = !app.show_log;
            }
            ui.add_space(theme::S1 + 2.0);
        }
    });

    action
}

fn icon(ui: &mut Ui, glyph: &str, active: bool, enabled: bool) -> egui::Response {
    let size = Vec2::splat(36.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let (bg, fg) = match (active, enabled, response.hovered()) {
        (true, _, _) => (theme::BG_OVERLAY, theme::ACCENT),
        (_, false, _) => (Color32::TRANSPARENT, theme::tint(theme::TEXT_FAINT, 90)),
        (_, true, true) => (theme::BG_SURFACE, theme::TEXT),
        _ => (Color32::TRANSPARENT, theme::TEXT_FAINT),
    };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(theme::R_MD), bg);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(15.0),
        fg,
    );

    if active {
        let tick = Rect::from_min_size(
            rect.left_center() - Vec2::new(6.0, 8.0),
            Vec2::new(2.5, 16.0),
        );
        ui.painter()
            .rect_filled(tick, CornerRadius::same(2), theme::ACCENT);
    }

    response
}
