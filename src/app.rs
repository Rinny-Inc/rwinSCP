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

#[derive(Clone, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
}
impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::Upload => "up",
            Direction::Download => "down",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum TransferState {
    Queued,
    Running,
    Done,
    Failed,
}

pub struct TransferRecord {
    pub label: String,
    pub host: String,
    pub direction: Direction,
    pub bytes: u64,
    pub total: Option<u64>,
    pub state: TransferState,
    pub at: Instant,
}

impl TransferRecord {
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total?;
        if total == 0 {
            return None;
        }
        Some((self.bytes as f32 / total as f32).clamp(0.0, 1.0))
    }

    pub fn bytes_per_second(&self) -> f64 {
        let seconds = self.at.elapsed().as_secs_f64();
        if seconds <= 0.0 {
            return 0.0;
        }
        self.bytes as f64 / seconds
    }

    pub fn eta_seconds(&self) -> Option<u64> {
        if self.state != TransferState::Running {
            return None;
        }

        let total = self.total?;
        let rate = self.bytes_per_second();
        if rate <= 0.0 || self.bytes >= total {
            return None;
        }
        Some(((total - self.bytes) as f64 / rate).ceil() as u64)
    }
}

pub struct PendingHostKey {
    pub session: usize,
    pub host: String,
    pub fingerprint: String,
    pub key_type: String,
}

pub struct PendingTransfer {
    profile: ConnectionProfile,
    command: Command,
    record: usize,
}

pub struct TransferJob {
    worker: WorkerHandle,
    record: usize,
    seen: std::collections::HashMap<String, u64>,
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
    UploadFolder,
    TrustHostKey,
    RejectHostKey,
    DroppedFiles(Vec<std::path::PathBuf>),
    ToggleHistory,
    ClearHistory,
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
    pub history: Vec<TransferRecord>,
    jobs: Vec<TransferJob>,
    queue: std::collections::VecDeque<PendingTransfer>,
    pub pending_host_key: Option<PendingHostKey>,
    pub show_history: bool,
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
            history: Vec::new(),
            jobs: Vec::new(),
            queue: std::collections::VecDeque::new(),
            pending_host_key: None,
            show_history: false,
            show_log: true,
        }
    }
}

