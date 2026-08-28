use std::collections::HashSet;
use std::time::Instant;

use crate::backend::{self, Command, Event, RemoteEntry, WorkerHandle};
use crate::connection::{ConnectionProfile, Protocol};
use crate::ui;

pub struct Host {
    pub profile: ConnectionProfile,
    pub last_used: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
}

pub struct Transfer {
    pub label: String,
    pub transferred: u64,
    pub total: u64,
}

impl Transfer {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.transferred as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

pub struct Session {
    pub profile: ConnectionProfile,
    pub worker: WorkerHandle,
    pub status: Status,
    pub cwd: String,
    pub entries: Vec<RemoteEntry>,
    pub selection: HashSet<usize>,
    pub anchor: Option<usize>,
    pub transfer: Option<Transfer>,
    pub loading: bool,
}

impl Session {
    pub fn selected_entries(&self) -> impl Iterator<Item = &RemoteEntry> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(i, _)| self.selection.contains(i))
            .map(|(_, entry)| entry)
    }

    pub fn sole_selection(&self) -> Option<&RemoteEntry> {
        let mut it = self.selected_entries();
        match (it.next(), it.next()) {
            (Some(entry), None) => Some(entry),
            _ => None,
        }
    }

    pub fn click_row(&mut self, index: usize, modifiers: egui::Modifiers) {
        if modifiers.shift
            && let Some(anchor) = self.anchor
        {
            let (lo, hi) = if anchor <= index {
                (anchor, index)
            } else {
                (index, anchor)
            };
            self.selection.extend(lo..=hi);
        } else if modifiers.command || modifiers.ctrl {
            if !self.selection.insert(index) {
                self.selection.remove(&index);
            }
            self.anchor = Some(index);
        } else {
            self.selection.clear();
            self.selection.insert(index);
            self.anchor = Some(index);
        }
    }

    pub fn navigate(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.loading = true;
        self.worker.send(Command::List { path });
    }

    pub fn refresh(&mut self) {
        self.navigate(self.cwd.clone());
    }
}

pub enum Action {
    NewHost(Protocol),
    EditHost(usize),
    DeleteHost(usize),
    Connect(usize),
    CancelEdit,
    SaveDraft,
    SaveDraftAndConnect,
    Disconnect,
    Navigate(String),
    Refresh,
    Download,
    Upload,
    Mkdir,
    DeleteSelected,
    ClickRow(usize, egui::Modifiers),
    OpenRow(usize),
    ClearLog,
}

pub struct LogLine {
    pub text: String,
    pub level: LogLevel,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Success,
    Error,
}

pub struct App {
    pub hosts: Vec<Host>,
    pub search: String,
    pub draft: Option<(ConnectionProfile, Option<usize>)>,
    pub session: Option<Session>,
    pub log: Vec<LogLine>,
    pub show_log: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            search: String::new(),
            draft: None,
            session: None,
            log: Vec::new(),
            show_log: true,
        }
    }
}

const LOG_CAPACITY: usize = 500;

impl App {
    pub fn info(&mut self, text: impl Into<String>) {
        self.push_log(text, LogLevel::Info);
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push_log(text, LogLevel::Error);
    }

    fn push_log(&mut self, text: impl Into<String>, level: LogLevel) {
        self.log.push(LogLine {
            text: text.into(),
            level,
        });
        if self.log.len() > LOG_CAPACITY {
            self.log.drain(..self.log.len() - LOG_CAPACITY);
        }
    }

