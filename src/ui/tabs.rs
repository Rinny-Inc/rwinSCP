use crate::app::{Action, App, Status};
use crate::icon;
use crate::theme;
use egui::{RichText, Sense, Ui, Vec2};

pub fn show(app: &App, ui: &mut Ui) -> Option<Action> {
    if app.sessions.is_empty() {
        return None;
    }

    let mut action = None;

    egui::ScrollArea::horizontal()
        .id_salt("tab_strip")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if tab(
                    ui,
                    "hosts",
                    icon::DESKTOP,
                    "Hosts",
                    app.active.is_none(),
                    None,
                )
                .clicked()
                {
                    action = Some(Action::SelectTab(None));
                }

                for (index, session) in app.sessions.iter().enumerate() {
                    let glyph = if session.terminal.is_some() {
                        icon::TERMINAL
                    } else {
                        icon::FOLDER
                    };
                    let status = match session.status {
                        Status::Connected => theme::OK,
                        Status::Connecting => theme::PENDING,
                    };

                    let response = tab(
                        ui,
                        &format!("tab{index}"),
                        glyph,
                        session.profile.display_name(),
                        app.active == Some(index),
                        Some(status),
                    );

                    if response.clicked() {
                        action = Some(Action::SelectTab(Some(index)));
                    }
                    if response.middle_clicked() {
                        action = Some(Action::CloseTab(index));
                    }
                    let close = ui.add(
                        egui::Button::new(RichText::new(icon::X_SMALL).small())
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(theme::R_SM)
                            .min_size(Vec2::splat(18.0)),
                    );
                    if close.on_hover_text("Close tab").clicked() {
                        action = Some(Action::CloseTab(index));
                    }
                }
            });
        });
    ui.add_space(theme::S2);
    action
}

fn tab(
    ui: &mut Ui,
    id_salt: &str,
    glyph: &str,
    label: &str,
    selected: bool,
    status: Option<egui::Color32>,
) -> egui::Response {
    let text_color = if selected {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };

    let inner = egui::Frame::default()
        .fill(if selected {
            theme::BG_SUBTLE
        } else {
            theme::BG_SURFACE
        })
        .stroke(egui::Stroke::new(
            1.0,
            if selected {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ))
        .corner_radius(theme::R_MD)
        .inner_margin(egui::Margin::symmetric(
            theme::S2 as i8,
            theme::S1 as i8 + 1,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(color) = status {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.5, color);
                }
                ui.label(RichText::new(glyph).color(text_color).small());
                ui.label(RichText::new(crate::app::ellipsize(label, 22)).color(text_color));
            });
        });
    ui.interact(
        inner.response.rect,
        ui.id().with(("tab", id_salt)),
        Sense::click(),
    )
}