const LOG_CAPACITY: usize = 500;
pub const HISTORY_CAPACITY: usize = 75;
pub const MAX_CONCURRENT_TRANSFERS: usize = 3;

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

    fn queue_transfer(
        &mut self,
        label: String,
        direction: Direction,
        command: Command,
        total: Option<u64>,
    ) {
        let Some(session) = self.session() else {
            return;
        };
        let host = session.profile.display_name().to_owned();
        let profile = session.profile.clone();

        self.history.push(TransferRecord {
            label,
            host,
            direction,
            bytes: 0,
            total,
            state: TransferState::Queued,
            at: Instant::now(),
        });
        if self.history.len() > HISTORY_CAPACITY {
            let excess = self.history.len() - HISTORY_CAPACITY;
            self.history.drain(..excess);
            for job in &mut self.jobs {
                job.record = job.record.saturating_sub(excess);
            }
            for pending in &mut self.queue {
                pending.record = pending.record.saturating_sub(excess);
            }
        }

        let record = self.history.len() - 1;
        self.queue.push_back(PendingTransfer {
            profile,
            command,
            record,
        });

        self.show_history = true;
        self.start_queued();
    }

    fn start_queued(&mut self) {
        while self.jobs.len() < MAX_CONCURRENT_TRANSFERS {
            let Some(pending) = self.queue.pop_front() else {
                return;
            };
            let job = TransferJob {
                worker: backend::spawn(pending.profile),
                record: pending.record,
                seen: std::collections::HashMap::new(),
            };
            if let Some(record) = self.history.get_mut(pending.record) {
                record.state = TransferState::Running;
                record.at = Instant::now();
            }
            job.worker.send(pending.command);
            self.jobs.push(job);
        }
    }

    fn poll_transfers(&mut self, ctx: &egui::Context) {
        let mut finished_indices = Vec::new();
        let mut logs: Vec<(String, LogLevel)> = Vec::new();
        let mut busy = false;

        for (index, job) in self.jobs.iter_mut().enumerate() {
            let mut done = false;

            while let Ok(event) = job.worker.try_recv() {
                match event {
                    Event::Connected => {}
                    Event::Progress {
                        transferred,
                        total,
                        label,
                    } => {
                        if let Some(record) = self.history.get_mut(job.record)
                            && record.total.is_none()
                            && total > 0
                            && label == record.label
                        {
                            record.total = Some(total);
                        }
                        job.seen.insert(label, transferred);
                        busy = true;
                    }
                    Event::TransferDone { label } => {
                        if let Some(record) = self.history.get_mut(job.record) {
                            record.bytes = job.seen.values().sum();
                            record.state = TransferState::Done;
                            logs.push((format!("Finished {label}"), LogLevel::Success));
                        }
                        done = true;
                    }
                    Event::ConnectFailed(message) | Event::Error(message) => {
                        if let Some(record) = self.history.get_mut(job.record) {
                            record.state = TransferState::Failed;
                            logs.push((format!("{}: {message}", record.label), LogLevel::Error));
                        }
                        done = true;
                    }
                    Event::Disconnected => done = true,

                    _ => {}
                }
            }

            if !done && let Some(record) = self.history.get_mut(job.record) {
                let moved: u64 = job.seen.values().sum();
                if moved != record.bytes {
                    record.bytes = moved;
                    busy = true;
                }
            }

            if done {
                job.worker.send(Command::Disconnect);
                finished_indices.push(index);
            }
        }

        for index in finished_indices.into_iter().rev() {
            self.jobs.remove(index);
        }
        for (text, level) in logs {
            self.push_log(text, level);
        }

        self.start_queued();

        if busy || !self.jobs.is_empty() {
            ctx.request_repaint();
        }
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
            Action::Upload => self.upload_files(false),
            Action::UploadFolder => self.upload_files(true),
            Action::DroppedFiles(paths) => self.upload_paths(paths),
            Action::ToggleHistory => self.show_history = !self.show_history,
            Action::ClearHistory => self.history.clear(),

            Action::TrustHostKey => {
                if let Some(pending) = self.pending_host_key.take()
                    && let Some(session) = self.sessions.get(pending.session)
                {
                    session.worker.send(Command::TrustHostKey);
                    self.info(format!("Trusted the host key for {}", pending.host));
                }
            }
            Action::RejectHostKey => {
                if let Some(pending) = self.pending_host_key.take() {
                    self.error(format!("Refused the host key for {}", pending.host));
                    if pending.session < self.sessions.len() {
                        self.close_session(pending.session);
                    }
                }
            }

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
        let targets: Vec<(String, String)> = session
            .selected_entries()
            .map(|entry| (join_path(&session.cwd, &entry.name), entry.name.clone()))
            .collect();

        if targets.is_empty() {
            return;
        }

        let Some(dir) = rfd::FileDialog::new().pick_folder() else {
            return;
        };

        for (remote_path, name) in targets {
            let local_path = dir.join(&name);
            self.queue_transfer(
                remote_path.clone(),
                Direction::Download,
                Command::Download {
                    remote_path,
                    local_path,
                },
                None,
            );
        }
    }

    fn upload_files(&mut self, folder: bool) {
        let dialog = rfd::FileDialog::new();
        let picked = if folder {
            dialog.pick_folders().unwrap_or_default()
        } else {
            dialog.pick_files().unwrap_or_default()
        };

        self.upload_paths(picked);
    }

    pub fn upload_paths(&mut self, paths: Vec<std::path::PathBuf>) {
        let Some(session) = self.session() else {
            return;
        };
        let cwd = session.cwd.clone();
        let mut skipped = 0;
        let mut queued = Vec::new();
        for local_path in paths {
            match local_path.file_name().and_then(|n| n.to_str()) {
                Some(name) => queued.push((local_path.clone(), join_path(&cwd, name))),
                None => skipped += 1,
            }
        }

        if skipped > 0 {
            self.error(format!(
                "Skipped {skipped} item(s) whose names are not valid UTF-8"
            ));
        }

        for (local_path, remote_path) in queued {
            let total = local_size(&local_path);
            self.queue_transfer(
                remote_path.clone(),
                Direction::Upload,
                Command::Upload {
                    local_path,
                    remote_path: remote_path.clone(),
                },
                total,
            );
        }
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
        let mut prompts: Vec<PendingHostKey> = Vec::new();
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

                    Event::HostKeyUnknown {
                        host,
                        fingerprint,
                        key_type,
                    } => {
                        prompts.push(PendingHostKey {
                            session: index,
                            host,
                            fingerprint,
                            key_type,
                        });
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

                    Event::Progress { .. } => {}
                    Event::TransferDone { .. } => session.refresh(),

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

            busy |= session.loading;
        }

        for (text, level) in logs {
            self.push_log(text, level);
        }
        if self.pending_host_key.is_none()
            && let Some(prompt) = prompts.into_iter().next()
        {
            self.pending_host_key = Some(prompt);
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
        let ctx = ui.ctx().clone();
        self.poll_worker(&ctx);
        self.poll_transfers(&ctx);

        if let Some(action) = ui::root(self, ui) {
            self.apply(action);
        }
    }
}

fn local_size(path: &std::path::Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_file() {
        return Some(meta.len());
    }

    let mut total = 0;
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        total += local_size(&entry.path())?;
    }
    Some(total)
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