    pub fn visible_hosts(&self) -> Vec<(usize, &Host)> {
        let needle = self.search.trim().to_lowercase();
        self.hosts
            .iter()
            .enumerate()
            .filter(|(_, host)| {
                needle.is_empty()
                    || host.profile.display_name().to_lowercase().contains(&needle)
                    || host.profile.host.to_lowercase().contains(&needle)
                    || host.profile.bucket.to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub fn most_recent(&self) -> Option<(usize, &Host)> {
        self.hosts
            .iter()
            .enumerate()
            .filter(|(_, host)| host.last_used.is_some())
            .max_by_key(|(_, host)| host.last_used)
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::NewHost(protocol) => {
                self.draft = Some((ConnectionProfile::new(protocol), None));
            }

            Action::EditHost(index) => {
                if let Some(host) = self.hosts.get(index) {
                    self.draft = Some((host.profile.clone(), Some(index)));
                }
            }

            Action::DeleteHost(index) => {
                if index < self.hosts.len() {
                    let host = self.hosts.remove(index);
                    self.info(format!("Removed {}", host.profile.display_name()));
                    if let Some((_, Some(editing))) = &mut self.draft {
                        if *editing == index {
                            self.draft = None;
                        } else if *editing > index {
                            *editing -= 1;
                        }
                    }
                }
            }

            Action::CancelEdit => self.draft = None,

            Action::SaveDraft => {
                self.commit_draft();
            }

            Action::SaveDraftAndConnect => {
                if let Some(index) = self.commit_draft() {
                    self.connect(index);
                }
            }

            Action::Connect(index) => self.connect(index),

            Action::Disconnect => {
                if let Some(session) = &self.session {
                    session.worker.send(Command::Disconnect);
                }
                self.session = None;
            }

            Action::Navigate(path) => {
                if let Some(session) = &mut self.session {
                    session.navigate(path);
                }
            }

            Action::Refresh => {
                if let Some(session) = &mut self.session {
                    session.refresh();
                }
            }

            Action::ClickRow(index, modifiers) => {
                if let Some(session) = &mut self.session {
                    session.click_row(index, modifiers);
                }
            }

            Action::OpenRow(index) => {
                if let Some(session) = &mut self.session
                    && let Some(entry) = session.entries.get(index)
                    && entry.is_dir
                {
                    let path = join_path(&session.cwd, &entry.name);
                    session.navigate(path);
                }
            }

            Action::Download => self.download_selection(),
            Action::Upload => self.upload_file(),

            Action::Mkdir => {
                if let Some(session) = &mut self.session {
                    let path = join_path(&session.cwd, "new-folder");
                    session.worker.send(Command::Mkdir { path });
                    session.refresh();
                }
            }

            Action::DeleteSelected => self.delete_selection(),

            Action::ClearLog => self.log.clear(),
        }
    }

    fn commit_draft(&mut self) -> Option<usize> {
        let (profile, editing) = self.draft.take()?;
        if !profile.is_connectable() {
            self.error("Cannot save: the host or bucket is empty");
            self.draft = Some((profile, editing));
            return None;
        }

        match editing {
            Some(index) if index < self.hosts.len() => {
                self.hosts[index].profile = profile;
                Some(index)
            }
            _ => {
                self.hosts.push(Host {
                    profile,
                    last_used: None,
                });
                Some(self.hosts.len() - 1)
            }
        }
    }

    fn connect(&mut self, index: usize) {
        let Some(host) = self.hosts.get_mut(index) else {
            return;
        };
        host.last_used = Some(Instant::now());

        let profile = host.profile.clone();
        let start_dir = profile.remote_start_dir.clone();
        self.info(format!("Connecting to {}...", profile.endpoint()));

        self.session = Some(Session {
            worker: backend::spawn(profile.clone()),
            profile,
            status: Status::Connecting,
            cwd: start_dir,
            entries: Vec::new(),
            selection: HashSet::new(),
            anchor: None,
            transfer: None,
            loading: true,
        });
    }

    fn download_selection(&mut self) {
        let Some(session) = &self.session else { return };
        let Some(entry) = session.sole_selection() else {
            return;
        };
        if entry.is_dir {
            self.error("Directory download is not supported yet");
            return;
        }

        let remote_path = join_path(&session.cwd, &entry.name);
        let Some(local_path) = rfd::FileDialog::new()
            .set_file_name(&entry.name)
            .save_file()
        else {
            return;
        };

        session.worker.send(Command::Download {
            remote_path,
            local_path,
        });
    }

    fn upload_file(&mut self) {
        let Some(session) = &self.session else { return };
        let Some(local_path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        let Some(name) = local_path.file_name().and_then(|n| n.to_str()) else {
            self.error("That filename is not valid UTF-8");
            return;
        };

        let remote_path = join_path(&session.cwd, name);
        session.worker.send(Command::Upload {
            local_path,
            remote_path,
        });
    }

    fn delete_selection(&mut self) {
        let Some(session) = &self.session else { return };
        let targets: Vec<(String, bool)> = session
            .selected_entries()
            .map(|entry| (join_path(&session.cwd, &entry.name), entry.is_dir))
            .collect();

        for (path, is_dir) in targets {
            session.worker.send(Command::Delete { path, is_dir });
        }

        if let Some(session) = &mut self.session {
            session.refresh();
        }
    }

    fn poll_worker(&mut self, ctx: &egui::Context) {
        let Some(session) = &mut self.session else {
            return;
        };

        let mut logs: Vec<(String, LogLevel)> = Vec::new();
        let mut closed = false;
        let mut busy = session.transfer.is_some() || session.loading;

        while let Ok(event) = session.worker.try_recv() {
            match event {
                Event::Connected => {
                    session.status = Status::Connected;
                    logs.push((
                        format!("Connected to {}", session.profile.display_name()),
                        LogLevel::Success,
                    ));
                    if session.profile.protocol.browsable() {
                        let cwd = session.cwd.clone();
                        session.navigate(cwd);
                    }
                }

                Event::ConnectFailed(message) => {
                    logs.push((format!("Connection failed: {message}"), LogLevel::Error));
                    closed = true;
                }

                Event::Listing { path, mut entries } => {
                    entries.sort_by(|a, b| {
                        b.is_dir
                            .cmp(&a.is_dir)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    });
                    session.cwd = path;
                    session.entries = entries;
                    session.selection.clear();
                    session.anchor = None;
                    session.loading = false;
                }

                Event::Progress {
                    transferred,
                    total,
                    label,
                } => {
                    session.transfer = Some(Transfer {
                        label,
                        transferred,
                        total,
                    });
                }

                Event::TransferDone { label } => {
                    session.transfer = None;
                    logs.push((format!("Finished {label}"), LogLevel::Success));
                    session.refresh();
                }

                Event::ExecOutput(output) => {
                    logs.push((output, LogLevel::Info));
                }

                Event::Error(message) => {
                    session.loading = false;
                    logs.push((message, LogLevel::Error));
                }

                Event::Disconnected => {
                    logs.push(("Disconnected".to_owned(), LogLevel::Info));
                    closed = true;
                }
            }
        }

        busy |= self
            .session
            .as_ref()
            .is_some_and(|s| s.transfer.is_some() || s.loading);

        for (text, level) in logs {
            self.push_log(text, level);
        }
        if closed {
            self.session = None;
        }

        if busy {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_worker(&ui.ctx().clone());

        if let Some(action) = ui::root(self, ui) {
            self.apply(action);
        }
    }
}

pub fn join_path(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "/" {
        format!("/{}", name.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            dir.trim_end_matches('/'),
            name.trim_start_matches('/')
        )
    }
}

pub fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        None | Some(0) => "/".to_owned(),
        Some(index) => trimmed[..index].to_owned(),
    }
}

pub fn human_size(bytes: u64) -> String {
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

pub fn relative_time(instant: Instant) -> String {
    let secs = instant.elapsed().as_secs();
    match secs {
        0..=59 => "just now".to_owned(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

pub fn ellipsize(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        let keep = max_chars.saturating_sub(1);
        text.chars().take(keep).collect::<String>() + "\u{2026}"
    }
}
