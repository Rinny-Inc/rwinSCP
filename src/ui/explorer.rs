use egui::{Align2, CornerRadius, FontId, RichText, Sense, Ui, Vec2};

use crate::app::{Action, App, Session, Status, human_size, parent_path};
use crate::icon;
use crate::theme;
use crate::ui::{keep, widgets};

const ROW_HEIGHT: f32 = 26.0;
const SIZE_COL: f32 = 96.0;
const MODIFIED_COL: f32 = 148.0;

pub fn show(app: &mut App, ui: &mut Ui) -> Option<Action> {
    let mut action = None;

    keep(&mut action, header(ui, app.session()?));
    ui.add_space(theme::S3);
    {
        let session = app.session_mut()?;
        let cwd = session.cwd.clone();
        keep(
            &mut action,
            breadcrumbs(ui, &cwd, session.path_edit.as_mut()),
        );
    }
    ui.add_space(theme::S2);
    keep(&mut action, toolbar(ui, app.session()?));
    {
        let session = app.session_mut()?;
        if let Some((og, edited)) = &mut session.rename {
            let og = og.clone();
            ui.add_space(theme::S2);
            keep(&mut action, rename_bar(ui, &og, edited));
        }
    }
    let session = app.session()?;
    ui.add_space(theme::S3);
    keep(&mut action, table(ui, session));

    let hovering = ui.ctx().input(|i| i.raw.hovered_files.len());
    if hovering > 0 {
        let screen = ui
            .ctx()
            .input(|i| i.raw.screen_rect.unwrap_or(ui.max_rect()));
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop_overlay"),
        ));
        painter.rect_filled(screen, 0, theme::tint(theme::ACCENT, 26));
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            format!("Drop {hovering} item(s) into {}", session.cwd),
            egui::FontId::proportional(18.0),
            theme::TEXT,
        );
    }

    action
}

fn header(ui: &mut Ui, session: &Session) -> Option<Action> {
    let mut action = None;

    ui.horizontal(|ui| {
        if widgets::ghost_button(ui, &format!("{}  Hosts", icon::ARROW_LEFT), true).clicked() {
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

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::ghost_button(ui, icon::ARROW_CLOCKWISE, true)
                .on_hover_text("Refresh")
                .clicked()
            {
                action = Some(Action::Refresh);
            }
        });
    });

    action
}

fn breadcrumbs(ui: &mut Ui, cwd: &str, editing: Option<&mut String>) -> Option<Action> {
    let mut action = None;

    if let Some(buffer) = editing {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::FOLDER).color(theme::ACCENT));

            let field = ui.add(
                egui::TextEdit::singleline(buffer)
                    .id_salt("path_bar")
                    .font(egui::TextStyle::Monospace)
                    .hint_text("/path/to/somewhere")
                    .desired_width(ui.available_width() - 90.0),
            );

            if !field.has_focus() {
                field.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                    let all = egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(buffer.chars().count()),
                    );
                    state.cursor.set_char_range(Some(all));
                    state.store(ui.ctx(), field.id);
                }
            }

            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter && (field.has_focus() || field.lost_focus()) {
                action = Some(Action::CommitPath);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Some(Action::CancelPath);
            }
        });

        return action;
    }

    ui.horizontal_wrapped(|ui| {
        if widgets::ghost_button(ui, "/", true)
            .on_hover_text("Root")
            .clicked()
        {
            action = Some(Action::Navigate("/".to_owned()));
        }

        let segments: Vec<&str> = cwd.split('/').filter(|s| !s.is_empty()).collect();
        let mut path = String::new();

        for (i, segment) in segments.iter().enumerate() {
            path.push('/');
            path.push_str(segment);

            ui.label(RichText::new(icon::CARET_RIGHT).color(theme::TEXT_FAINT));

            if i + 1 == segments.len() {
                ui.label(RichText::new(*segment).color(theme::TEXT).strong());
            } else if widgets::ghost_button(ui, segment, true).clicked() {
                action = Some(Action::Navigate(path.clone()));
            }
        }

        ui.add_space(theme::S2);
        if widgets::ghost_button(ui, icon::PENCIL, true)
            .on_hover_text("Type a path")
            .clicked()
        {
            action = Some(Action::EditPath);
        }
    });

    action
}

fn toolbar(ui: &mut Ui, session: &Session) -> Option<Action> {
    let mut action = None;
    let selected = session.selection.len();
    let selected_count = session.selection.len();

    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                session.cwd != "/",
                egui::Button::new(format!("{} Up", icon::ARROW_UP)).corner_radius(theme::R_SM),
            )
            .clicked()
        {
            action = Some(Action::Navigate(parent_path(&session.cwd)));
        }

        widgets::divider_spacer(ui);

        if ui
            .add_enabled(
                selected_count == 1,
                egui::Button::new(format!("{} Rename", icon::PENCIL)).corner_radius(theme::R_SM),
            )
            .on_hover_text("Rename the selected item (F2)")
            .clicked()
        {
            action = Some(Action::BeginRename);
        }

        let download_label = if selected_count > 1 {
            format!("{} Download ({selected_count})", icon::DOWNLOAD)
        } else {
            format!("{} Download", icon::DOWNLOAD)
        };

        if ui
            .add_enabled(
                selected_count > 0,
                egui::Button::new(download_label).corner_radius(theme::R_SM),
            )
            .clicked()
        {
            action = Some(Action::Download);
        }
        if ui
            .add(egui::Button::new(format!("{} Upload", icon::UPLOAD)).corner_radius(theme::R_SM))
            .on_hover_text("Upload files (or drop them on the window)")
            .clicked()
        {
            action = Some(Action::Upload);
        }
        if ui
            .add(
                egui::Button::new(format!("{} Upload folder", icon::UPLOAD))
                    .corner_radius(theme::R_SM),
            )
            .on_hover_text("Upload whole folders, contents & all")
            .clicked()
        {
            action = Some(Action::UploadFolder);
        }
        if ui
            .add(
                egui::Button::new(format!("{} New folder", icon::FOLDER_PLUS))
                    .corner_radius(theme::R_SM),
            )
            .clicked()
        {
            action = Some(Action::Mkdir);
        }

        let delete_label = if selected > 1 {
            format!("{} Delete ({selected})", icon::TRASH)
        } else {
            format!("{} Delete", icon::TRASH)
        };
        if ui
            .add_enabled(
                selected > 0,
                egui::Button::new(RichText::new(delete_label).color(if selected > 0 {
                    theme::DANGER
                } else {
                    theme::TEXT_FAINT
                }))
                .corner_radius(theme::R_SM),
            )
            .clicked()
        {
            action = Some(Action::DeleteSelected);
        }
    });

    action
}

