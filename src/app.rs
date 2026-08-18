use std::collections::HashSet;

use egui::{Color32, RichText, Stroke, Ui};

use crate::backend::{self, Command, Event, RemoteEntry, WorkerHandle};
use crate::connection::{Auth, ConnectionProfile, Protocol};
use crate::theme;

fn host_color(name: &str) -> Color32 {
    let mut hash: i32 = 0;
    for c in name.chars() {
        hash = hash.wrapping_mul(31).wrapping_add(c as i32);
    }
    let idx = (hash.unsigned_abs() as usize) % theme::HOST_COLORS.len();
    theme::HOST_COLORS[idx]
}

fn relative_time(secs_ago: u64) -> String {
    if secs_ago < 60 {
        "just now".to_string()
    } else if secs_ago < 3600 {
        format!("{}m ago", secs_ago / 60)
    } else if secs_ago < 86400 {
        format!("{}h ago", secs_ago / 3600)
    } else {
        format!("{}d ago", secs_ago / 86400)
    }
}

struct SavedConnection {
    profile: ConnectionProfile,
    auth: Auth,
    connected_at: std::time::Instant,
}

struct ActiveSession {
    profile: ConnectionProfile,
    worker: WorkerHandle,
    cwd: String,
    entries: Vec<RemoteEntry>,
    selected: HashSet<usize>,
    last_clicked: Option<usize>,
    connected: bool,
    progress: Option<(u64, u64, String)>,
}

pub struct App {
    saved: Vec<SavedConnection>,
    search_query: String,
    show_form: bool,
    form: ConnectionProfile,
    password_buf: String,
    key_path_buf: String,
    key_pass_buf: String,
    s3_access_buf: String,
    s3_secret_buf: String,

    active: Option<ActiveSession>,
    log: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            saved: Vec::new(),
            search_query: String::new(),
            show_form: false,
            form: ConnectionProfile::default(),
            password_buf: String::new(),
            key_path_buf: String::new(),
            key_pass_buf: String::new(),
            s3_access_buf: String::new(),
            s3_secret_buf: String::new(),
            active: None,
            log: vec!["Ready.".into()],
        }
    }
}

