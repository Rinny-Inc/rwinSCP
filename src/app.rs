use std::collections::HashSet;
use std::time::Instant;

use crate::backend::{self, Command, Event, RemoteEntry, WorkerHandle};
use crate::connection::{Auth, ConnectionProfile, Protocol};
use crate::ui;

pub struct Host {
    pub id: String,
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

pub struct Terminal {
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKey {
    pub host: usize,
    pub shell: bool,
}

pub struct Session {
    pub key: SessionKey,
    pub profile: ConnectionProfile,
    pub worker: WorkerHandle,
    pub status: Status,
    pub cwd: String,
    pub entries: Vec<RemoteEntry>,
    pub selection: HashSet<usize>,
    pub anchor: Option<usize>,
    pub transfer: Option<Transfer>,
    pub loading: bool,
    pub terminal: Option<Terminal>,
    pub path_edit: Option<String>,
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
    OpenShell(usize),
    SelectTab(Option<usize>),
    CloseTab(usize),
    CancelEdit,
    SaveDraft,
    SaveDraftAndConnect,
    Disconnect,
    Navigate(String),
    Refresh,
    EditPath,
    CommitPath,
    CancelPath,
    ShellBytes(String),
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
    pub sessions: Vec<Session>,
    pub active: Option<usize>,
    pub log: Vec<LogLine>,
    pub show_log: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            search: String::new(),
            draft: None,
            sessions: Vec::new(),
            active: None,
            log: Vec::new(),
            show_log: true,
        }
    }
}

const LOG_CAPACITY: usize = 500;

impl App {
    pub fn load() -> Self {
        let hosts = crate::store::load();
        let restored = hosts.len();
        let mut app = Self {
            hosts,
            ..Self::default()
        };
        if restored > 0 {
            app.info(format!(
                "Restored {restored} host{}",
                if restored == 1 { "" } else { "s" }
            ));
        }
        app
    }

    fn persist(&mut self) {
        if let Err(e) = crate::store::save(&self.hosts) {
            self.error(format!("Could not save hosts: {e}"));
            return;
        }

        let unsaved: Vec<String> = self
            .hosts
            .iter()
            .filter(|host| !crate::store::save_secret(host))
            .map(|host| host.profile.display_name().to_owned())
            .collect();

        if !unsaved.is_empty() {
            self.error(format!("Hosts saved, but the keychain refused credentials for {}; you will be asked for them again", unsaved.join(", ")));
        }
    }

    pub fn session(&self) -> Option<&Session> {
        self.active.and_then(|i| self.sessions.get(i))
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.active.and_then(|i| self.sessions.get_mut(i))
    }

    fn close_session(&mut self, index: usize) {
        if index >= self.sessions.len() {
            return;
        }
        let session = self.sessions.remove(index);
        session.worker.send(Command::Disconnect);

        self.active = match self.active {
            Some(active) if active == index => {
                index.checked_sub(1).or(if self.sessions.is_empty() {
                    None
                } else {
                    Some(0)
                })
            }
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
    }

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
                    crate::store::forget_secret(&host.id);
                    self.persist();
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
                if self.commit_draft().is_some() {
                    self.persist();
                }
            }

            Action::SaveDraftAndConnect => {
                if let Some(index) = self.commit_draft() {
                    self.persist();
                    self.connect(index, None);
                }
            }

            Action::Connect(index) => self.connect(index, None),

            Action::OpenShell(index) => self.connect(index, Some(Protocol::Ssh)),

            Action::Disconnect => {
                self.active = None;
            }

            Action::SelectTab(index) => {
                self.active = index.filter(|i| *i < self.sessions.len());
            }
            Action::CloseTab(index) => self.close_session(index),

            Action::Navigate(path) => {
                if let Some(session) = self.session_mut() {
                    session.navigate(path);
                }
            }

            Action::ShellBytes(bytes) => {
                if let Some(session) = &self.session()
                    && session.terminal.is_some()
                {
                    session.worker.send(Command::ShellInput(bytes));
                }
            }

            Action::EditPath => {
                if let Some(session) = self.session_mut() {
                    session.path_edit = Some(session.cwd.clone());
                }
            }
            Action::CommitPath => {
                if let Some(session) = self.session_mut()
                    && let Some(typed) = session.path_edit.take()
                {
                    let target = typed.trim();
                    let target = if target.is_empty() { "/" } else { target };
                    session.navigate(target.to_owned());
                }
            }
            Action::CancelPath => {
                if let Some(session) = self.session_mut() {
                    session.path_edit = None;
                }
            }

            Action::Refresh => {
                if let Some(session) = self.session_mut() {
                    session.refresh();
                }
            }

            Action::ClickRow(index, modifiers) => {
                if let Some(session) = self.session_mut() {
                    session.click_row(index, modifiers);
                }
            }