fn table(ui: &mut Ui, session: &Session) -> Option<Action> {
    let mut action = None;

    header_row(ui);
    widgets::divider(ui);

    if session.loading && session.entries.is_empty() {
        widgets::empty_state(ui, "Loading\u{2026}", "Fetching the directory listing");
        return None;
    }

    if !session.profile.protocol.browsable() {
        widgets::empty_state(
            ui,
            "No file browser for SSH",
            "Plain SSH sessions are shell-only; use SFTP or SCP to transfer files",
        );
        return None;
    }

    if session.entries.is_empty() {
        widgets::empty_state(ui, "Empty directory", "Nothing here yet");
        return None;
    }

    egui::ScrollArea::vertical()
        .id_salt("file_table")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, entry) in session.entries.iter().enumerate() {
                let selected = session.selection.contains(&index);
                let width = ui.available_width();
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());

                if !ui.is_rect_visible(rect) {
                    continue;
                }

                let painter = ui.painter();
                if selected {
                    painter.rect_filled(
                        rect,
                        CornerRadius::same(theme::R_SM),
                        theme::tint(theme::ACCENT, 30),
                    );
                } else if response.hovered() {
                    painter.rect_filled(rect, CornerRadius::same(theme::R_SM), theme::BG_SURFACE);
                }

                let name_width = (width - SIZE_COL - MODIFIED_COL - theme::S4).max(60.0);
                let glyph = if entry.is_dir {
                    icon::FOLDER
                } else {
                    icon::FILE
                };
                let name_color = if entry.is_dir {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                };

                painter.text(
                    rect.left_center() + Vec2::new(theme::S2, 0.0),
                    Align2::LEFT_CENTER,
                    glyph,
                    FontId::proportional(12.0),
                    if entry.is_dir {
                        theme::ACCENT
                    } else {
                        theme::TEXT_FAINT
                    },
                );
                painter.text(
                    rect.left_center() + Vec2::new(theme::S2 + 20.0, 0.0),
                    Align2::LEFT_CENTER,
                    crate::app::ellipsize(&entry.name, (name_width / 7.2) as usize),
                    FontId::proportional(13.5),
                    name_color,
                );

                let size_x = rect.right() - MODIFIED_COL - theme::S2;
                if !entry.is_dir {
                    painter.text(
                        egui::pos2(size_x, rect.center().y),
                        Align2::RIGHT_CENTER,
                        human_size(entry.size),
                        FontId::monospace(11.5),
                        theme::TEXT_FAINT,
                    );
                }

                if let Some(modified) = &entry.modified {
                    painter.text(
                        egui::pos2(rect.right() - theme::S2, rect.center().y),
                        Align2::RIGHT_CENTER,
                        crate::app::ellipsize(modified, 18),
                        FontId::monospace(11.5),
                        theme::TEXT_FAINT,
                    );
                }

                if response.clicked() {
                    action = Some(Action::ClickRow(index, ui.ctx().input(|i| i.modifiers)));
                }
                if response.double_clicked() && entry.is_dir {
                    action = Some(Action::OpenRow(index));
                }
            }
        });

    action
}

fn header_row(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    let painter = ui.painter();
    let font = FontId::proportional(10.5);

    painter.text(
        rect.left_center() + Vec2::new(theme::S2, 0.0),
        Align2::LEFT_CENTER,
        "NAME",
        font.clone(),
        theme::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(rect.right() - MODIFIED_COL - theme::S2, rect.center().y),
        Align2::RIGHT_CENTER,
        "SIZE",
        font.clone(),
        theme::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(rect.right() - theme::S2, rect.center().y),
        Align2::RIGHT_CENTER,
        "MODIFIED",
        font,
        theme::TEXT_FAINT,
    );
}

fn rename_bar(ui: &mut Ui, og: &str, edited: &mut String) -> Option<Action> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.label(RichText::new(icon::PENCIL).color(theme::ACCENT));
        ui.label(
            RichText::new(format!("{og}  \u{2192}"))
                .color(theme::TEXT_FAINT)
                .monospace()
                .small(),
        );

        let field = ui.add(
            egui::TextEdit::singleline(edited)
                .font(egui::TextStyle::Monospace)
                .desired_width(280.0),
        );

        if !field.has_focus() {
            field.request_focus();
        }

        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter && (field.has_focus() || field.lost_focus()) {
            action = Some(Action::CommitRename);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = Some(Action::CancelRename);
        }

        if ui.small_button("Rename").clicked() {
            action = Some(Action::CommitRename);
        }
        if ui.small_button("Cancel").clicked() {
            action = Some(Action::CancelRename);
        }
    });

    action
}