impl App {
    fn log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 300 {
            self.log.drain(0..self.log.len() - 300);
        }
    }

    fn auth_from_form(&self, protocol: Protocol) -> Auth {
        match protocol {
            Protocol::Ftp | Protocol::Ssh | Protocol::Sftp | Protocol::Scp => {
                if !self.key_path_buf.is_empty() {
                    Auth::KeyFile {
                        path: self.key_path_buf.clone(),
                        passphrase: self.key_pass_buf.clone(),
                    }
                } else {
                    Auth::Password(self.password_buf.clone())
                }
            }
            Protocol::S3 => Auth::S3Keys {
                access_key: self.s3_access_buf.clone(),
                secret_key: self.s3_secret_buf.clone(),
            },
        }
    }

    fn connect_profile(&mut self, mut profile: ConnectionProfile, auth: Auth) {
        profile.auth = auth;
        let start_dir = profile.remote_start_dir.clone();
        let worker = backend::spawn(profile.clone());
        self.log(format!(
            "Connecting to {} via {}...",
            profile.host, profile.protocol
        ));
        self.active = Some(ActiveSession {
            profile,
            worker,
            cwd: start_dir,
            entries: Vec::new(),
            selected: HashSet::new(),
            last_clicked: None,
            connected: false,
            progress: None,
        });
    }

    fn save_and_connect(&mut self) {
        let profile = self.form.clone();
        let auth = self.auth_from_form(profile.protocol);
        self.saved.push(SavedConnection {
            profile: profile.clone(),
            auth: auth.clone(),
            connected_at: std::time::Instant::now(),
        });
        self.show_form = false;
        self.connect_profile(profile, auth);
    }

    fn poll_worker(&mut self, ctx: &egui::Context) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let mut messages = Vec::new();
        let mut disconnected = false;

        while let Ok(evt) = active.worker.rx.try_recv() {
            match evt {
                Event::Connected => {
                    active.connected = true;
                    messages.push("Connected.".to_string());
                    active
                        .worker
                        .tx
                        .send(Command::List {
                            path: active.cwd.clone(),
                        })
                        .ok();
                }
                Event::ConnectFailed(e) => {
                    messages.push(format!("Connect failed: {e}"));
                    disconnected = true;
                }
                Event::Listing { path, entries } => {
                    active.cwd = path;
                    active.entries = entries;
                    active.entries.sort_by(|a, b| {
                        b.is_dir
                            .cmp(&a.is_dir)
                            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    });
                    active.selected.clear();
                    active.last_clicked = None;
                }
                Event::Progress {
                    transferred,
                    total,
                    label,
                } => {
                    active.progress = Some((transferred, total, label));
                }
                Event::TransferDone { label } => {
                    active.progress = None;
                    messages.push(format!("Done: {label}"));
                    let cwd = active.cwd.clone();
                    active.worker.tx.send(Command::List { path: cwd }).ok();
                }
                Event::ExecOutput(out) => messages.push(format!("Output:\n{out}")),
                Event::Error(e) => messages.push(format!("Error: {e}")),
                Event::Disconnected => {
                    messages.push("Disconnected.".to_string());
                    disconnected = true;
                }
            }
        }

        for m in messages {
            self.log(m);
        }
        if disconnected {
            self.active = None;
        }
        ctx.request_repaint();
    }

    fn rail(&mut self, ui: &mut Ui) {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            rail_icon(ui, "\u{25A3}", true); // Hosts (active)
            ui.add_space(6.0);
            rail_icon(ui, "{ }", false); // Snippets
            ui.add_space(6.0);
            rail_icon(ui, "\u{21C4}", false); // Tunnels
            ui.add_space(6.0);
            rail_icon(ui, "\u{21BB}", false); // History
        });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            rail_icon(ui, "\u{00BB}", false); // collapse chevron
            ui.add_space(6.0);
            rail_icon(ui, "\u{2699}", false); // settings gear
            ui.add_space(6.0);
            rail_icon(ui, "\u{21C5}", false); // sort
        });
    }

    fn hosts_dashboard(&mut self, ui: &mut Ui) {
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - 24.0);

                let tab = egui::Button::new(RichText::new("Hosts").color(theme::ACCENT))
                    .fill(theme::BG_SURFACE)
                    .stroke(Stroke::new(1.0, theme::ACCENT))
                    .corner_radius(8);
                ui.add(tab);

                ui.add_space(18.0);
                ui.label(RichText::new("Hosts").color(theme::TEXT_PRIMARY).size(26.0).strong());
                ui.label(
                    RichText::new("Manage your saved servers, organize them into groups, and connect with one click")
                        .color(theme::TEXT_MUTED),
                );

                ui.add_space(18.0);
                egui::Frame::default()
                    .fill(theme::BG_SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(10)
                    .inner_margin(10)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Search").color(theme::TEXT_MUTED));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.search_query)
                                    .hint_text("Search hosts...")
                                    .desired_width(f32::INFINITY),
                            );
                        });
                    });

                if let Some(recent) = self.saved.last() {
                    ui.add_space(18.0);
                    ui.label(RichText::new("RECENT").color(theme::TEXT_MUTED).small().strong());
                    ui.add_space(6.0);

                    let secs = recent.connected_at.elapsed().as_secs();
                    let frame = egui::Frame::default().fill(theme::BG_SURFACE).corner_radius(20).inner_margin(egui::Margin::symmetric(12, 6));
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 4.0, theme::ACCENT);
                            ui.label(RichText::new(&recent.profile.name).color(theme::TEXT_PRIMARY));
                            ui.label(RichText::new(relative_time(secs)).color(theme::TEXT_MUTED).small());
                        });
                    });
                }

                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if dashboard_action_button(ui, "+", "NEW SERVER") {
                        self.form = ConnectionProfile::default();
                        self.form.protocol = Protocol::Sftp;
                        self.show_form = true;
                    }
                    if dashboard_action_button(ui, "\u{2601}", "NEW S3") {
                        self.form = ConnectionProfile::default();
                        self.form.protocol = Protocol::S3;
                        self.form.port = Protocol::S3.default_port();
                        self.show_form = true;
                    }
                    ui.add_enabled(false, egui::Button::new("+ NEW GROUP").corner_radius(8));
                    ui.add_enabled(false, egui::Button::new("\u{2913} IMPORT").corner_radius(8));
                });

                if self.show_form {
                    ui.add_space(16.0);
                    self.connect_form(ui);
                }

                ui.add_space(24.0);
                ui.label(RichText::new("HOSTS").color(theme::TEXT_MUTED).small().strong());
                ui.add_space(10.0);

                let query = self.search_query.to_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let mut to_connect = None;
                        let mut to_remove = None;
                        for (i, saved) in self.saved.iter().enumerate() {
                            if !query.is_empty() && !saved.profile.name.to_lowercase().contains(&query) {
                                continue;
                            }
                            if let Some(action) = host_card(ui, i, saved) {
                                match action {
                                    CardAction::Connect => to_connect = Some(i),
                                    CardAction::Delete => to_remove = Some(i),
                                }
                            }
                        }
                        if let Some(i) = to_connect {
                            let profile = self.saved[i].profile.clone();
                            let auth = self.saved[i].auth.clone();
                            self.connect_profile(profile, auth);
                        }
                        if let Some(i) = to_remove {
                            self.saved.remove(i);
                        }
                    });
                });
            });
        });
    }

    fn connect_form(&mut self, ui: &mut Ui) {
        egui::Frame::default()
            .fill(theme::BG_SURFACE)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(12)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_max_width(420.0);
                ui.label(
                    RichText::new("NEW CONNECTION")
                        .color(theme::TEXT_MUTED)
                        .small()
                        .strong(),
                );
                ui.add_space(8.0);

                egui::Grid::new("conn_form")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Name").color(theme::TEXT_SECONDARY));
                        ui.text_edit_singleline(&mut self.form.name);
                        ui.end_row();

                        ui.label(RichText::new("Protocol").color(theme::TEXT_SECONDARY));
                        egui::ComboBox::from_id_salt("protocol")
                            .selected_text(self.form.protocol.label())
                            .show_ui(ui, |ui| {
                                for p in Protocol::ALL {
                                    if ui
                                        .selectable_value(&mut self.form.protocol, p, p.label())
                                        .clicked()
                                    {
                                        self.form.port = p.default_port();
                                    }
                                }
                            });
                        ui.end_row();

                        if self.form.protocol != Protocol::S3 {
                            ui.label(RichText::new("Host").color(theme::TEXT_SECONDARY));
                            ui.text_edit_singleline(&mut self.form.host);
                            ui.end_row();

                            ui.label(RichText::new("Port").color(theme::TEXT_SECONDARY));
                            ui.add(egui::DragValue::new(&mut self.form.port).range(1..=65535));
                            ui.end_row();

                            ui.label(RichText::new("Username").color(theme::TEXT_SECONDARY));
                            ui.text_edit_singleline(&mut self.form.username);
                            ui.end_row();

                            ui.label(RichText::new("Password").color(theme::TEXT_SECONDARY));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.password_buf).password(true),
                            );
                            ui.end_row();

                            ui.label(RichText::new("Key file (opt.)").color(theme::TEXT_SECONDARY));
                            ui.text_edit_singleline(&mut self.key_path_buf);
                            ui.end_row();

                            if !self.key_path_buf.is_empty() {
                                ui.label(
                                    RichText::new("Key passphrase").color(theme::TEXT_SECONDARY),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.key_pass_buf)
                                        .password(true),
                                );
                                ui.end_row();
                            }
                        } else {
                            ui.label(RichText::new("Bucket").color(theme::TEXT_SECONDARY));
                            ui.text_edit_singleline(&mut self.form.bucket);
                            ui.end_row();

                            ui.label(RichText::new("Region").color(theme::TEXT_SECONDARY));
                            ui.text_edit_singleline(&mut self.form.region);
                            ui.end_row();

                            ui.label(RichText::new("Access key").color(theme::TEXT_SECONDARY));
                            ui.text_edit_singleline(&mut self.s3_access_buf);
                            ui.end_row();

                            ui.label(RichText::new("Secret key").color(theme::TEXT_SECONDARY));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.s3_secret_buf).password(true),
                            );
                            ui.end_row();
                        }

                        ui.label(RichText::new("Start dir").color(theme::TEXT_SECONDARY));
                        ui.text_edit_singleline(&mut self.form.remote_start_dir);
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let connect_btn = egui::Button::new(
                        RichText::new("Save & Connect")
                            .color(theme::TEXT_PRIMARY)
                            .strong(),
                    )
                    .fill(theme::ACCENT)
                    .corner_radius(6);
                    if ui.add(connect_btn).clicked() {
                        self.save_and_connect();
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_form = false;
                    }
                });
            });
    }

    fn central_panel(&mut self, ui: &mut Ui) {
        let Some(active) = self.active.as_mut() else {
            self.hosts_dashboard(ui);
            return;
        };

        ui.horizontal(|ui| {
            if ui.button("\u{2190} Hosts").clicked() {
                active.worker.tx.send(Command::Disconnect).ok();
            }
            ui.label(
                RichText::new(&active.profile.name)
                    .color(theme::TEXT_PRIMARY)
                    .strong(),
            );
            let (dot, color) = if active.connected {
                ("\u{25CF}", theme::STATUS_CONNECTED)
            } else {
                ("\u{25CB}", theme::STATUS_CONNECTING)
            };
            ui.label(RichText::new(dot).color(color));
        });
        ui.add_space(6.0);

        breadcrumb_bar(ui, &active.cwd, &active.worker.tx);
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let single = active.selected.len() == 1;
            if ui
                .add_enabled(single, egui::Button::new("Download"))
                .clicked()
            {
                if let Some(&i) = active.selected.iter().next() {
                    if let Some(entry) = active.entries.get(i).cloned() {
                        if !entry.is_dir {
                            if let Some(dest) = rfd::FileDialog::new()
                                .set_file_name(&entry.name)
                                .save_file()
                            {
                                let remote_path = join_path(&active.cwd, &entry.name);
                                active
                                    .worker
                                    .tx
                                    .send(Command::Download {
                                        remote_path,
                                        local_path: dest,
                                    })
                                    .ok();
                            }
                        }
                    }
                }
            }
            if ui.button("Upload").clicked() {
                if let Some(src) = rfd::FileDialog::new().pick_file() {
                    if let Some(fname) = src.file_name().and_then(|s| s.to_str()) {
                        let remote_path = join_path(&active.cwd, fname);
                        active
                            .worker
                            .tx
                            .send(Command::Upload {
                                local_path: src,
                                remote_path,
                            })
                            .ok();
                    }
                }
            }
            if ui.button("New folder").clicked() {
                let path = join_path(&active.cwd, "new_folder");
                active.worker.tx.send(Command::Mkdir { path }).ok();
            }
            let any_selected = !active.selected.is_empty();
            let delete_label = if active.selected.len() > 1 {
                format!("Delete ({})", active.selected.len())
            } else {
                "Delete".to_string()
            };
            if ui
                .add_enabled(any_selected, egui::Button::new(delete_label))
                .clicked()
            {
                for &i in &active.selected {
                    if let Some(entry) = active.entries.get(i) {
                        let path = join_path(&active.cwd, &entry.name);
                        active
                            .worker
                            .tx
                            .send(Command::Delete {
                                path,
                                is_dir: entry.is_dir,
                            })
                            .ok();
                    }
                }
            }
        });

        if let Some((done, total, label)) = &active.progress {
            ui.add_space(4.0);
            let frac = if *total > 0 {
                *done as f32 / *total as f32
            } else {
                0.0
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .text(format!("{label}: {done}/{total} bytes"))
                    .fill(theme::ACCENT),
            );
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("file_table")
                .striped(false)
                .num_columns(3)
                .min_col_width(0.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("NAME")
                            .color(theme::TEXT_MUTED)
                            .small()
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("SIZE")
                                .color(theme::TEXT_MUTED)
                                .small()
                                .strong(),
                        );
                    });
                    ui.label(
                        RichText::new("MODIFIED")
                            .color(theme::TEXT_MUTED)
                            .small()
                            .strong(),
                    );
                    ui.end_row();

                    let entries = active.entries.clone();
                    for (i, entry) in entries.iter().enumerate() {
                        let selected = active.selected.contains(&i);
                        if selected {
                            let row_rect = ui.cursor();
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    row_rect.min,
                                    egui::vec2(ui.available_width(), 22.0),
                                ),
                                4,
                                theme::accent_tint(26),
                            );
                        }

                        let name_text = if entry.is_dir {
                            RichText::new(format!("{}/", entry.name)).color(theme::TEXT_PRIMARY)
                        } else {
                            RichText::new(&entry.name).color(theme::TEXT_PRIMARY)
                        };
                        let label = ui.selectable_label(false, name_text);

                        if label.clicked() {
                            let modifiers = ui.ctx().input(|inp| inp.modifiers);
                            if modifiers.shift
                                && let Some(anchor) = active.last_clicked
                            {
                                let (lo, hi) = if anchor <= i {
                                    (anchor, i)
                                } else {
                                    (i, anchor)
                                };
                                for j in lo..=hi {
                                    active.selected.insert(j);
                                }
                            } else if modifiers.command || modifiers.ctrl {
                                if !active.selected.insert(i) {
                                    active.selected.remove(&i);
                                }
                                active.last_clicked = Some(i);
                            } else {
                                active.selected.clear();
                                active.selected.insert(i);
                                active.last_clicked = Some(i);
                            }
                        }
                        if label.double_clicked() && entry.is_dir {
                            let path = join_path(&active.cwd, &entry.name);
                            active.worker.tx.send(Command::List { path }).ok();
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let size_text = if entry.is_dir {
                                "\u{2014}".to_string()
                            } else {
                                human_size(entry.size)
                            };
                            ui.label(
                                RichText::new(size_text)
                                    .color(theme::TEXT_MUTED)
                                    .monospace()
                                    .small(),
                            );
                        });
                        ui.label(
                            RichText::new(entry.modified.clone().unwrap_or_default())
                                .color(theme::TEXT_MUTED)
                                .small(),
                        );
                        ui.end_row();
                    }
                });
        });
    }

    fn bottom_panel(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(110.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.log {
                    let color = if line.starts_with("Error") || line.starts_with("Connect failed") {
                        theme::err_color()
                    } else {
                        theme::TEXT_MUTED
                    };
                    ui.label(RichText::new(line).color(color).small().monospace());
                }
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_worker(&ctx);

        egui::Panel::left("rail")
            .resizable(false)
            .default_size(60.0)
            .show(ui, |ui| {
                self.rail(ui);
            });

        egui::Panel::bottom("bottom")
            .resizable(true)
            .default_size(110.0)
            .show(ui, |ui| {
                self.bottom_panel(ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            self.central_panel(ui);
        });
    }
}

enum CardAction {
    Connect,
    Delete,
}

fn host_card(ui: &mut Ui, index: usize, saved: &SavedConnection) -> Option<CardAction> {
    let display_name = &saved.profile.name;
    let avatar_color = host_color(display_name);
    let initial = display_name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    let mut action = None;

    let frame = egui::Frame::default()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(16)
        .inner_margin(14);

    let inner = frame.show(ui, |ui| {
        ui.set_width(320.0);

        let glow_rect = ui.max_rect();
        ui.painter().circle_filled(
            glow_rect.left_top() + egui::vec2(30.0, 20.0),
            70.0,
            Color32::from_rgba_unmultiplied(
                avatar_color.r(),
                avatar_color.g(),
                avatar_color.b(),
                18,
            ),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if ui
                .small_button("\u{25B4}")
                .on_hover_text("Explorer")
                .clicked()
            {
                action = Some(CardAction::Connect);
            }
            ui.add_enabled(false, egui::Button::new(">_"))
                .on_hover_text("Terminal (not implemented)");
            ui.add_enabled(false, egui::Button::new("\u{2022}"))
                .on_hover_text("Ping (not implemented)");
        });

        ui.add_space(4.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
        ui.painter().circle_filled(
            rect.center(),
            18.0,
            Color32::from_rgba_unmultiplied(
                avatar_color.r(),
                avatar_color.g(),
                avatar_color.b(),
                40,
            ),
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initial,
            egui::FontId::proportional(14.0),
            avatar_color,
        );

        ui.add_space(6.0);
        ui.label(RichText::new(display_name).color(theme::TEXT_PRIMARY));
        ui.label(
            RichText::new(format!(
                "{}, {}",
                saved.profile.protocol, saved.profile.host
            ))
            .color(theme::TEXT_MUTED)
            .small()
            .monospace(),
        );

        if ui.small_button("\u{00D7} remove").clicked() {
            action = Some(CardAction::Delete);
        }
    });

    let card_id = ui.id().with(("host_card", index));
    let card_response = ui.interact(inner.response.rect, card_id, egui::Sense::click());
    if card_response.hovered() {
        ui.painter().rect_stroke(
            inner.response.rect,
            egui::CornerRadius::same(16),
            Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Outside,
        );
    }
    if card_response.clicked() && action.is_none() {
        action = Some(CardAction::Connect);
    }
    ui.add_space(10.0);

    action
}

fn dashboard_action_button(ui: &mut Ui, icon: &str, label: &str) -> bool {
    let btn = egui::Button::new(format!("{icon}  {label}"))
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(8);
    ui.add(btn).clicked()
}

fn rail_icon(ui: &mut Ui, glyph: &str, active: bool) {
    let (bg, fg) = if active {
        (theme::BG_OVERLAY, theme::ACCENT)
    } else {
        (theme::BG_BASE, theme::TEXT_MUTED)
    };
    let btn = egui::Button::new(RichText::new(glyph).color(fg))
        .fill(bg)
        .corner_radius(8);
    ui.add_sized([36.0, 36.0], btn);
}

fn breadcrumb_bar(ui: &mut Ui, cwd: &str, tx: &std::sync::mpsc::Sender<Command>) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Home").clicked() {
            let _ = tx.send(Command::List {
                path: "/".to_string(),
            });
        }
        let segments: Vec<&str> = cwd
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut built = String::new();
        for (i, segment) in segments.iter().enumerate() {
            built.push('/');
            built.push_str(segment);
            ui.label(RichText::new("/").color(theme::TEXT_MUTED));
            let is_last = i == segments.len() - 1;
            if is_last {
                ui.label(RichText::new(*segment).color(theme::TEXT_PRIMARY).strong());
            } else if ui
                .button(RichText::new(*segment).color(theme::TEXT_MUTED))
                .clicked()
            {
                tx.send(Command::List {
                    path: built.clone(),
                })
                .ok();
            }
        }
    });
}

fn join_path(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