            Action::OpenRow(index) => {
                if let Some(session) = self.session_mut()
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
                if let Some(session) = self.session_mut() {
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
        let (mut profile, editing) = self.draft.take()?;
        profile.normalize_endpoint();
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
                    id: crate::store::new_id(),
                    profile,
                    last_used: None,
                });
                Some(self.hosts.len() - 1)
            }
        }
    }

    fn missing_secret(profile: &ConnectionProfile) -> bool {
        match &profile.auth {
            Auth::Password(pswrd) => pswrd.is_empty(),
            Auth::KeyFile { path, .. } => path.is_empty(),
            Auth::S3Keys { secret_key, .. } => secret_key.is_empty(),
        }
    }

    fn connect(&mut self, index: usize, as_protocol: Option<Protocol>) {
        let shell = as_protocol == Some(Protocol::Ssh)
            || self
                .hosts
                .get(index)
                .is_some_and(|h| h.profile.protocol == Protocol::Ssh);
        let key = SessionKey { host: index, shell };

        if let Some(existing) = self.sessions.iter().position(|s| s.key == key) {
            self.active = Some(existing);
            return;
        }
        let Some(host) = self.hosts.get(index) else {
            return;
        };
        if Self::missing_secret(&host.profile) {
            let name = host.profile.display_name().to_owned();
            self.error(format!("No stored credentials for {name} -- open its Edit form and enter them again. \
                                     (Saved hosts keep secrets in the OS keychain, which can refuse to forget them.)"));
            return;
        }
        let Some(host) = self.hosts.get_mut(index) else {
            return;
        };
        host.last_used = Some(Instant::now());

        let mut profile = host.profile.clone();
        if let Some(protocol) = as_protocol {
            profile.protocol = protocol;
        }
        let start_dir = profile.remote_start_dir.clone();
        let is_shell = profile.protocol == Protocol::Ssh;
        self.info(format!("Connecting to {}...", profile.endpoint()));

        self.sessions.push(Session {
            key,
            worker: backend::spawn(profile.clone()),
            profile,
            status: Status::Connecting,
            cwd: start_dir,
            entries: Vec::new(),
            selection: HashSet::new(),
            anchor: None,
            transfer: None,
            loading: true,
            terminal: is_shell.then(|| Terminal {
                output: String::new(),
            }),
            path_edit: None,
        });
        self.active = Some(self.sessions.len() - 1);
    }

    fn download_selection(&mut self) {
        let Some(session) = self.session() else {
            return;
        };
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
        let Some(session) = self.session() else {
            return;
        };
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
        let Some(session) = self.session() else {
            return;
        };
        let targets: Vec<(String, bool)> = session
            .selected_entries()
            .map(|entry| (join_path(&session.cwd, &entry.name), entry.is_dir))
            .collect();

        for (path, is_dir) in targets {
            session.worker.send(Command::Delete { path, is_dir });
        }

        if let Some(session) = self.session_mut() {
            session.refresh();
        }
    }

    fn poll_worker(&mut self, ctx: &egui::Context) {
        let mut logs: Vec<(String, LogLevel)> = Vec::new();
        let mut closed: Vec<usize> = Vec::new();
        let mut busy = false;

        for (index, session) in self.sessions.iter_mut().enumerate() {
            let name = session.profile.display_name().to_owned();

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
                        } else {
                            session.loading = false;
                        }
                    }

                    Event::ConnectFailed(message) => {
                        logs.push((format!("{name}: {message}"), LogLevel::Error));
                        closed.push(index);
                    }

                    Event::Listing { path, mut entries } => {
                        entries.sort_by(|a, b| {
                            b.is_dir
                                .cmp(&a.is_dir)
                                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                        });
                        session.cwd = path;
                        session.entries = entries;
                        session.path_edit = None;
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

                    Event::ShellOutput(chunk) => {
                        if let Some(terminal) = &mut session.terminal {
                            append_terminal_output(&mut terminal.output, &chunk);
                        }
                        busy = true;
                    }

                    Event::Error(message) => {
                        session.loading = false;
                        logs.push((message, LogLevel::Error));
                    }

                    Event::Disconnected => {
                        logs.push((format!("{name} disconnected"), LogLevel::Info));
                        closed.push(index);
                    }
                }
            }

            busy |= session.transfer.is_some() || session.loading;
        }

        for (text, level) in logs {
            self.push_log(text, level);
        }

        closed.sort_unstable();
        closed.dedup();
        for index in closed.into_iter().rev() {
            if index < self.sessions.len() {
                self.sessions.remove(index);
            }
            self.active = match self.active {
                Some(active) if active == index => None,
                Some(active) if active > index => Some(active - 1),
                other => other,
            };
        }

        if self.active.is_none_or(|i| i >= self.sessions.len()) && !self.sessions.is_empty() {
            self.active = self.active.filter(|i| *i < self.sessions.len());
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

const TERMINAL_CAPACITY: usize = 120_000;

pub fn append_terminal_output(buffer: &mut String, chunk: &str) {
    let mut chars = chunk.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\u{1B}' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7E}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1B}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                    buffer.push('\n');
                } else {
                    let line_start = buffer.rfind('\n').map_or(0, |i| i + 1);
                    buffer.truncate(line_start);
                }
            }
            '\u{8}' => {
                buffer.pop();
            }

            c if c.is_control() && c != '\n' && c != '\t' => {}

            c => buffer.push(c),
        }
    }

    if buffer.len() > TERMINAL_CAPACITY {
        let excess = buffer.len() - TERMINAL_CAPACITY;
        let cut = buffer[excess..]
            .find('\n')
            .map_or(excess, |i| excess + i + 1);
        buffer.drain(..cut);
    }
}
