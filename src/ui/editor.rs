use egui::{RichText, Ui};

use crate::app::Action;
use crate::connection::{Auth, ConnectionProfile, Protocol};
use crate::theme;
use crate::ui::widgets;

pub fn show(ui: &mut Ui, profile: &mut ConnectionProfile, editing: bool) -> Option<Action> {
    let mut action = None;

    widgets::card_frame(theme::R_LG).show(ui, |ui| {
        ui.set_max_width(460.0);
        widgets::section_label(ui, if editing { "EDIT HOST" } else { "NEW HOST" });
        ui.add_space(theme::S3);

        egui::Grid::new("host_form")
            .num_columns(2)
            .spacing([theme::S3, theme::S2])
            .min_col_width(96.0)
            .show(ui, |ui| {
                field(ui, "Name");
                ui.add(
                    egui::TextEdit::singleline(&mut profile.name)
                        .hint_text("optional label")
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();

                field(ui, "Protocol");
                protocol_picker(ui, profile);
                ui.end_row();

                if profile.protocol.is_object_store() {
                    s3_fields(ui, profile);
                } else {
                    host_fields(ui, profile);
                }

                field(ui, "Start dir");
                ui.add(
                    egui::TextEdit::singleline(&mut profile.remote_start_dir)
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();
            });

        ui.add_space(theme::S3);
        ui.horizontal(|ui| {
            let ready = profile.is_connectable();
            if widgets::primary_button(ui, "Save & Connect", ready).clicked() {
                action = Some(Action::SaveDraftAndConnect);
            }
            if widgets::secondary_button(ui, "Save").clicked() {
                action = Some(Action::SaveDraft);
            }
            if widgets::ghost_button(ui, "Cancel", true).clicked() {
                action = Some(Action::CancelEdit);
            }
            if !ready {
                ui.label(
                    RichText::new(if profile.protocol.is_object_store() {
                        "Bucket is required"
                    } else {
                        "Host is required"
                    })
                    .color(theme::TEXT_FAINT)
                    .small(),
                );
            }
        });
    });

    action
}

fn field(ui: &mut Ui, label: &str) {
    ui.label(RichText::new(label).color(theme::TEXT_DIM));
}

fn protocol_picker(ui: &mut Ui, profile: &mut ConnectionProfile) {
    egui::ComboBox::from_id_salt("protocol_picker")
        .selected_text(profile.protocol.label())
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for candidate in Protocol::ALL {
                let mut selected = profile.protocol;
                if ui
                    .selectable_value(&mut selected, candidate, candidate.label())
                    .clicked()
                {
                    profile.set_protocol(candidate);
                }
            }
        });
}

fn host_fields(ui: &mut Ui, profile: &mut ConnectionProfile) {
    field(ui, "Host");
    ui.vertical(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut profile.host)
                .hint_text("exemple.com")
                .desired_width(f32::INFINITY),
        );
        ui.label(
            RichText::new(format!(
                "port {} by default \u{2014} add :port to override",
                profile.protocol.default_port()
            ))
            .color(theme::TEXT_FAINT)
            .small(),
        );
    });
    ui.end_row();

    field(ui, "Username");
    ui.add(egui::TextEdit::singleline(&mut profile.username).desired_width(f32::INFINITY));
    ui.end_row();

    let use_key = matches!(profile.auth, Auth::KeyFile { .. });

    field(ui, "Auth");
    ui.horizontal(|ui| {
        if ui.selectable_label(!use_key, "Password").clicked() && use_key {
            profile.auth = Auth::Password(String::new());
        }
        if ui.selectable_label(use_key, "Key file").clicked() && !use_key {
            profile.auth = Auth::KeyFile {
                path: String::new(),
                passphrase: String::new(),
            };
        }
    });
    ui.end_row();

    match &mut profile.auth {
        Auth::Password(password) => {
            field(ui, "Password");
            ui.add(
                egui::TextEdit::singleline(password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();
        }
        Auth::KeyFile { path, passphrase } => {
            field(ui, "Key file");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(path)
                        .hint_text("~/.ssh/id_ed25519")
                        .desired_width(ui.available_width() - 44.0),
                );
                if ui
                    .small_button("\u{2026}")
                    .on_hover_text("Browse")
                    .clicked()
                    && let Some(picked) = rfd::FileDialog::new().pick_file()
                {
                    *path = picked.to_string_lossy().into_owned();
                }
            });
            ui.end_row();

            field(ui, "Passphrase");
            ui.add(
                egui::TextEdit::singleline(passphrase)
                    .password(true)
                    .hint_text("if the key is encrypted")
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();
        }
        Auth::S3Keys { .. } => {}
    }
}

fn s3_fields(ui: &mut Ui, profile: &mut ConnectionProfile) {
    field(ui, "Bucket");
    ui.add(egui::TextEdit::singleline(&mut profile.bucket).desired_width(f32::INFINITY));
    ui.end_row();

    field(ui, "Region");
    ui.add(egui::TextEdit::singleline(&mut profile.region).desired_width(f32::INFINITY));
    ui.end_row();

    if let Auth::S3Keys {
        access_key,
        secret_key,
    } = &mut profile.auth
    {
        field(ui, "Access key");
        ui.add(egui::TextEdit::singleline(access_key).desired_width(f32::INFINITY));
        ui.end_row();

        field(ui, "Secret key");
        ui.add(
            egui::TextEdit::singleline(secret_key)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        ui.end_row();
    }
}
